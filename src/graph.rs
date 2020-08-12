use std::collections::VecDeque;
use std::marker::PhantomData;

//
// Index & ref types
//

type ComponentIdx = usize;
type LayerIdx = usize;
type GenerationIdx = usize;
type Refcount = usize;

pub trait UnsafeNode {
    fn pos(&self) -> NodePosition;
}

pub trait SafeNode: UnsafeNode + Sized {
    type MarkerType;
    fn pin(&self, graph: &mut Graph) -> PinnedNode<Self::MarkerType> {
        graph.pin(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePosition {
    pub(crate) layer_idx: LayerIdx,
    pub(crate) item_idx: ComponentIdx,
}
impl UnsafeNode for NodePosition {
    fn pos(&self) -> NodePosition {
        *self
    }
}

pub struct Node<T> {
    pos: NodePosition,
    gen: GenerationIdx,
    _marker: PhantomData<*const T>,
}
impl<T> Node<T> {
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

pub struct NodeRef<'a, T> {
    pub item: &'a T,
    pos: NodePosition,
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
    pub fn node(&self, graph: &Graph) -> Node<T> {
        Node {
            pos: self.pos,
            gen: graph.generations[self.pos.layer_idx][self.pos.item_idx],
            _marker: PhantomData,
        }
    }
}

pub struct NodeRefMut<'a, T> {
    pub item: &'a mut T,
    pos: NodePosition,
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
    pub fn node(&self, graph: &Graph) -> Node<T> {
        Node {
            pos: self.pos,
            gen: graph.generations[self.pos.layer_idx][self.pos.item_idx],
            _marker: PhantomData,
        }
    }
}

//
// Graph
//

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
    pub fn new() -> Self {
        Self {
            edge_layers: Vec::new(),
            refcounts: Vec::new(),
            generations: Vec::new(),
            vacant_slots: Vec::new(),
        }
    }

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

    pub fn connect(&mut self, node1: &impl SafeNode, node2: &impl SafeNode) {
        self.connect_oneway(node1, node2);
        self.connect_oneway(node2, node1);
    }

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

    pub fn get_neighbor<'to, To>(
        &self,
        node: &impl SafeNode,
        to_layer: &'to Layer<To>,
    ) -> Option<NodeRef<'to, To>> {
        self.get_neighbor_unchecked(node, to_layer)
    }

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

    pub fn unpin<T>(&mut self, pin: PinnedNode<T>) {
        self.refcounts[pin.pos.layer_idx][pin.pos.item_idx] -= 1;

        if self.refcounts[pin.pos.layer_idx][pin.pos.item_idx] == 0 {
            self.vacant_slots[pin.pos.layer_idx].push_back(pin.pos.item_idx);
            self.generations[pin.pos.layer_idx][pin.pos.item_idx] += 1;
        }
    }

    pub fn get_refcount(&self, node: &impl SafeNode) -> Refcount {
        self.get_refcount_unchecked(node)
    }

    pub fn get_refcount_unchecked(&self, node: &impl UnsafeNode) -> Refcount {
        let node = node.pos();
        self.refcounts[node.layer_idx][node.item_idx]
    }

    pub fn delete(&mut self, root: impl SafeNode) {
        let root = root.pos();
        // check if the node is already considered deleted before doing anything
        if self.refcounts[root.layer_idx][root.item_idx] <= 0 {
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
            if self.refcounts[node.layer_idx][node.item_idx] <= 0 {
                debug_assert!(
                    self.vacant_slots[node.layer_idx]
                        .iter()
                        .find(|&&idx| idx == node.item_idx)
                        .is_none(),
                    format!("Same slot marked vacant twice ({:?})", node),
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

#[derive(Debug)]
pub struct Layer<T> {
    index: LayerIdx,
    content: Vec<T>,
}

impl<T> Layer<T> {
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

    pub fn get(&self, node: impl SafeNode) -> NodeRef<'_, T> {
        let pos = node.pos();
        self.get_unchecked(pos)
    }

    pub fn get_unchecked(&self, pos: NodePosition) -> NodeRef<'_, T> {
        NodeRef {
            item: &self.content[pos.item_idx],
            pos,
        }
    }

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
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        // TODO: make this a pinned node instead once pinning is a thing
        let everyones_shape = shapes.insert(Shape(69), &mut graph).node(&graph);
        let everyones_shape = everyones_shape.check(&graph).unwrap();

        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(i), &mut graph);
            graph.connect(&vel_node, &tr_node);
            graph.connect(&rb_node, &tr_node);
            graph.connect(&rb_node, &vel_node);
            graph.connect_oneway(&rb_node, &everyones_shape);
            // refcounts
            assert_eq!(graph.get_refcount(&tr_node), 2);
            assert_eq!(graph.get_refcount(&rb_node), 2);
            assert_eq!(graph.get_refcount(&everyones_shape), i + 1);
            // neighbors are found
            assert_eq!(
                graph.get_neighbor(&rb_node, &shapes).unwrap().item,
                &Shape(69)
            );
            assert_eq!(
                graph.get_neighbor(&tr_node, &rbs).unwrap().item,
                &RigidBody(i)
            );
            assert!(graph.get_neighbor(&tr_node, &shapes).is_none());

            // spawn something with different connections in between
            let tr_node_ = trs.insert(Transform(42 + i), &mut graph);
            let shape_node_ = shapes.insert(Shape(i), &mut graph);
            graph.connect(&tr_node_, &shape_node_);
            assert_eq!(
                graph.get_neighbor(&tr_node_, &shapes).unwrap().item,
                &Shape(i)
            );
        }
    }

    #[test]
    fn iterate() {
        let mut graph = Graph::new();
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph).pin(&mut graph);

        for i in 0..10 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(&rb_node, &tr_node);
            if i % 2 == 0 {
                graph.connect(&tr_node, &vel_node);
            }
            if i % 4 == 0 {
                graph.connect_oneway(&rb_node, &everyones_shape);
            }
        }

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for mut rb in rbs.iter_mut(&graph) {
            let tr = match graph.get_neighbor(&rb, &trs) {
                Some(tr) => tr,
                None => continue,
            };
            let vel = match graph.get_neighbor(&tr, &vels) {
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
                tr.item,
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
                .get_neighbor(&rb, &trs)
                .and_then(|tr| graph.get_neighbor(&tr, &vels))
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
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();
        let mut sub_shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph);
        let shape_owns_thing = sub_shapes.insert(Shape(42), &mut graph);
        graph.connect(&everyones_shape, &shape_owns_thing);

        for i in 0..10 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(&rb_node, &tr_node);
            if i % 2 == 0 {
                graph.connect(&tr_node, &vel_node);
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
        // half of trs had vels attached and have also been deleted
        assert_eq!(trs.iter(&graph).count(), 5);

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
        assert_eq!(trs.iter(&graph).count(), 0);
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
            assert!(
                edge.is_none(),
                format!("Layer {} had a non-empty edge", layer_idx),
            );
        }
    }

    #[test]
    fn reuse_deleted_slots() {
        let mut graph = Graph::new();
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph).pin(&mut graph);

        // delete and respawn stuff a few times to hopefully see if things get connected wrong at some point
        // (this doesn't prove things won't go wrong over a long enough time but fingers crossed)
        for i in 0..5 {
            for tr_node in trs
                .iter(&graph)
                // delete half of everything that was placed,
                // so every new iteration we should be reusing slots for half and pushing new for half
                .take(5)
                .map(|tr| tr.pos())
                .collect::<Vec<_>>()
            {
                graph.delete(trs.get_unchecked(tr_node));
            }
            for j in 0..10 {
                let id = i * 20 + j;
                let tr_node = trs.insert(Transform(id), &mut graph);
                let vel_node = vels.insert(Velocity(id), &mut graph);
                let rb_node = rbs.insert(RigidBody(id), &mut graph);
                graph.connect(&tr_node, &vel_node);
                graph.connect(&vel_node, &rb_node);
                graph.connect_oneway(&tr_node, &everyones_shape);
                // delete and replace every other
                if i % 2 == 0 {
                    let tr_node = tr_node.node(&graph); // drop the ref to the layer

                    let tr_len_before = trs.content.len();

                    graph.delete(
                        trs.get(
                            tr_node
                                .check(&graph)
                                .expect("tr_node wasn't deleted yet so it should be there"),
                        ),
                    );
                    assert!(
                        tr_node.check(&graph).is_none(),
                        "tr_node was deleted so checking it should now give None"
                    );

                    let tr_node = trs.insert(Transform(100), &mut graph);
                    let vel_node = vels.insert(Velocity(100), &mut graph);
                    graph.connect(&tr_node, &vel_node);
                    if i % 4 == 0 {
                        let rb_node = rbs.insert(RigidBody(100), &mut graph);
                        graph.connect(&rb_node, &tr_node);
                    }

                    let tr_len_after = trs.content.len();
                    // reused the slot that was left behind by delete
                    assert_eq!(tr_len_before, tr_len_after);
                }
            }
        }

        println!("trs.content: {:?}", trs.content);
        println!("vels.content: {:?}", vels.content);
        println!("rbs.content: {:?}", rbs.content);

        assert_eq!(trs.content.len(), 30);
        assert_eq!(vels.content.len(), 30);
        // everyones_shape was never deleted
        assert_eq!(shapes.content.len(), 1);
    }

    #[test]
    fn pin() {
        let mut graph = Graph::new();
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();

        let tr = trs.insert(Transform(0), &mut graph);
        let tr_pin = tr.pin(&mut graph);
        let tr = tr.node(&graph); // just to be able to delete and still have the pinned node
                                  // should appear in the iterator even though it's not connected to anything
        assert_eq!(trs.iter(&graph).count(), 1);

        // awkward way to drop the reference to `vels` but this is unlikely to be needed in an actual spawning function
        let vel = vels.insert(Velocity(0), &mut graph).node(&graph);
        let vel_check = vel.check(&graph).unwrap();
        let rb = rbs.insert(RigidBody(0), &mut graph).node(&graph);
        let rb_check = rb.check(&graph).unwrap();

        graph.connect(&tr_pin, &vel_check);
        graph.connect_oneway(&tr_pin, &rb_check);
        graph.connect_oneway(&vel_check, &rb_check);

        // this should not delete anything because the root node of deletion is pinned
        graph.delete(tr.check(&graph).unwrap());
        assert!(graph.get_neighbor(&tr_pin, &vels).is_some());
        assert!(graph.get_neighbor(&tr_pin, &rbs).is_some());
        assert!(graph.get_neighbor(&vel_check, &rbs).is_some());

        // unpinning and then deleting should delete everything
        graph.unpin(tr_pin);
        graph.delete(tr.check(&graph).unwrap());
        assert!(tr.check(&graph).is_none());
        assert!(vel.check(&graph).is_none());
        assert!(rb.check(&graph).is_none());
    }
}
