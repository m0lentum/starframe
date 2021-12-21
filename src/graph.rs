//! Starframe's entity system, i.e. the data structure representing game objects.
//!
//! A game object in Starframe is a _directed graph_ of components.
//! Conceptually, one may look something like this:
//! ```text
//!   Pose <----> Body <----> Collider
//!    ^            ^
//!    |            |
//!    v            v
//!  Sprite     EventSink
//!    |
//!    |
//!    v
//! Texture
//! ```
//! Most edges (connections between nodes) should go both ways, but because the graph is directed,
//! one-directional edges such as from Sprite to Texture in the above diagram can also be made.
//! This is important to the way object boundaries are determined by the deletion algorithm (see
//! [`delete`][self::Graph::delete] for details). This comes into play when sharing one component
//! between multiple objects.
//!
//! Similarly to how systems in ECS iterate over specific sets of components,
//! systems using the graph iterate over specific *patterns* of connected nodes.
//!
//! # Usage example
//! ```
//! # use starframe::{graph::{make_graph, LayerViewMut}, math::Pose};
//!
//! struct Player;
//! enum Hat {
//!     Fedora,
//!     PropellerHat,
//! }
//! struct Sword {
//!     coolness_level: usize,
//! }
//!
//! type MyGraph = make_graph! {
//!     Player,
//!     Hat,
//!     Sword,
//! };
//! let graph = MyGraph::new();
//!
//! fn spawn_player(
//!     pose: Pose,
//!     hat: Hat,
//!     sword: Sword,
//!     (mut l_pose, mut l_player, mut l_hat, mut l_sword): (
//!         LayerViewMut<Pose>,
//!         LayerViewMut<Player>,
//!         LayerViewMut<Hat>,
//!         LayerViewMut<Sword>,
//!     )
//! ) {
//!     let mut pose_node = l_pose.insert(pose);
//!     let mut player_node = l_player.insert(Player);
//!     let mut hat_node = l_hat.insert(hat);
//!     let mut sword_node = l_sword.insert(sword);
//!
//!     player_node.connect(&mut pose_node);
//!     player_node.connect(&mut hat_node);
//!     player_node.connect(&mut sword_node);
//! }
//!
//! spawn_player(
//!     Pose::default(),
//!     Hat::PropellerHat,
//!     Sword { coolness_level: 9001 },
//!     graph.get_layer_bundle(),
//! );
//! spawn_player(
//!     Pose::default(),
//!     Hat::Fedora,
//!     Sword { coolness_level: 1 },
//!     graph.get_layer_bundle(),
//! );
//!
//! let l_player = graph.get_layer::<Player>();
//! let l_sword = graph.get_layer::<Sword>();
//! for player_node in l_player.iter() {
//!     if let Some(sword) = player_node.get_neighbor(&l_sword) {
//!         println!("Watch out, this guy's got a lvl {} sword", sword.c.coolness_level);
//!     }
//! }
//! ```
//!
//! See individual types' documentation for details.

use std::{any::Any, collections::VecDeque, marker::PhantomData};

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

//

#[macro_use]
mod component_defs;
pub use crate::make_graph;
#[doc(hidden)]
pub use component_defs::BUILTIN_LAYER_COUNT;

mod layer_bundle;
pub use layer_bundle::LayerBundle;

//
// Index & ref types
//

type ComponentIdx = usize;
type GenerationIdx = usize;

/// Allows types to be inserted into the graph.
/// Implemented for custom types using the [`make_graph`][self::make_graph] macro;
/// do not implement manually!
pub trait Component: 'static + Send + Sync {
    /// Address of this type's graph layer.
    const LAYER_INDEX: usize;
}

/// Node position without generation info, used internally to traverse the graph
/// without knowing types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BareNodeKey {
    layer: usize,
    idx: usize,
}
impl<T: Component> From<NodeKey<T>> for BareNodeKey {
    fn from(key: NodeKey<T>) -> Self {
        Self {
            layer: T::LAYER_INDEX,
            idx: key.idx,
        }
    }
}

/// An identifier for looking up a specific node.
pub struct NodeKey<T: Component> {
    pub(crate) idx: usize,
    pub(crate) gen: usize,
    pub(crate) _marker: PhantomData<T>,
}
impl<'a, T: Component> From<NodeRef<'a, T>> for NodeKey<T> {
    fn from(node: NodeRef<'a, T>) -> Self {
        Self {
            idx: node.idx,
            gen: node.layer_meta.statuses[node.idx].generation,
            _marker: PhantomData,
        }
    }
}
impl<'a, T: Component> From<NodeRefMut<'a, T>> for NodeKey<T> {
    fn from(node: NodeRefMut<'a, T>) -> Self {
        Self {
            idx: node.idx,
            gen: node.layer_meta.statuses[node.idx].generation,
            _marker: PhantomData,
        }
    }
}
// blanket impls required because phantomdata makes derive unnecessarily restrict type of T
impl<T: Component> Clone for NodeKey<T> {
    fn clone(&self) -> Self {
        NodeKey {
            idx: self.idx,
            gen: self.idx,
            _marker: PhantomData,
        }
    }
}
impl<T: Component> Copy for NodeKey<T> {}
impl<T: Component> std::fmt::Debug for NodeKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Node {{\n idx: {:?},\n gen: {},\n}}",
            self.idx, self.gen
        ))
    }
}
impl<T: Component> PartialEq for NodeKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.gen == other.gen
    }
}
impl<T: Component> Eq for NodeKey<T> {}
impl<T: Component> std::hash::Hash for NodeKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
    }
}

/// An immutable reference to a node in the graph.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a, T: Component> {
    /// The component that this node points to.
    pub c: &'a T,
    idx: usize,
    layer_meta: &'a LayerMetadata,
}

