//! Tools for representing, storing, and connecting game objects.
//!
//! A game object in Starframe is a _directed graph_ of components.
//! Conceptually, one may look something like this:
//! ```text
//!   Pose <----> RigidBody <----> Collider
//!    ^              ^
//!    |              |
//!    v              v
//!  Sprite       EventSink
//!    |
//!    |
//!    v
//! Texture
//! ```
//! Most edges (connections between nodes) should go both ways, but because the graph is directed, one-directional edges
//! such as from Sprite to Texture in the above diagram can also be made.
//! This is important to the way object boundaries are determined by the deletion algorithm
//! (see `Graph::delete` for details).
//! This comes into play, for example, when sharing one component between multiple objects.
//!
//! Edges are stored in the `Graph`, while components themselves are stored in `Layer`s.
//! This separation allows general graph algorithms to be used on the `Graph`
//! without knowing anything about the types of the components.
//!
//! Similarly to how systems in ECS iterate over specific sets of components,
//! systems using the graph iterate over specific patterns of connected nodes.
//! This is detailed in the `Iter` documentation.

use std::collections::VecDeque;
use std::marker::PhantomData;

//
// Index & ref types
//

type ComponentIdx = usize;
type LayerIdx = usize;
type GenerationIdx = usize;
type Refcount = usize;

/// Implemented by all node types that can be used to query the graph.
///
/// Types that can be stored between frames, and therefore don't know if the node they point to has been deleted,
/// only implement this and not `SafeNode`.
pub trait UnsafeNode {
    fn pos(&self) -> NodePosition;
}

/// Implemented by node types that know the node they point to has not been deleted.
///
/// Note that the above is not currently strictly true.
/// For instance, you can clone a `Node`, get a `CheckedNode` from both, delete one,
/// and the other will now point to a deleted node.
/// However, the consequences of this aren't severe and it takes some strange moves to happen at all,
/// so I'm willing to live with this API for now.
pub trait SafeNode: UnsafeNode + Sized {
    type MarkerType;
    fn pin(&self, graph: &mut Graph) -> PinnedNode<Self::MarkerType> {
        graph.pin(self)
    }
}

/// The position in the graph of a node of any type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodePosition {
    pub(crate) layer_idx: LayerIdx,
    pub(crate) item_idx: ComponentIdx,
}
impl UnsafeNode for NodePosition {
    fn pos(&self) -> NodePosition {
        *self
    }
}

/// An index to a graph node that is safe to keep across frame boundaries.
/// Can be used to retrieve the component it represents from the corresponding `Layer`.
///
/// A Node contains the position of the node, a type marker of the component's type to prevent use with a wrong layer,
/// and a generation index to check that the node hasn't been deleted.
/// An example use case could be to use them as a kind of "weak pointer" connecting together different objects
/// where you don't want both to be deleted if one is.
pub struct Node<T> {
    pos: NodePosition,
    gen: GenerationIdx,
    _marker: PhantomData<*const T>,
}
impl<T> Node<T> {
    /// Returns a `CheckedNode`, which implements `SafeNode` and can be used in graph operations,
    /// or `None` if the node has been deleted from the graph.
    pub fn check(&self, graph: &Graph) -> Option<CheckedNode<'_, T>> {
        if graph.generations[self.pos.layer_idx][self.pos.item_idx] == self.gen {
            Some(CheckedNode { node: self })
        } else {
            None
        }
    }
}
impl<T> UnsafeNode for Node<T> {
    fn pos(&self) -> NodePosition {
        self.pos
    }
}
// blanket impls required because derive restricts type of T
impl<T> Clone for Node<T> {
    fn clone(&self) -> Self {
        Node {
            pos: self.pos,
            gen: self.gen,
            _marker: PhantomData,
        }
    }
}
impl<T> Copy for Node<T> {}
impl<T> std::fmt::Debug for Node<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Node {{\n pos: {:?},\n gen: {},\n}}",
            self.pos, self.gen
        ))
    }
}
impl<T> PartialEq for Node<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pos == other.pos && self.gen == other.gen
    }
}
impl<T> Eq for Node<T> {}
impl<T> std::hash::Hash for Node<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pos.hash(state);
    }
}

/// A `Node` that is known to be alive.
#[derive(Debug)]
pub struct CheckedNode<'a, T> {
    node: &'a Node<T>,
}
impl<'a, T> UnsafeNode for CheckedNode<'a, T> {
    fn pos(&self) -> NodePosition {
        self.node.pos
    }
}
impl<'a, T> SafeNode for CheckedNode<'a, T> {
    type MarkerType = T;
}