impl<'a, T: Component> NodeRef<'a, T> {
    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr LayerView<'l, Target>,
    ) -> Option<NodeRef<'lr, Target>> {
        get_neighbor(self.layer_meta, self.idx, layer)
    }

    /// Get an iterator going over all edges from this node to the target layer.
    /// Usually there is only one and [`get_neighbor`][self::get_neighbor] can be used instead.
    #[inline]
    pub fn get_all_neighbors<'r, 'l, Target: Component>(
        &'r self,
        layer: &'r LayerView<'l, Target>,
    ) -> EdgeIter<'r, 'l, Target> {
        EdgeIter {
            curr_list_node: get_neighbor_list_node::<Target>(self.layer_meta, self.idx),
            edge_list: &self.layer_meta.secondary_edges,
            layer,
        }
    }

    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a mutable reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor_mut<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr mut LayerViewMut<'l, Target>,
    ) -> Option<NodeRefMut<'lr, Target>> {
        get_neighbor_mut(self.layer_meta, self.idx, layer)
    }

    /// Get a mutable iterator going over all edges from this node to the target layer.
    /// Usually there is only one and [`get_neighbor`][self::get_neighbor_mut] can be used instead.
    #[inline]
    pub fn get_all_neighbors_mut<'r, 'l, Target: Component>(
        &'r self,
        layer: &'r mut LayerViewMut<'l, Target>,
    ) -> EdgeIterMut<'r, 'l, Target> {
        EdgeIterMut {
            curr_list_node: get_neighbor_list_node::<Target>(self.layer_meta, self.idx),
            edge_list: &self.layer_meta.secondary_edges,
            layer,
        }
    }

    /// Get a key that can be used to access this node later.
    #[inline]
    pub fn key(&self) -> NodeKey<T> {
        NodeKey {
            idx: self.idx,
            gen: self.layer_meta.statuses[self.idx].generation,
            _marker: PhantomData,
        }
    }
}

/// A mutable reference to a node in the graph.
pub struct NodeRefMut<'a, T: Component> {
    /// The component that this node points to.
    pub c: &'a mut T,
    idx: usize,
    layer_meta: &'a mut LayerMetadata,
}

impl<'a, T: Component> NodeRefMut<'a, T> {
    #[inline]
    pub fn connect<Other: Component>(&mut self, other: &mut NodeRefMut<'_, Other>) {
        self.connect_oneway(other);
        other.connect_oneway(self);
    }

    fn connect_oneway<Other: Component>(&mut self, other: &mut NodeRefMut<'_, Other>) {
        let edges = &mut self.layer_meta.edges[Other::LAYER_INDEX];
        if edges.len() <= self.idx {
            edges.resize(self.idx + 1, None);
        }
        let new_edge_node = EdgeListNode {
            target: other.idx,
            next_edge: None,
        };
        // add the node either as primary if it's the first one,
        // or to the end of the linked list if not
        match edges[self.idx] {
            None => {
                edges[self.idx] = Some(new_edge_node);
            }
            Some(ref mut existing_edge) => {
                let new_edge_idx = match self.layer_meta.vacant_edge_slots.pop_front() {
                    Some(vacant) => {
                        self.layer_meta.secondary_edges[vacant] = new_edge_node;
                        vacant
                    }
                    None => {
                        self.layer_meta.secondary_edges.push(new_edge_node);
                        self.layer_meta.secondary_edges.len() - 1
                    }
                };
                // attach to the list
                match existing_edge.next_edge {
                    None => {
                        existing_edge.next_edge = Some(new_edge_idx);
                    }
                    Some(next_edge) => {
                        let mut last_edge_idx = next_edge;
                        while let Some(next) =
                            self.layer_meta.secondary_edges[last_edge_idx].next_edge
                        {
                            last_edge_idx = next;
                        }
                        self.layer_meta.secondary_edges[last_edge_idx].next_edge =
                            Some(new_edge_idx);
                    }
                }
            }
        }
    }

    /// If there's an edge from this node to a node of the given type,
    /// get a reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr LayerView<'l, Target>,
    ) -> Option<NodeRef<'lr, Target>> {
        get_neighbor(self.layer_meta, self.idx, layer)
    }

    /// Get an iterator going over all edges from this node to the target layer.
    /// Usually there is only one and [`get_neighbor`][self::get_neighbor] can be used instead.
    #[inline]
    pub fn get_all_neighbors<'r, 'l, Target: Component>(
        &'r self,
        layer: &'r LayerView<'l, Target>,
    ) -> EdgeIter<'r, 'l, Target> {
        EdgeIter {
            curr_list_node: get_neighbor_list_node::<Target>(self.layer_meta, self.idx),
            edge_list: &self.layer_meta.secondary_edges,
            layer,
        }
    }

    /// If there's an edge from this node to a node of the given type,
    /// get a mutable reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor_mut<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr mut LayerViewMut<'l, Target>,
    ) -> Option<NodeRefMut<'lr, Target>> {
        get_neighbor_mut(self.layer_meta, self.idx, layer)
    }

    /// Get a mutable iterator going over all edges from this node to the target layer.
    /// Usually there is only one and [`get_neighbor`][self::get_neighbor_mut] can be used instead.
    #[inline]
    pub fn get_all_neighbors_mut<'r, 'l, Target: Component>(
        &'r self,
        layer: &'r mut LayerViewMut<'l, Target>,
    ) -> EdgeIterMut<'r, 'l, Target> {
        EdgeIterMut {
            curr_list_node: get_neighbor_list_node::<Target>(self.layer_meta, self.idx),
            edge_list: &self.layer_meta.secondary_edges,
            layer,
        }
    }

    /// Get a key that can be used to access this node later.
    #[inline]
    pub fn key(&self) -> NodeKey<T> {
        NodeKey {
            idx: self.idx,
            gen: self.layer_meta.statuses[self.idx].generation,
            _marker: PhantomData,
        }
    }

    /// Get an immutable `NodeRef` from the `NodeRefMut`.
    #[inline]
    pub fn subview(&self) -> NodeRef<'_, T> {
        NodeRef {
            c: self.c,
            idx: self.idx,
            layer_meta: self.layer_meta,
        }
    }
}

#[inline]
fn get_neighbor_list_node<Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
) -> Option<EdgeListNode> {
    if node_layer_meta.edges.len() <= Target::LAYER_INDEX
        || node_layer_meta.edges[Target::LAYER_INDEX].len() <= node_idx
    {
        None
    } else {
        node_layer_meta.edges[Target::LAYER_INDEX][node_idx]
    }
}

#[inline]
pub(crate) fn get_neighbor_idx<Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
) -> Option<usize> {
    get_neighbor_list_node::<Target>(node_layer_meta, node_idx).map(|e| e.target)
}