/// A variant of `Node` that cannot be deleted.
///
/// If you drop this without calling `Graph::unpin`, the node it represents can never be deleted.
/// After unpinning, the node will once again be deleted normally if all connections to it are deleted.
/// If all connections are already gone when you call unpin, the node will immediately be deleted.
pub struct PinnedNode<T> {
    pos: NodePosition,
    _marker: PhantomData<*const T>,
}
impl<T> UnsafeNode for PinnedNode<T> {
    fn pos(&self) -> NodePosition {
        self.pos
    }
}
impl<T> SafeNode for PinnedNode<T> {
    type MarkerType = T;
}

/// A reference to a component in a `Layer`
/// that knows its position in the graph and can be used in graph operations.
pub struct NodeRef<'a, T> {
    item: &'a T,
    pos: NodePosition,
}
impl<'a, T> std::ops::Deref for NodeRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}
impl<'a, T> UnsafeNode for NodeRef<'a, T> {
    fn pos(&self) -> NodePosition {
        self.pos
    }
}
impl<'a, T> SafeNode for NodeRef<'a, T> {
    type MarkerType = T;
}
impl<'a, T> NodeRef<'a, T> {
    pub fn as_node(nr: &Self, graph: &Graph) -> Node<T> {
        Node {
            pos: nr.pos,
            gen: graph.generations[nr.pos.layer_idx][nr.pos.item_idx],
            _marker: PhantomData,
        }
    }
}

/// A mutable reference to a component in a `Layer`
/// that knows its position in the graph and can be used in graph operations.
pub struct NodeRefMut<'a, T> {
    // exposed to crate to allow a trick in `event.rs`
    pub(crate) item: &'a mut T,
    pos: NodePosition,
}
impl<'a, T> std::ops::Deref for NodeRefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}
impl<'a, T> std::ops::DerefMut for NodeRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}
impl<'a, T> UnsafeNode for NodeRefMut<'a, T> {
    fn pos(&self) -> NodePosition {
        self.pos
    }
}
impl<'a, T> SafeNode for NodeRefMut<'a, T> {
    type MarkerType = T;
}
impl<'a, T> NodeRefMut<'a, T> {
    pub fn as_node(nrm: &Self, graph: &Graph) -> Node<T> {
        Node {
            pos: nrm.pos,
            gen: graph.generations[nrm.pos.layer_idx][nrm.pos.item_idx],
            _marker: PhantomData,
        }
    }
}

//
// Graph
//

/// The heart of the Starframe game object representation.
/// Contains connections between components stored in `Layer`s.
///
/// The usual way to use this is to define a struct containing a `Graph` and a `Layer` for each component type:
/// ```
/// # use starframe::graph::{Graph, Layer};
/// # use starframe::physics::{Collider, RigidBody};
/// # use starframe::math::Pose;
/// struct MyGraph {
///     graph: Graph,
///     l_pose: Layer<Pose>,
///     l_collider: Layer<Collider>,
///     l_body: Layer<RigidBody>,
///     // etc.
/// }
/// impl MyGraph {
///     pub fn new() -> Self {
///         let mut graph = Graph::new();
///         let l_pose = graph.create_layer();
///         let l_collider = graph.create_layer();
///         let l_body = graph.create_layer();
///         MyGraph {
///             graph,
///             l_pose,
///             l_collider,
///             l_body,
///         }
///     }
/// }
/// ```
/// If multiple instances of `Graph` exist in one program,
/// care must be taken not to mix nodes or layers from different instances.
/// Doing so will either panic or cause strange behavior depending on what's in the two graphs.
#[derive(Debug)]
pub struct Graph {
    /// 3D array:
    /// * 1st dimension is the starting layer
    /// * 2nd dimension is the target layer
    /// * 3rd dimension is the component on the starting layer
    /// * and the stored value is the index of the component on the ending layer
    /// used to connect nodes
    edge_layers: Vec<Vec<Vec<Option<ComponentIdx>>>>,
    /// 2D array:
    /// * 1st dimension is the layer
    /// * 2nd dimension is the component
    /// used to determine if an object is dead or alive
    refcounts: Vec<Vec<Refcount>>,
    /// Same structure as above,
    /// used to invalidate slots that have been deleted and maybe recycled
    generations: Vec<Vec<GenerationIdx>>,
    /// FIFO queue for slot reuse
    vacant_slots: Vec<VecDeque<ComponentIdx>>,
}