#[inline]
fn get_neighbor<'lr, 'l, Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
    target_layer: &'lr LayerView<'l, Target>,
) -> Option<NodeRef<'lr, Target>> {
    get_neighbor_idx::<Target>(node_layer_meta, node_idx).map(|target| NodeRef {
        c: &target_layer.components[target],
        idx: target,
        layer_meta: target_layer.meta,
    })
}

#[inline]
fn get_neighbor_mut<'lr, 'l, Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
    target_layer: &'lr mut LayerViewMut<'l, Target>,
) -> Option<NodeRefMut<'lr, Target>> {
    get_neighbor_idx::<Target>(node_layer_meta, node_idx).map(move |target| NodeRefMut {
        c: &mut target_layer.components[target],
        idx: target,
        layer_meta: target_layer.meta,
    })
}

//
// Layer metadata
//

/// Tracking edges, refcounts, generations and vacant slots for a single layer.
#[derive(Debug)]
pub(crate) struct LayerMetadata {
    edges: Vec<Vec<Option<EdgeListNode>>>,
    secondary_edges: Vec<EdgeListNode>,
    vacant_edge_slots: VecDeque<usize>,

    statuses: Vec<NodeStatus>,
    vacant_comp_slots: VecDeque<ComponentIdx>,
}
impl LayerMetadata {
    fn new(layer_count: usize) -> Self {
        Self {
            edges: vec![Vec::new(); layer_count],
            secondary_edges: Vec::new(),
            vacant_edge_slots: VecDeque::new(),

            statuses: Vec::new(),
            vacant_comp_slots: VecDeque::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct NodeStatus {
    generation: GenerationIdx,
    // needed for skipping in iteration
    currently_exists: bool,
}

/// Intrusive list node for secondary edges
#[derive(Clone, Copy, Debug)]
struct EdgeListNode {
    target: ComponentIdx,
    next_edge: Option<usize>,
}

/// An iterator over all edges from a node to nodes in a specific layer.
pub struct EdgeIter<'a, 'l: 'a, T: Component> {
    curr_list_node: Option<EdgeListNode>,
    edge_list: &'a [EdgeListNode],
    layer: &'a LayerView<'l, T>,
}
impl<'a, 'l: 'a, T: Component> Iterator for EdgeIter<'a, 'l, T> {
    type Item = NodeRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.curr_list_node {
            None => None,
            Some(curr) => {
                self.curr_list_node = curr.next_edge.map(|next| self.edge_list[next]);
                Some(self.layer.get_unchecked_by_item_idx(curr.target))
            }
        }
    }
}

/// An iterator over all edges from a node to nodes in a specific layer,
/// yielding mutable references.
pub struct EdgeIterMut<'a, 'l: 'a, T: Component> {
    curr_list_node: Option<EdgeListNode>,
    edge_list: &'a [EdgeListNode],
    layer: &'a mut LayerViewMut<'l, T>,
}
impl<'a, 'l: 'a, T: Component> EdgeIterMut<'a, 'l, T> {
    /// Get a reference to the next connected node.
    ///
    /// Unfortunately this isn't compatible with the Iterator trait
    /// because the lifetime of yielded references needs to be `'_`,
    /// which doesn't exist in associated types.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<NodeRefMut<'_, T>> {
        match self.curr_list_node {
            None => None,
            Some(curr) => {
                self.curr_list_node = curr.next_edge.map(|next| self.edge_list[next]);
                Some(self.layer.get_mut_unchecked_by_item_idx(curr.target))
            }
        }
    }
}

/// Storage type allowing us to store all layers in a single Vec
/// and access their metadata without having to know their type.
#[derive(Debug)]
struct TypeErasedLayer {
    meta: LayerMetadata,
    components: ComponentStorage,
}
impl TypeErasedLayer {
    fn new(layer_count: usize) -> Self {
        Self {
            meta: LayerMetadata::new(layer_count),
            components: ComponentStorage(None),
        }
    }
}

//
// Layer storage
//

/// Dynamically typed, lazily initialized storage for component buffers.
#[derive(Debug)]
struct ComponentStorage(Option<Box<dyn Any>>);

impl ComponentStorage {
    fn downcast<T: 'static>(&self) -> &[T] {
        match self.0 {
            Some(ref already_inited) => (*already_inited)
                .downcast_ref::<Vec<T>>()
                .unwrap()
                .as_slice(),
            None => &[],
        }
    }

    fn downcast_mut<T: Sized + 'static>(&mut self) -> &mut Vec<T> {
        if let Some(ref mut already_inited) = self.0 {
            return already_inited.downcast_mut::<Vec<T>>().unwrap();
        }
        self.0 = Some(Box::new(Vec::<T>::new()));
        match self.0 {
            Some(ref mut arst) => arst.downcast_mut().unwrap(),
            None => unreachable!(),
        }
    }
}

/// An immutable view into a single layer of the graph.
///
/// Acquired with [`Graph::get_layer`][self::Graph::get_layer_mut] or as a part of
/// [`Graph::get_layer_bundle`][self::Graph::get_layer_bundle].
pub struct LayerView<'a, T: Component> {
    pub(crate) meta: &'a LayerMetadata,
    pub(crate) components: &'a [T],
    // Using the same unsafe pattern as with `LayerViewMut`,
    // even though it's not _strictly_ necessary here.
    // The reason why (and why it's in an Option) is so that we can implement
    // borrowing a LayerView from a LayerViewMut so we're not restricted to one or the other
    // in function parameters, and can forward views to nested functions if needed.
    _guard: Option<RwLockReadGuard<'a, TypeErasedLayer>>,
}

impl<'a, T: Component> LayerView<'a, T> {
    /// Get an immutable reference to a node if it still exists, otherwise return None.
    #[inline]
    pub fn get(&self, key: NodeKey<T>) -> Option<NodeRef<'_, T>> {
        if self.meta.statuses.len() <= key.idx || self.meta.statuses[key.idx].generation != key.gen
        {
            None
        } else {
            Some(self.get_unchecked(key))
        }
    }

    /// Get an immutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_unchecked(&self, key: NodeKey<T>) -> NodeRef<'_, T> {
        self.get_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_unchecked_by_item_idx(&self, idx: usize) -> NodeRef<'_, T> {
        NodeRef {
            c: &self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            layer_meta: &*self.meta,
            comp_iter: self.components.iter().enumerate(),
        }
    }

    /// Take a sub-view into this view.
    ///
    /// Useful for forwarding the view to other functions without moving it.
    pub fn subview(&self) -> LayerView<'_, T> {
        LayerView {
            meta: self.meta,
            components: self.components,
            _guard: None,
        }
    }
}

/// A mutable view into a single layer of the graph.
///
/// Acquired with [`Graph::get_layer_mut`][self::Graph::get_layer_mut] or as a part of
/// [`Graph::get_layer_bundle`][self::Graph::get_layer_bundle].
pub struct LayerViewMut<'a, T: Component> {
    pub(crate) meta: &'a mut LayerMetadata,
    pub(crate) components: &'a mut Vec<T>,
    // Storing the lock guard inside this
    // because I can't figure out a way to map it cleanly to a view like this.
    // This requires unsafe and is ugly :(
    // SAFETY: never access the guard, only use above fields.
    // Because it's in the same struct as the references to its inside, all drop at the same time
    // (in an option to allow subviews)
    _guard: Option<RwLockWriteGuard<'a, TypeErasedLayer>>,
}

impl<'a, T: Component> LayerViewMut<'a, T> {
    /// Insert a component into the layer.
    ///
    /// This returns a reference to the node that was created,
    /// which you can use to connect it to other nodes.
    /// # Example
    /// ```
    /// # use starframe::{graph::make_graph, math::Pose, physics::Collider};
    /// # type MyGraph = make_graph!{};
    /// # let graph = MyGraph::new();
    /// let mut l_pose = graph.get_layer_mut::<Pose>();
    /// let mut l_collider = graph.get_layer_mut::<Collider>();
    /// let mut pose_node = l_pose.insert(Pose::default());
    /// let mut collider_node = l_collider.insert(Collider::new_circle(1.0));
    /// pose_node.connect(&mut collider_node);
    /// ```
    pub fn insert(&mut self, component: T) -> NodeRefMut<'_, T> {
        let item_idx = if let Some(vacant_slot) = self.meta.vacant_comp_slots.pop_front() {
            // no generation increment here, that happens on delete
            self.components[vacant_slot] = component;
            self.meta.statuses[vacant_slot].currently_exists = true;
            vacant_slot
        } else {
            self.components.push(component);
            self.meta.statuses.push(NodeStatus {
                generation: 0,
                currently_exists: true,
            });
            self.components.len() - 1
        };

        NodeRefMut {
            c: &mut self.components[item_idx],
            idx: item_idx,
            layer_meta: &mut self.meta,
        }
    }

    /// Get an immutable reference to a node if it still exists, otherwise return None.
    pub fn get(&self, key: NodeKey<T>) -> Option<NodeRef<'_, T>> {
        if self.meta.statuses[key.idx].generation != key.gen {
            None
        } else {
            Some(self.get_unchecked(key))
        }
    }

    /// Get an immutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_unchecked(&self, key: NodeKey<T>) -> NodeRef<'_, T> {
        self.get_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_unchecked_by_item_idx(&self, idx: usize) -> NodeRef<'_, T> {
        NodeRef {
            c: &self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    /// Get a mutable reference to a node if it still exists, otherwise return None.
    pub fn get_mut(&mut self, key: NodeKey<T>) -> Option<NodeRefMut<'_, T>> {
        if self.meta.statuses.len() <= key.idx || self.meta.statuses[key.idx].generation != key.gen
        {
            None
        } else {
            Some(self.get_mut_unchecked(key))
        }
    }

    /// Get a mutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_mut_unchecked(&mut self, key: NodeKey<T>) -> NodeRefMut<'_, T> {
        self.get_mut_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_mut_unchecked_by_item_idx(&mut self, idx: usize) -> NodeRefMut<'_, T> {
        NodeRefMut {
            c: &mut self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            layer_meta: self.meta,
            comp_iter: self.components.iter().enumerate(),
        }
    }

    pub fn iter_mut(&mut self) -> LayerIterMut<'_, T> {
        LayerIterMut {
            layer_meta: self.meta,
            comp_iter: self.components.iter_mut().enumerate(),
        }
    }

    /// Take an immutable sub-view into this mutable view.
    pub fn subview(&self) -> LayerView<'_, T> {
        LayerView {
            meta: self.meta,
            components: self.components,
            _guard: None,
        }
    }

    /// Take a mutable sub-view into this mutable view.
    ///
    /// Useful for forwarding the view to other functions without moving it.
    pub fn subview_mut(&mut self) -> LayerViewMut<'_, T> {
        LayerViewMut {
            meta: self.meta,
            components: self.components,
            _guard: None,
        }
    }
}

//
// Iterators
//

/// An immutable iterator over components in a single layer.
///
/// Create with the `iter` method on [`LayerView`][self::LayerView]
/// or [`LayerViewMut`][self::LayerViewMut].
pub struct LayerIter<'a, T: Component> {
    layer_meta: &'a LayerMetadata,
    comp_iter: std::iter::Enumerate<std::slice::Iter<'a, T>>,
}

impl<'a, T: Component> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let (next_idx, next) = loop {
            let (next_idx, next) = self.comp_iter.next()?;
            if self.layer_meta.statuses[next_idx].currently_exists {
                break (next_idx, next);
            }
        };
        Some(NodeRef {
            c: next,
            idx: next_idx,
            layer_meta: self.layer_meta,
        })
    }
}

/// A mutable iterator over components in a single layer.
///
/// Create with the `iter_mut` method on [`LayerViewMut`][self::LayerViewMut].
pub struct LayerIterMut<'a, T: Component> {
    layer_meta: &'a mut LayerMetadata,
    comp_iter: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
}