impl Graph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            edge_layers: Vec::new(),
            refcounts: Vec::new(),
            generations: Vec::new(),
            vacant_slots: Vec::new(),
        }
    }

    /// Create a new `Layer` in this graph.
    pub fn create_layer<T>(&mut self) -> Layer<T> {
        let next_idx = self.edge_layers.len();

        // add this as a target layer to all layers that already exist
        for layer in &mut self.edge_layers {
            layer.push(Vec::new());
        }
        // for the new layer, add a target layer for each of the already existing ones plus itself
        let targets = vec![Vec::new(); next_idx + 1];
        self.edge_layers.push(targets);

        // add refcounts, generation indices and vacant slot queues for the layer
        self.refcounts.push(Vec::new());
        self.generations.push(Vec::new());
        self.vacant_slots.push(VecDeque::new());

        Layer {
            index: next_idx,
            content: Vec::new(),
        }
    }

    /// Create two edges, one in both directions, between two nodes.
    ///
    /// This makes both nodes hierarchically equal parts of the same object.
    /// Both nodes can find each other with `Graph::get_neighbor`,
    /// and both will (in most cases) be deleted if `Graph::delete` is called on either one.
    ///
    /// Internally this calls `connect_oneway` twice, so all the same caveats apply.
    ///
    /// # Panics
    /// Panics if either edge this creates would make `connect_oneway` panic.
    pub fn connect(&mut self, node1: &impl SafeNode, node2: &impl SafeNode) {
        self.connect_oneway(node1, node2);
        self.connect_oneway(node2, node1);
    }

    /// Create an edge from one node to another.
    ///
    /// This makes the second node hierarchically lower than the first, in a sense.
    /// It can be used to share one node between multiple objects in such a way that the shared node
    /// is only deleted when the last object referring to it is deleted.
    ///
    /// Edges are stored in `Vec`s in the same order as components in `Layer`s.
    /// Thus, an allocation may be triggered here if the starting node is the last one on its layer
    /// to have an edge to the target layer.
    ///
    /// # Panics
    /// The current graph implementation is limited to one edge from one node to one layer.
    /// Therefore, you cannot do things like pointing from one `Transform` directly to two `Shape`s.
    /// If an edge from one of the nodes to the other's layer already exists, this function will panic,
    /// because this signals that you're creating a malformed object that won't work the way you expect.
    pub fn connect_oneway(&mut self, start: &impl SafeNode, end: &impl SafeNode) {
        let start = start.pos();
        let end = end.pos();
        let edge_vec = &mut self.edge_layers[start.layer_idx][end.layer_idx];
        // extend the edge vec when adding an edge past its current end.
        // we don't allocate all the space at the start because it's likely to not get used
        if edge_vec.len() <= start.item_idx {
            if edge_vec.len() != start.item_idx {
                edge_vec.resize_with(start.item_idx, || None);
            }
            edge_vec.push(Some(end.item_idx));
        } else {
            let prev_val = edge_vec[start.item_idx].replace(end.item_idx);
            assert!(
                prev_val.is_none(),
                "Attempted to overwrite an edge. \
                If you're trying to do shared ownership, use `connect_oneway`."
            );
        }

        self.refcounts[end.layer_idx][end.item_idx] += 1;
    }

    /// If an edge from the given node to the target layer exists, returns the node it points to.
    /// This method takes a node type that implements `SafeNode`, meaning it knows it's currently alive.
    /// See the docs for `SafeNode` and the available node types.
    pub fn get_neighbor<'to, To>(
        &self,
        node: &impl SafeNode,
        to_layer: &'to Layer<To>,
    ) -> Option<NodeRef<'to, To>> {
        self.get_neighbor_unchecked(node, to_layer)
    }

    /// Unchecked variant of `get_neighbor`.
    /// This method takes any node type, including ones that don't know for sure they're alive.
    ///
    /// All `Graph` methods that take nodes follow this convention —
    /// a `SafeNode` variant without a prefix and an `UnsafeNode` variant called `<name>-unchecked`.
    pub fn get_neighbor_unchecked<'to, To>(
        &self,
        node: &impl UnsafeNode,
        to_layer: &'to Layer<To>,
    ) -> Option<NodeRef<'to, To>> {
        let node = node.pos();
        let edge_layer = &self.edge_layers[node.layer_idx][to_layer.index];
        if edge_layer.len() <= node.item_idx {
            None
        } else {
            let to_id = edge_layer[node.item_idx]?;
            Some(NodeRef {
                item: &to_layer.content[to_id],
                pos: NodePosition {
                    item_idx: to_id,
                    layer_idx: to_layer.index,
                },
            })
        }
    }

    pub fn get_neighbor_mut<'to, To>(
        &self,
        node: &impl SafeNode,
        to_layer: &'to mut Layer<To>,
    ) -> Option<NodeRefMut<'to, To>> {
        self.get_neighbor_mut_unchecked(node, to_layer)
    }

    pub fn get_neighbor_mut_unchecked<'to, To>(
        &self,
        node: &impl UnsafeNode,
        to_layer: &'to mut Layer<To>,
    ) -> Option<NodeRefMut<'to, To>> {
        let node = node.pos();
        let edge_layer = &self.edge_layers[node.layer_idx][to_layer.index];
        if edge_layer.len() <= node.item_idx {
            None
        } else {
            let to_id = edge_layer[node.item_idx]?;
            Some(NodeRefMut {
                item: &mut to_layer.content[to_id],
                pos: NodePosition {
                    item_idx: to_id,
                    layer_idx: to_layer.index,
                },
            })
        }
    }

    /// Pin a node, guaranteeing that it won't be deleted. See `PinnedNode`.
    pub fn pin<N: SafeNode>(&mut self, node: &N) -> PinnedNode<N::MarkerType> {
        let pos = node.pos();
        // we don't need a whole layer for pins because nothing ever needs to connect to a pin source.
        // so we just increment refcount by 1 when we pin to ensure it always stays above 0
        self.refcounts[pos.layer_idx][pos.item_idx] += 1;
        PinnedNode {
            pos,
            _marker: PhantomData,
        }
    }

    /// Unpin a pinned node, making it able to be deleted again.
    pub fn unpin<T>(&mut self, pin: PinnedNode<T>) {
        self.refcounts[pin.pos.layer_idx][pin.pos.item_idx] -= 1;

        if self.refcounts[pin.pos.layer_idx][pin.pos.item_idx] == 0 {
            self.vacant_slots[pin.pos.layer_idx].push_back(pin.pos.item_idx);
            self.generations[pin.pos.layer_idx][pin.pos.item_idx] += 1;
        }
    }

    /// Get the number of edges pointing towards the given node.
    pub fn get_refcount(&self, node: &impl SafeNode) -> Refcount {
        self.get_refcount_unchecked(node)
    }

    pub fn get_refcount_unchecked(&self, node: &impl UnsafeNode) -> Refcount {
        let node = node.pos();
        self.refcounts[node.layer_idx][node.item_idx]
    }

    /// Delete a whole _object_ from the graph, beginning from the given node.
    ///
    /// What constitutes a single object in the graph isn't quite straightforward due to the
    /// number of ways in which nodes can be connected.
    /// The deletion algorithm traverses every node it can find by recursively following edges from the starting node,
    /// deleting edges along the way.
    ///
    /// A node is considered deleted once it no longer has any edges pointing to it.
    /// At this point, any `Node`s referring to it become invalidated (will not pass `check`)
    /// and the node is marked to be reused by a new component later.
    /// If there are edges from outside of the deletion traversal's path pointing to a node that is traversed,
    /// that node and everything traversed after it will remain alive until all pointing nodes are deleted first.
    ///
    /// Illustrated example:
    /// ```text
    /// O<->(O)<->O-->O<--O<->O
    ///               ^
    ///               L->O
    /// Delete edges, starting from (O) (note how it stops on the "shared" node)
    /// O   (O)   O   O<--O<->O
    ///               ^
    ///               L->O
    /// Mark nodes without any more edges for reuse
    /// O<--O<->[O]
    /// ^
    /// L->O
    /// Delete edges, starting from [0]
    /// O   O   O
    ///
    ///    O
    /// Everything is now deleted
    /// ```
    /// There are a lot of nuances to this depending on object structure, but the vast majority of the time
    /// objects will just be their own islands in the graph where everything is connected with bidirectional edges.
    /// In these cases, the whole island will be deleted regardless of which node you start on.
    pub fn delete(&mut self, root: impl SafeNode) {
        let root = root.pos();
        // check if the node is already considered deleted before doing anything
        if self.refcounts[root.layer_idx][root.item_idx] == 0 {
            return;
        }
        let mut visited = vec![VisitedNode {
            node: root,
            visit_count: 0,
            all_refs_visited: false,
            visited_on_delete: false,
        }];

        self.visit_all(root, &mut visited);

        for vis in visited.iter_mut() {
            if self.refcounts[vis.node.layer_idx][vis.node.item_idx] == vis.visit_count {
                vis.all_refs_visited = true;
            }
        }

        self.delete_owned(0, &mut visited);

        for vis_node in visited {
            let node = vis_node.node;
            if self.refcounts[node.layer_idx][node.item_idx] == 0 {
                debug_assert!(
                    self.vacant_slots[node.layer_idx]
                        .iter()
                        .find(|&&idx| idx == node.item_idx)
                        .is_none(),
                    "Same slot marked vacant twice ({:?})",
                    node,
                );
                self.vacant_slots[node.layer_idx].push_back(node.item_idx);
                self.generations[node.layer_idx][node.item_idx] += 1;
            }
        }
    }

    fn visit_all(&self, curr_node: NodePosition, visited: &mut Vec<VisitedNode>) {
        for other_layer_idx in 0..self.edge_layers.len() {
            let edges_to_other = &self.edge_layers[curr_node.layer_idx][other_layer_idx];
            if curr_node.item_idx < edges_to_other.len() {
                if let Some(other_item_idx) = edges_to_other[curr_node.item_idx] {
                    let next_node = NodePosition {
                        layer_idx: other_layer_idx,
                        item_idx: other_item_idx,
                    };
                    if let Some(already_seen) = visited.iter_mut().find(|n| n.node == next_node) {
                        already_seen.visit_count += 1;
                    } else {
                        visited.push(VisitedNode {
                            node: next_node,
                            visit_count: 1,
                            all_refs_visited: false,
                            visited_on_delete: false,
                        });
                        self.visit_all(next_node, visited);
                    }
                }
            }
        }
    }

    fn delete_owned(&mut self, curr_visited_idx: usize, visited: &mut Vec<VisitedNode>) {
        if visited[curr_visited_idx].visited_on_delete {
            // we already went over this one
            return;
        }
        visited[curr_visited_idx].visited_on_delete = true;

        if !visited[curr_visited_idx].all_refs_visited {
            // this node is shared, don't delete anything after it
            if curr_visited_idx == 0 {
                eprintln!(
                    "Warning: failed to delete a node due to references from outside or pinning"
                );
            }
            return;
        }

        let node = visited[curr_visited_idx].node;
        for other_layer_idx in 0..self.edge_layers.len() {
            let edges_to_other = &mut self.edge_layers[node.layer_idx][other_layer_idx];
            if node.item_idx < edges_to_other.len() {
                if let Some(other_item_idx) = edges_to_other[node.item_idx] {
                    edges_to_other[node.item_idx] = None;
                    self.refcounts[other_layer_idx][other_item_idx] -= 1;

                    let next_node = NodePosition {
                        layer_idx: other_layer_idx,
                        item_idx: other_item_idx,
                    };
                    // unwrap because visited contains every connected node
                    let visited_next = visited.iter().position(|v| v.node == next_node).unwrap();
                    self.delete_owned(visited_next, visited);
                }
            }
        }
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct VisitedNode {
    node: NodePosition,
    visit_count: Refcount,
    all_refs_visited: bool,
    visited_on_delete: bool,
}

//
// Layer
//

/// A layer of a graph, responsible for concrete storage of the components.
///
/// Components are stored contiguously in a Vec<T>.
#[derive(Debug)]
pub struct Layer<T> {
    index: LayerIdx,
    content: Vec<T>,
}

impl<T> Layer<T> {
    /// Insert a component into the `Layer`'s storage and create some tracking information in the `Graph`.
    ///
    /// If a component has been previously deleted from the layer, its slot will be reused instead of pushing to the back.
    pub fn insert(&mut self, component: T, graph: &mut Graph) -> NodeRef<'_, T> {
        let item_idx = if let Some(vacant_slot) = graph.vacant_slots[self.index].pop_front() {
            self.content[vacant_slot] = component;
            vacant_slot
        } else {
            self.content.push(component);
            graph.refcounts[self.index].push(0);
            graph.generations[self.index].push(0);
            self.content.len() - 1
        };

        NodeRef {
            item: &self.content[item_idx],
            pos: NodePosition {
                layer_idx: self.index,
                item_idx,
            },
        }
    }

    /// Get a reference to the component represented by the given node.
    pub fn get(&self, node: impl SafeNode) -> NodeRef<'_, T> {
        let pos = node.pos();
        self.get_unchecked(pos)
    }

    /// Unchecked variant of `get`, in the same sense as the unchecked methods of `Graph`.
    pub fn get_unchecked(&self, pos: NodePosition) -> NodeRef<'_, T> {
        NodeRef {
            item: &self.content[pos.item_idx],
            pos,
        }
    }

    /// Get a mutable reference to the component represented by the given node.
    pub fn get_mut(&mut self, node: impl SafeNode) -> NodeRefMut<'_, T> {
        let pos = node.pos();
        self.get_mut_unchecked(pos)
    }

    pub fn get_mut_unchecked(&mut self, pos: NodePosition) -> NodeRefMut<'_, T> {
        NodeRefMut {
            item: &mut self.content[pos.item_idx],
            pos,
        }
    }

    /// Get an iterator over the components stored in this `Layer`
    /// that are alive, that is, have at least one edge pointing to them.
    ///
    /// You can use this with `Graph::get_neighbor` to iterate over patterns in the graph:
    /// ```
    /// # use starframe::{math::Pose, physics::{Collider, RigidBody}, graph::{Graph, Layer}};
    /// # let mut graph = Graph::new();
    /// # let l_rigidbody: Layer<RigidBody> = graph.create_layer();
    /// # let l_pose: Layer<Pose> = graph.create_layer();
    /// # let l_collider: Layer<Collider> = graph.create_layer();
    /// for body in l_rigidbody.iter(&graph) {
    ///     let pose = match graph.get_neighbor(&body, &l_pose) {
    ///         Some(pose) => pose,
    ///         None => continue,
    ///     };
    ///     let coll = match graph.get_neighbor(&body, &l_collider) {
    ///         Some(coll) => coll,
    ///         None => continue,
    ///     };
    ///     // do stuff with body, pose and coll...
    /// }
    /// ```
    pub fn iter<'s, 'g: 's>(&'s self, graph: &'g Graph) -> LayerIter<'s, T> {
        LayerIter {
            iter: self.content.iter().enumerate(),
            layer_idx: self.index,
            refcounts: &graph.refcounts[self.index],
        }
    }

    pub fn iter_mut<'s, 'g: 's>(&'s mut self, graph: &'g Graph) -> LayerIterMut<'s, T> {
        LayerIterMut {
            iter: self.content.iter_mut().enumerate(),
            layer_idx: self.index,
            refcounts: &graph.refcounts[self.index],
        }
    }
}