impl<'a, T: Component> Iterator for LayerIterMut<'a, T> {
    type Item = NodeRefMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let (next_idx, next) = loop {
            let (next_idx, next) = self.comp_iter.next()?;
            if self.layer_meta.statuses[next_idx].currently_exists {
                break (next_idx, next);
            }
        };
        let layer_meta: *mut LayerMetadata = self.layer_meta;
        Some(NodeRefMut {
            c: next,
            idx: next_idx,
            // SAFETY: aliasing layer_meta is ok here because we only ever use it
            // to access node-specific metadata.
            // If we could only store references to node-specific metadata in the NodeRef
            // and use slice iterators to avoid the unsafe here that would be neat,
            // but due to how edges are indexed this isn't feasible.
            layer_meta: unsafe { &mut *layer_meta },
        })
    }
}

//
// Graph
//

/// The component graph itself.
///
/// Use the [`make_graph`][self::make_graph] macro to automatically create one
/// with the correct layer count.
/// See that macro's documentation for an example and some talk of limitations.
///
/// A graph is built out of _layers_, one per type of component stored.
/// Layers contain _nodes_ representing individual components,
/// and these are connected to other components with _edges_.
#[derive(Debug)]
pub struct Graph<const LAYER_COUNT: usize> {
    layers: Vec<RwLock<TypeErasedLayer>>,
}

impl<const LAYER_COUNT: usize> Default for Graph<LAYER_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const LAYER_COUNT: usize> Graph<LAYER_COUNT> {
    pub fn new() -> Self {
        let mut layers = Vec::new();
        layers.resize_with(LAYER_COUNT, || {
            RwLock::new(TypeErasedLayer::new(LAYER_COUNT))
        });
        Self { layers }
    }

    /// Lock a layer for reading.
    /// # Panics
    /// Panics if the layer is currently locked for writing.
    pub fn get_layer<T: Component>(&self) -> LayerView<'_, T> {
        let err = || {
            // not sure if panic here is the right call,
            // but it's surely better than having it hang forever in case of a conflict
            panic!(
                "Could not lock layer for reading: {}",
                std::any::type_name::<T>()
            )
        };
        let guard = self.layers[T::LAYER_INDEX].try_read().unwrap_or_else(err);
        // taking references to things inside the lock for the sake of API.
        // SAFETY: the guard will drop at the same time as the references
        // and we never access the guard itself.
        unsafe {
            let meta: *const LayerMetadata = &guard.meta;
            let components: *const ComponentStorage = &guard.components;
            LayerView {
                meta: &*meta,
                components: (&*components).downcast(),
                _guard: Some(guard),
            }
        }
    }

    /// Lock a layer for writing.
    /// # Panics
    /// Panics if the layer is currently locked for reading or writing.
    pub fn get_layer_mut<T: Component>(&self) -> LayerViewMut<'_, T> {
        let err = || {
            panic!(
                "Could not lock layer for writing: {}",
                std::any::type_name::<T>()
            )
        };
        let mut guard = self.layers[T::LAYER_INDEX].try_write().unwrap_or_else(err);
        // taking references to things inside the lock for the sake of API.
        // SAFETY: the guard will drop at the same time as the references
        // and we never access the guard itself.
        unsafe {
            let meta: *mut LayerMetadata = &mut guard.meta;
            let components: *mut ComponentStorage = &mut guard.components;
            LayerViewMut {
                meta: &mut *meta,
                components: (&mut *components).downcast_mut(),
                _guard: Some(guard),
            }
        }
    }

    /// Get a tuple of layer views in one call.
    /// This is a common pattern in arguments for functions that manipulate the graph.
    /// # Example
    /// ```
    /// # use starframe::{
    /// #    math::Pose,
    /// #    physics::{Body, Collider},
    /// #    graph::{LayerView, LayerViewMut, make_graph}
    /// # };
    /// # type MyGraph = make_graph!{};
    /// # let graph = MyGraph::new();
    ///
    /// fn do_things_with_bodies(
    ///     how_many_things: usize,
    ///     for_how_long: usize,
    ///     (l_pose, l_body, l_collider): (
    ///         LayerViewMut<Pose>,
    ///         LayerViewMut<Body>,
    ///         LayerView<Collider>,
    ///     ),
    /// ) {
    ///     // do stuff...
    /// }
    ///
    /// do_things_with_bodies(42, 69, graph.get_layer_bundle());
    /// ```
    pub fn get_layer_bundle<'a, B: LayerBundle<'a>>(&'a self) -> B {
        B::get_from_graph(self)
    }

    fn write_all_layers(&mut self) -> Vec<RwLockWriteGuard<'_, TypeErasedLayer>> {
        self.layers
            .iter()
            .map(|lock| {
                lock.try_write()
                    .expect("One or more layers were in use when trying to delete")
            })
            .collect()
    }

    /// Drop all content and recreate the graph from scratch.
    pub fn reset(&mut self) {
        let layer_count = self.layers.len();
        for mut layer in self.write_all_layers() {
            *layer = TypeErasedLayer::new(layer_count);
        }
    }

    /// Create a query that finds all nodes accessible from the given node via edges.
    /// This is used to delete segments of the graph.
    pub fn gather<T: Component>(&mut self, node: NodeKey<T>) -> Gather<'_, LAYER_COUNT> {
        Gather {
            graph: self,
            inner: GatherInner {
                root: node.into(),
                root_gen: node.gen,
                stop_at_layers: Vec::new(),
            },
        }
    }
}

//
// Gather & delete
//

pub struct Gather<'g, const LAYER_COUNT: usize> {
    graph: &'g mut Graph<LAYER_COUNT>,
    inner: GatherInner,
}

#[derive(Clone, Debug)]
struct GatherInner {
    root: BareNodeKey,
    root_gen: GenerationIdx,
    stop_at_layers: Vec<usize>,
}

#[derive(Clone, Debug)]
struct GatherResult {
    nodes: Vec<BareNodeKey>,
    edges: Vec<(BareNodeKey, BareNodeKey)>,
}

impl<'g, const LAYER_COUNT: usize> Gather<'g, LAYER_COUNT> {
    pub fn stop_at_layer<T: Component>(mut self) -> Self {
        self.inner.stop_at_layers.push(T::LAYER_INDEX);
        self
    }

    pub fn delete(mut self) {
        let mut locked_layers = self.graph.write_all_layers();

        let root_idx = self.inner.root.idx;
        // check if the node still exists before doing anything
        let start_layer = &locked_layers[self.inner.root.layer].meta;
        if start_layer.statuses.len() <= root_idx
            || start_layer.statuses[root_idx].generation != self.inner.root_gen
            || !start_layer.statuses[root_idx].currently_exists
        {
            return;
        }

        let result = Self::run(&mut self.inner, &mut locked_layers);

        for (edge_start, edge_end) in result.edges {
            let layer_meta = &mut locked_layers[edge_start.layer].meta;

            if !self.inner.stop_at_layers.contains(&edge_start.layer) {
                //
                // this is a node that's being deleted; remove the entire list of edges

                let mut curr_edge = layer_meta.edges[edge_end.layer][edge_start.idx].unwrap();
                layer_meta.edges[edge_end.layer][edge_start.idx] = None;
                while let Some(next) = curr_edge.next_edge {
                    curr_edge = layer_meta.secondary_edges[next];
                    // we don't explicitly delete anything here,
                    // just remove the primary edge and mark the secondary slots for reuse
                    layer_meta.vacant_edge_slots.push_back(next);
                }
            } else {
                //
                // this is a boundary node; find and remove only this particular edge in the list

                let primary_edge = layer_meta.edges[edge_end.layer][edge_start.idx]
                    .as_mut()
                    .unwrap();
                if primary_edge.target == edge_end.idx {
                    match primary_edge.next_edge {
                        None => {
                            layer_meta.edges[edge_end.layer][edge_start.idx] = None;
                        }
                        Some(next) => {
                            *primary_edge = layer_meta.secondary_edges[next];
                            layer_meta.vacant_edge_slots.push_back(next);
                        }
                    }
                } else {
                    let first_secondary_idx = primary_edge.next_edge.unwrap();
                    let first_secondary = layer_meta.secondary_edges[first_secondary_idx];
                    if first_secondary.target == edge_end.idx {
                        primary_edge.next_edge = first_secondary.next_edge;
                        layer_meta.vacant_edge_slots.push_back(first_secondary_idx);
                    } else {
                        let mut curr_idx = first_secondary_idx;
                        while let Some(next_idx) = layer_meta.secondary_edges[curr_idx].next_edge {
                            let next_edge = layer_meta.secondary_edges[next_idx];
                            if next_edge.target == edge_end.idx {
                                layer_meta.secondary_edges[curr_idx].next_edge =
                                    next_edge.next_edge;
                                layer_meta.vacant_edge_slots.push_back(next_idx);
                                break;
                            }
                            curr_idx = next_idx;
                        }
                    }
                }
            }
        }
        for node in result.nodes {
            locked_layers[node.layer].meta.statuses[node.idx].generation += 1;
            locked_layers[node.layer].meta.statuses[node.idx].currently_exists = false;
            locked_layers[node.layer]
                .meta
                .vacant_comp_slots
                .push_back(node.idx);
        }
    }