//
// Iterators
//

/// An iterator over the components stored in a `Layer` that have at least one edge pointing to them.
#[derive(Clone, Debug)]
pub struct LayerIter<'a, T> {
    iter: std::iter::Enumerate<std::slice::Iter<'a, T>>,
    layer_idx: LayerIdx,
    refcounts: &'a Vec<Refcount>,
}
impl<'a, T> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;
        if self.refcounts[item_idx] > 0 {
            Some(NodeRef {
                item,
                pos: NodePosition {
                    item_idx,
                    layer_idx: self.layer_idx,
                },
            })
        } else {
            self.next()
        }
    }
}

/// Mutable variant of `LayerIter`.
pub struct LayerIterMut<'a, T> {
    iter: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
    layer_idx: LayerIdx,
    refcounts: &'a Vec<Refcount>,
}
impl<'a, T> Iterator for LayerIterMut<'a, T> {
    type Item = NodeRefMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;
        if self.refcounts[item_idx] > 0 {
            Some(NodeRefMut {
                item,
                pos: NodePosition {
                    item_idx,
                    layer_idx: self.layer_idx,
                },
            })
        } else {
            self.next()
        }
    }
}

//
// Tests
//

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Transform(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Velocity(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RigidBody(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Shape(usize);

    /// Creating layers generates the correct storages.
    #[test]
    fn create_layers() {
        let mut graph = Graph::new();
        let l0: Layer<Transform> = graph.create_layer();
        let l1: Layer<Velocity> = graph.create_layer();

        // both layers should now have two target layers
        assert_eq!(graph.edge_layers[l0.index].len(), 2);
        assert_eq!(graph.edge_layers[l1.index].len(), 2);

        // this should add a target layer to everyone
        let l2: Layer<Shape> = graph.create_layer();
        assert_eq!(graph.edge_layers[l0.index].len(), 3);
        assert_eq!(graph.edge_layers[l1.index].len(), 3);
        assert_eq!(graph.edge_layers[l2.index].len(), 3);
    }

    /// Nodes can be connected and then queried for their neighbors.
    /// Multiple ownership works.
    #[test]
    fn connect_nodes() {
        let mut graph = Graph::new();
        let mut poses: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph).pin(&mut graph);

        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let pose_node = poses.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(i), &mut graph);
            graph.connect(&vel_node, &pose_node);
            graph.connect(&rb_node, &pose_node);
            graph.connect(&rb_node, &vel_node);
            graph.connect_oneway(&rb_node, &everyones_shape);
            // refcounts
            assert_eq!(graph.get_refcount(&pose_node), 2);
            assert_eq!(graph.get_refcount(&rb_node), 2);
            // 1 extra from the pin
            assert_eq!(graph.get_refcount(&everyones_shape), i + 2);
            // neighbors are found
            assert_eq!(
                graph.get_neighbor(&rb_node, &shapes).unwrap().item,
                &Shape(69)
            );
            assert_eq!(
                graph.get_neighbor(&pose_node, &rbs).unwrap().item,
                &RigidBody(i)
            );
            assert!(graph.get_neighbor(&pose_node, &shapes).is_none());

            // spawn something with different connections in between
            let pose_node_ = poses.insert(Transform(42 + i), &mut graph);
            let shape_node_ = shapes.insert(Shape(i), &mut graph);
            graph.connect(&pose_node_, &shape_node_);
            assert_eq!(
                graph.get_neighbor(&pose_node_, &shapes).unwrap().item,
                &Shape(i)
            );
        }
    }

    #[test]
    fn iterate() {
        let mut graph = Graph::new();
        let mut poses: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph).pin(&mut graph);

        for i in 0..10 {
            let pose_node = poses.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(&rb_node, &pose_node);
            if i % 2 == 0 {
                graph.connect(&pose_node, &vel_node);
            }
            if i % 4 == 0 {
                graph.connect_oneway(&rb_node, &everyones_shape);
            }
        }

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for mut rb in rbs.iter_mut(&graph) {
            let pose = match graph.get_neighbor(&rb, &poses) {
                Some(pose) => pose,
                None => continue,
            };
            let vel = match graph.get_neighbor(&pose, &vels) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;
            rb.item.0 = 42;

            let mut shape = graph.get_neighbor_mut(&rb, &mut shapes);
            if let Some(shape) = &mut shape {
                full_match_count += 1;
                shape.item.0 += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.item.0 % 2, 0);

            println!(
                "{:?}, {:?}, {:?}, {:?}",
                rb.item,
                pose.item,
                vel.item,
                shape.map(|s| s.item)
            );
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);
        assert_eq!(shapes.get_unchecked(everyones_shape.pos()).item, &Shape(72));

        println!("All rbs: {:?}", rbs.content);

        for rb in rbs.iter(&graph) {
            if graph
                .get_neighbor(&rb, &poses)
                .and_then(|pose| graph.get_neighbor(&pose, &vels))
                .is_none()
            {
                assert_eq!(rb.item.0, 0);
            } else {
                assert_eq!(rb.item.0, 42);
            }
        }
    }

    #[test]
    fn delete() {
        let mut graph = Graph::new();
        let mut poses: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();
        let mut sub_shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph);
        let shape_owns_thing = sub_shapes.insert(Shape(42), &mut graph);
        graph.connect(&everyones_shape, &shape_owns_thing);

        for i in 0..10 {
            let pose_node = poses.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(&rb_node, &pose_node);
            if i % 2 == 0 {
                graph.connect(&pose_node, &vel_node);
            } else {
                graph.connect_oneway(&vel_node, &vel_node); // connect vel to itself to "keep it alive"
            }
            if i % 3 == 0 {
                graph.connect_oneway(&rb_node, &everyones_shape);
            }
        }

        println!("Refcounts after creations {:?}", graph.refcounts);

        assert_eq!(vels.iter(&graph).count(), 10);
        for vel_to_del in vels.iter(&graph).map(|v| v.pos()).collect::<Vec<_>>() {
            println!("delete");
            graph.delete(vels.get_unchecked(vel_to_del));
            println!("refcounts {:?}", graph.refcounts);
        }
        // all vels deleted (== have 0 referrers)
        assert_eq!(vels.iter(&graph).count(), 0);
        // half of poses had vels attached and have also been deleted
        assert_eq!(poses.iter(&graph).count(), 5);

        println!("Refcounts after first deletions {:?}", graph.refcounts);

        // rbs are connected to everything so deleting all of them should delete everything
        for rb_to_del in rbs.iter(&graph).map(|rb| rb.pos()).collect::<Vec<_>>() {
            // everyones_shape and its subcomponent should live until the last rb is deleted
            // BECAUSE the last rb is connected to it
            // (remember this if changing the iteration counts!)
            assert_eq!(shapes.iter(&graph).count(), 1);
            assert_eq!(sub_shapes.iter(&graph).count(), 1);

            graph.delete(rbs.get_unchecked(rb_to_del));
        }
        assert_eq!(shapes.iter(&graph).count(), 0);
        assert_eq!(poses.iter(&graph).count(), 0);
        assert_eq!(vels.iter(&graph).count(), 0);
        assert_eq!(rbs.iter(&graph).count(), 0);

        // check that edges were all cleared too
        for (layer_idx, edge) in
            graph
                .edge_layers
                .iter()
                .enumerate()
                .flat_map(|(layer_idx, from_l)| {
                    from_l
                        .iter()
                        .flat_map(move |to_l| to_l.iter().map(move |e| (layer_idx, e)))
                })
        {
            assert!(edge.is_none(), "Layer {} had a non-empty edge", layer_idx);
        }
    }

    #[test]
    fn reuse_deleted_slots() {
        let mut graph = Graph::new();
        let mut poses: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph).pin(&mut graph);

        // delete and respawn stuff a few times to hopefully see if things get connected wrong at some point
        // (this doesn't prove things won't go wrong over a long enough time but fingers crossed)
        for i in 0..5 {
            for pose_node in poses
                .iter(&graph)
                // delete half of everything that was placed,
                // so every new iteration we should be reusing slots for half and pushing new for half
                .take(5)
                .map(|pose| pose.pos())
                .collect::<Vec<_>>()
            {
                graph.delete(poses.get_unchecked(pose_node));
            }
            for j in 0..10 {
                let id = i * 20 + j;
                let pose_node = poses.insert(Transform(id), &mut graph);
                let vel_node = vels.insert(Velocity(id), &mut graph);
                let rb_node = rbs.insert(RigidBody(id), &mut graph);
                graph.connect(&pose_node, &vel_node);
                graph.connect(&vel_node, &rb_node);
                graph.connect_oneway(&pose_node, &everyones_shape);
                // delete and replace every other
                if i % 2 == 0 {
                    let pose_node = NodeRef::as_node(&pose_node, &graph); // drop the ref to the layer

                    let pose_len_before = poses.content.len();

                    graph.delete(
                        poses.get(
                            pose_node
                                .check(&graph)
                                .expect("pose_node wasn't deleted yet so it should be there"),
                        ),
                    );
                    assert!(
                        pose_node.check(&graph).is_none(),
                        "pose_node was deleted so checking it should now give None"
                    );

                    let pose_node = poses.insert(Transform(100), &mut graph);
                    let vel_node = vels.insert(Velocity(100), &mut graph);
                    graph.connect(&pose_node, &vel_node);
                    if i % 4 == 0 {
                        let rb_node = rbs.insert(RigidBody(100), &mut graph);
                        graph.connect(&rb_node, &pose_node);
                    }

                    let pose_len_after = poses.content.len();
                    // reused the slot that was left behind by delete
                    assert_eq!(pose_len_before, pose_len_after);
                }
            }
        }

        println!("poses.content: {:?}", poses.content);
        println!("vels.content: {:?}", vels.content);
        println!("rbs.content: {:?}", rbs.content);

        assert_eq!(poses.content.len(), 30);
        assert_eq!(vels.content.len(), 30);
        // everyones_shape was never deleted
        assert_eq!(shapes.content.len(), 1);
    }

    #[test]
    fn pin() {
        let mut graph = Graph::new();
        let mut poses: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();

        let pose = poses.insert(Transform(0), &mut graph);
        let pose_pin = pose.pin(&mut graph);
        let pose = NodeRef::as_node(&pose, &graph); // just to be able to delete and still have the pinned node
                                                    // should appear in the iterator even though it's not connected to anything
        assert_eq!(poses.iter(&graph).count(), 1);

        // awkward way to drop the reference to `vels` but this is unlikely to be needed in an actual spawning function
        let vel = NodeRef::as_node(&vels.insert(Velocity(0), &mut graph), &graph);
        let vel_check = vel.check(&graph).unwrap();
        let rb = NodeRef::as_node(&rbs.insert(RigidBody(0), &mut graph), &graph);
        let rb_check = rb.check(&graph).unwrap();

        graph.connect(&pose_pin, &vel_check);
        graph.connect_oneway(&pose_pin, &rb_check);
        graph.connect_oneway(&vel_check, &rb_check);

        // this should not delete anything because the root node of deletion is pinned
        graph.delete(pose.check(&graph).unwrap());
        assert!(graph.get_neighbor(&pose_pin, &vels).is_some());
        assert!(graph.get_neighbor(&pose_pin, &rbs).is_some());
        assert!(graph.get_neighbor(&vel_check, &rbs).is_some());

        // unpinning and then deleting should delete everything
        graph.unpin(pose_pin);
        graph.delete(pose.check(&graph).unwrap());
        assert!(pose.check(&graph).is_none());
        assert!(vel.check(&graph).is_none());
        assert!(rb.check(&graph).is_none());
    }
}