    fn run(
        inner: &mut GatherInner,
        locked_layers: &mut [RwLockWriteGuard<'_, TypeErasedLayer>],
    ) -> GatherResult {
        let mut ret = GatherResult {
            nodes: Vec::new(),
            edges: Vec::new(),
        };

        ret.nodes.push(inner.root);

        // recursive depth first search to find all nodes and edges
        fn search_all(
            curr_node: BareNodeKey,
            ret: &mut GatherResult,
            locked_layers: &[RwLockWriteGuard<TypeErasedLayer>],
            stop_at_layers: &[usize],
        ) {
            let curr_layer = &locked_layers[curr_node.layer];
            for (target_layer_idx, edges_to_target) in curr_layer.meta.edges.iter().enumerate() {
                if curr_node.idx < edges_to_target.len() {
                    // iterate over the list of edges
                    let mut curr_edge = edges_to_target[curr_node.idx];
                    // the whole node is being deleted, so every outgoing edge is too.
                    // only push the first edge, the rest will be deleted by following the list
                    let mut primary_edge = true;
                    while let Some(edge) = curr_edge {
                        let next_node = BareNodeKey {
                            layer: target_layer_idx,
                            idx: edge.target,
                        };

                        if stop_at_layers.contains(&next_node.layer) {
                            // the next node won't be deleted. this requires some extra attention
                            // since it needs to be plucked out of the edge list
                            // instead of deleting the whole list
                            ret.edges.push((next_node, curr_node));
                            if primary_edge {
                                ret.edges.push((curr_node, next_node));
                            }
                        } else if let Some(already_seen) =
                            ret.nodes.iter().find(|n| **n == next_node)
                        {
                            if primary_edge {
                                ret.edges.push((curr_node, *already_seen));
                            }
                        } else {
                            if primary_edge {
                                ret.edges.push((curr_node, next_node));
                            }
                            ret.nodes.push(next_node);
                            search_all(next_node, ret, locked_layers, stop_at_layers);
                        }

                        primary_edge = false;
                        curr_edge = edge
                            .next_edge
                            .map(|next| curr_layer.meta.secondary_edges[next]);
                    }
                }
            }
        }
        search_all(inner.root, &mut ret, locked_layers, &inner.stop_at_layers);

        ret
    }
}

//
// Tests
//

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Pose(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Velocity(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Body(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Shape(usize);

    type Graph = make_graph! {
        Pose,
        Velocity,
        Body,
        Shape,
    };

    // shorthands for layer views because we have to repeat this stuff a lot here
    type L<'a, T> = LayerView<'a, T>;
    type LM<'a, T> = LayerViewMut<'a, T>;
    type AllLayers<'a> = (L<'a, Pose>, L<'a, Velocity>, L<'a, Body>, L<'a, Shape>);
    type AllLayersMut<'a> = (LM<'a, Pose>, LM<'a, Velocity>, LM<'a, Body>, LM<'a, Shape>);

    /// Nodes can be connected and then queried for their neighbors.
    #[test]
    fn connect_nodes() {
        let graph = Graph::new();

        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let pose_key = {
                let (mut poses, mut vels, mut bodies, _) = graph.get_layer_bundle::<AllLayersMut>();

                let mut pose_node = poses.insert(Pose(i));
                let mut vel_node = vels.insert(Velocity(i));
                let mut body_node = bodies.insert(Body(i));
                vel_node.connect(&mut pose_node);
                body_node.connect(&mut pose_node);
                body_node.connect(&mut vel_node);
                // neighbors are found
                // (getting them is cumbersome here because we have to juggle layer references
                // back and forth to drop mutable refs, but this won't be done in real code)
                pose_node.key()
            };
            {
                let (poses, _, bodies, shapes) = graph.get_layer_bundle::<AllLayers>();
                assert_eq!(
                    *poses
                        .get(pose_key)
                        .unwrap()
                        .get_neighbor(&bodies)
                        .unwrap()
                        .c,
                    Body(i)
                );
                assert!(poses.get(pose_key).unwrap().get_neighbor(&shapes).is_none());
            }

            // spawn something with different connections in between
            let mut poses = graph.get_layer_mut::<Pose>();
            let mut shapes = graph.get_layer_mut::<Shape>();
            let mut pose_node_ = poses.insert(Pose(42 + i));
            let mut shape_node_ = shapes.insert(Shape(i));
            pose_node_.connect(&mut shape_node_);
            let pose_key_ = pose_node_.key();
            drop(poses);
            drop(shapes);
            assert_eq!(
                *graph
                    .get_layer::<Pose>()
                    .get(pose_key_)
                    .unwrap()
                    .get_neighbor(&graph.get_layer::<Shape>())
                    .unwrap()
                    .c,
                Shape(i)
            );
        }
    }

    /// Iterating over nodes hits every alive node and only every alive node.
    #[test]
    fn iterate() {
        let graph = Graph::new();
        let (mut poses, mut vels, mut bodies, mut shapes) =
            graph.get_layer_bundle::<AllLayersMut>();

        for i in 0..10 {
            let mut pose_node = poses.insert(Pose(i));
            let mut vel_node = vels.insert(Velocity(i));
            let mut body_node = bodies.insert(Body(0));
            body_node.connect(&mut pose_node);
            if i % 2 == 0 {
                pose_node.connect(&mut vel_node);
            }
            if i % 4 == 0 {
                let mut shape_node = shapes.insert(Shape(i));
                body_node.connect(&mut shape_node);
            }
        }

        drop(poses);
        drop(vels);

        let poses = graph.get_layer::<Pose>();
        let vels = graph.get_layer::<Velocity>();

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for mut body in bodies.iter_mut() {
            let pose = match body.get_neighbor(&poses) {
                Some(pose) => pose,
                None => continue,
            };
            let vel = match pose.get_neighbor(&vels) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;
            body.c.0 = 42;

            let mut shape = body.get_neighbor_mut(&mut shapes);
            if let Some(shape) = &mut shape {
                full_match_count += 1;
                shape.c.0 += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.c.0 % 2, 0);

            println!(
                "{:?}, {:?}, {:?}, {:?}",
                body.c,
                pose.c,
                vel.c,
                shape.map(|s| s.c)
            );
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);

        println!("All rbs: {:?}", bodies.components);

        for body in bodies.iter() {
            if body
                .get_neighbor(&poses)
                .and_then(|pose| pose.get_neighbor(&vels))
                .is_none()
            {
                assert_eq!(body.c.0, 0);
            } else {
                assert_eq!(body.c.0, 42);
            }
        }
    }

    /// Deleting hits every intended node.
    #[test]
    fn delete() {
        let mut graph = Graph::new();

        let vels_to_del: Vec<NodeKey<Velocity>> = {
            let (mut poses, mut vels, mut bodies, mut shapes) =
                graph.get_layer_bundle::<AllLayersMut>();

            for i in 0..10 {
                let mut pose_node = poses.insert(Pose(i));
                let mut vel_node = vels.insert(Velocity(i));
                let mut body_node = bodies.insert(Body(0));
                body_node.connect(&mut pose_node);
                if i % 2 == 0 {
                    pose_node.connect(&mut vel_node);
                } else {
                    let mut shape = shapes.insert(Shape(i));
                    vel_node.connect(&mut shape);
                }
            }

            assert_eq!(vels.iter().count(), 10);

            vels.iter().map(|v| v.key()).collect()
        };
        for vel_to_del in vels_to_del {
            graph.gather(vel_to_del).delete();
        }
        // all vels deleted
        assert_eq!(graph.get_layer::<Velocity>().iter().count(), 0);
        // half of poses had vels attached and have also been deleted
        assert_eq!(graph.get_layer::<Pose>().iter().count(), 5);

        let bodies_to_del: Vec<NodeKey<Body>> = graph
            .get_layer::<Body>()
            .iter()
            .map(|rb| rb.key())
            .collect();
        // rbs are connected to everything so deleting all of them should delete everything
        for rb_to_del in bodies_to_del {
            graph.gather(rb_to_del).delete();
        }
        assert_eq!(graph.get_layer::<Shape>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Pose>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Velocity>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Body>().iter().count(), 0);

        // check that edges were all cleared too
        for (layer_idx, layer) in graph.layers.iter().map(|l| l.read()).enumerate() {
            for edge in layer.meta.edges.iter().flat_map(|e| e.iter()) {
                assert!(edge.is_none(), "Layer {} had a non-empty edge", layer_idx);
            }
        }
    }

    /// Slots left over by `delete` are reused when spawning more components.
    #[test]
    fn reuse_deleted_slots() {
        let mut graph = Graph::new();

        // delete and respawn stuff a few times to hopefully see if things get connected wrong at some point
        // (this doesn't prove things won't go wrong over a long enough time but fingers crossed)
        for i in 0..5 {
            let pose_nodes: Vec<NodeKey<Pose>> = graph
                .get_layer_mut::<Pose>()
                .iter()
                // delete half of everything that was placed,
                // so every new iteration we should be reusing slots for half and pushing new for half
                .take(5)
                .map(|pose| pose.key())
                .collect();
            for pose_node in pose_nodes {
                graph.gather(pose_node).delete();
            }
            for j in 0..10 {
                let pose_key;
                {
                    let (mut poses, mut vels, mut bodies, _) =
                        graph.get_layer_bundle::<AllLayersMut>();

                    let id = i * 20 + j;
                    let mut pose_node = poses.insert(Pose(id));
                    let mut vel_node = vels.insert(Velocity(id));
                    let mut rb_node = bodies.insert(Body(id));
                    pose_node.connect(&mut vel_node);
                    vel_node.connect(&mut rb_node);

                    pose_key = pose_node.key();
                }
                // delete and replace on every other loop
                if i % 2 == 0 {
                    let pose_len_before = graph.get_layer::<Pose>().components.len();

                    graph.gather(pose_key).delete();
                    let mut poses = graph.get_layer_mut::<Pose>();
                    assert!(
                        poses.get(pose_key).is_none(),
                        "pose_node was deleted so checking it should now give None"
                    );
                    let mut vels = graph.get_layer_mut::<Velocity>();
                    let mut bodies = graph.get_layer_mut::<Body>();

                    let mut pose_node = poses.insert(Pose(100));
                    let mut vel_node = vels.insert(Velocity(100));
                    pose_node.connect(&mut vel_node);
                    if i % 4 == 0 {
                        let mut body_node = bodies.insert(Body(100));
                        body_node.connect(&mut pose_node);
                    }

                    let pose_len_after = poses.components.len();
                    // reused the slot that was left behind by delete
                    assert_eq!(pose_len_before, pose_len_after);
                }
            }
        }

        let (poses, vels, bodies, _) = graph.get_layer_bundle::<AllLayers>();
        println!("poses.content: {:?}", &poses.components);
        println!("vels.content: {:?}", &vels.components);
        println!("bodies.content: {:?}", &bodies.components);

        assert_eq!(poses.components.len(), 30);
        assert_eq!(vels.components.len(), 30);
    }

    /// If stopping layers are specified, delete algorithm stops there as intended.
    #[test]
    fn delete_stop() {
        let mut graph = Graph::new();

        let mut pose_keys = Vec::new();
        {
            let (mut poses, mut vels, mut bodies, mut shapes): AllLayersMut =
                graph.get_layer_bundle();
            for i in 0..10 {
                let mut pose = poses.insert(Pose(i));
                let mut vel = vels.insert(Velocity(i));
                let mut body = bodies.insert(Body(i));
                let mut shape = shapes.insert(Shape(i));
                // triangle shape of pose, body, and vel,
                // attached to shape at the body
                pose.connect(&mut vel);
                vel.connect(&mut body);
                body.connect(&mut pose);
                body.connect(&mut shape);

                pose_keys.push(pose.key());
            }
        }

        for (i, pose) in pose_keys.iter().enumerate() {
            let mut gather = graph.gather(*pose).stop_at_layer::<Body>();
            if i % 2 == 0 {
                gather = gather.stop_at_layer::<Velocity>();
            }
            gather.delete();
        }

        let (poses, vels, bodies, shapes): AllLayers = graph.get_layer_bundle();
        // poses should all be deleted, half of vels should, and none of bodies and shapes
        assert_eq!(poses.iter().count(), 0);
        assert_eq!(vels.iter().count(), 5);
        assert_eq!(bodies.iter().count(), 10);
        assert_eq!(shapes.iter().count(), 10);
        for vel in vels.iter() {
            let body = vel.get_neighbor(&bodies).expect("unwanted edge deleted");
            let _shape = body.get_neighbor(&shapes).expect("unwanted edge deleted");
        }
        for body in bodies.iter() {
            let _shape = body.get_neighbor(&shapes).expect("unwanted edge deleted");
        }
    }

    /// Multiple edges from one node to one layer are created, followed, and destroyed correctly.
    #[test]
    fn multiple_edges() {
        let mut graph = Graph::new();

        // do the whole thing a few times to ensure we don't leave garbage
        for _ in 0..3 {
            let mut poses = graph.get_layer_mut::<Pose>();
            let mut bodies = graph.get_layer_mut::<Body>();
            let mut shapes = graph.get_layer_mut::<Shape>();

            let mut pose = poses.insert(Pose(0));
            let mut shape = shapes.insert(Shape(0));
            pose.connect(&mut shape);
            let mut shape_connected_body_keys = Vec::new();
            for i in 0..10 {
                let mut body = bodies.insert(Body(i));
                pose.connect(&mut body);
                if i % 2 == 0 {
                    shape.connect(&mut body);
                    shape_connected_body_keys.push(body.key());
                }
            }
            assert_eq!(bodies.iter().count(), 10);

            // all neighbors are found

            assert_eq!(pose.get_all_neighbors(&bodies.subview()).count(), 10);
            assert_eq!(shape.get_all_neighbors(&bodies.subview()).count(), 5);

            let mut conn_bodies = pose.get_all_neighbors_mut(&mut bodies);
            while let Some(body) = conn_bodies.next() {
                body.c.0 = 69;
            }
            for body in shape.get_all_neighbors(&bodies.subview()) {
                assert_eq!(body.c.0, 69);
            }

            let pose_key = pose.key();
            for &body in &shape_connected_body_keys {
                let b = bodies.get(body).unwrap();
                assert_eq!(b.get_neighbor(&poses.subview()).unwrap().key(), pose_key);
            }

            let shape_key = shape.key();

            // delete gets everything

            drop(poses);
            drop(bodies);
            drop(shapes);
            // half of bodies should go with this, pose should survive with 5 attached
            graph.gather(shape_key).stop_at_layer::<Pose>().delete();

            let poses = graph.get_layer::<Pose>();
            let bodies = graph.get_layer::<Body>();

            assert_eq!(bodies.iter().count(), 5);
            let pose = poses.get(pose_key).expect("Pose was deleted??");
            assert_eq!(pose.get_all_neighbors(&bodies).count(), 5);
            for body in pose.get_all_neighbors(&bodies) {
                assert!(!shape_connected_body_keys.contains(&body.key()));
            }

            drop(poses);
            drop(bodies);

            // delete the rest of everything
            graph.gather(pose_key).delete();

            let poses = graph.get_layer::<Pose>();
            let bodies = graph.get_layer::<Body>();
            for edge in &bodies.meta.edges[Pose::LAYER_INDEX] {
                assert!(edge.is_none());
            }
            for edge in &bodies.meta.edges[Shape::LAYER_INDEX] {
                assert!(edge.is_none());
            }
            // all secondary edges became vacant (10 edges - 1 primary edge)
            assert_eq!(poses.meta.vacant_edge_slots.len(), 9);
            // secondary edges stay after delete,
            // but none more were added after first iteration
            // because vacant slots were used
            assert_eq!(poses.meta.secondary_edges.len(), 9);
        }
    }
}
