use std::collections::VecDeque;
use std::marker::PhantomData;

//
// Index & ref types
//

type ComponentIdx = usize;
type LayerIdx = usize;
type EdgeCount = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnyNode {
    pub(crate) item_idx: ComponentIdx,
    layer_idx: LayerIdx,
}
impl AnyNode {
    pub(self) fn typed<T>(self) -> TypedNode<T> {
        TypedNode {
            node: self,
            _marker: PhantomData,
        }
    }
}
pub struct TypedNode<T> {
    pub(crate) node: AnyNode,
    _marker: PhantomData<*const T>,
}
impl<T> Into<AnyNode> for TypedNode<T> {
    fn into(self) -> AnyNode {
        self.node
    }
}
// blanket impls required because derive restricts type of T
impl<T> Clone for TypedNode<T> {
    fn clone(&self) -> Self {
        TypedNode {
            node: self.node,
            _marker: PhantomData,
        }
    }
}
impl<T> Copy for TypedNode<T> {}
impl<T> std::fmt::Debug for TypedNode<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.node.fmt(f)
    }
}
impl<T> PartialEq for TypedNode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}
impl<T> Eq for TypedNode<T> {}

pub type NodeRef<'a, T> = (&'a T, TypedNode<T>);
pub type NodeRefMut<'a, T> = (&'a mut T, TypedNode<T>);

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
    edge_counts: Vec<Vec<EdgeCount>>,
    /// FIFO queue for slot reuse
    vacant_slots: Vec<VecDeque<ComponentIdx>>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            edge_layers: Vec::new(),
            edge_counts: Vec::new(),
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

        // add refcounts and vacant slot queues for the layer
        self.edge_counts.push(Vec::new());
        self.vacant_slots.push(VecDeque::new());

        Layer {
            index: next_idx,
            content: Vec::new(),
        }
    }

    pub fn connect(&mut self, node1: impl Into<AnyNode>, node2: impl Into<AnyNode>) {
        let node1 = node1.into();
        let node2 = node2.into();
        self.connect_oneway(node1, node2);
        self.connect_oneway(node2, node1);
    }

    pub fn connect_oneway(&mut self, start: impl Into<AnyNode>, end: impl Into<AnyNode>) {
        let start = start.into();
        let end = end.into();
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

        self.edge_counts[start.layer_idx][start.item_idx] += 1;
        self.edge_counts[end.layer_idx][end.item_idx] += 1;
    }

    pub fn get_neighbor<'to, To>(
        &self,
        node: impl Into<AnyNode>,
        to_layer: &'to Layer<To>,
    ) -> Option<NodeRef<'to, To>> {
        let node = node.into();
        let edge_layer = &self.edge_layers[node.layer_idx][to_layer.index];
        if edge_layer.len() <= node.item_idx {
            None
        } else {
            let to_id = edge_layer[node.item_idx]?;
            Some((
                &to_layer.content[to_id],
                AnyNode {
                    item_idx: to_id,
                    layer_idx: to_layer.index,
                }
                .typed(),
            ))
        }
    }

    pub fn get_neighbor_mut<'to, To>(
        &self,
        node: impl Into<AnyNode>,
        to_layer: &'to mut Layer<To>,
    ) -> Option<NodeRefMut<'to, To>> {
        let node = node.into();
        let edge_layer = &self.edge_layers[node.layer_idx][to_layer.index];
        if edge_layer.len() <= node.item_idx {
            None
        } else {
            let to_id = edge_layer[node.item_idx]?;
            Some((
                &mut to_layer.content[to_id],
                AnyNode {
                    item_idx: to_id,
                    layer_idx: to_layer.index,
                }
                .typed(),
            ))
        }
    }

    pub fn get_edge_count(&self, node: impl Into<AnyNode>) -> EdgeCount {
        let node = node.into();
        self.edge_counts[node.layer_idx][node.item_idx]
    }

    pub fn delete(&mut self, root: impl Into<AnyNode>) {
        let root = root.into();
        // check if the node is already considered deleted before doing anything
        if self.edge_counts[root.layer_idx][root.item_idx] <= 0 {
            return;
        }
        let mut visited = Vec::new();
        self.recursive_delete(root, &mut visited);

        for node in visited {
            if self.edge_counts[node.layer_idx][node.item_idx] <= 0 {
                debug_assert!(
                    self.vacant_slots[node.layer_idx]
                        .iter()
                        .find(|&&idx| idx == node.item_idx)
                        .is_none(),
                    format!("Same slot marked vacant twice ({:?})", node),
                );
                self.vacant_slots[node.layer_idx].push_back(node.item_idx);
            }
        }
    }

    fn recursive_delete(&mut self, node: AnyNode, visited: &mut Vec<AnyNode>) {
        visited.push(node);

        // early out if the node has no more connections
        if self.edge_counts[node.layer_idx][node.item_idx] <= 0 {
            return;
        }

        for other_layer_idx in 0..self.edge_layers.len() {
            let edges_to_other = &mut self.edge_layers[node.layer_idx][other_layer_idx];
            if node.item_idx < edges_to_other.len() {
                if let Some(other_item_idx) = edges_to_other[node.item_idx] {
                    edges_to_other[node.item_idx] = None;
                    self.edge_counts[other_layer_idx][other_item_idx] -= 1;
                    self.edge_counts[node.layer_idx][node.item_idx] -= 1;

                    let next_node = AnyNode {
                        layer_idx: other_layer_idx,
                        item_idx: other_item_idx,
                    };
                    if !visited.iter().any(|seen_node| *seen_node == next_node) {
                        self.recursive_delete(next_node, visited);
                    }
                }
            }
        }
    }
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
    pub fn insert(&mut self, component: T, graph: &mut Graph) -> TypedNode<T> {
        let item_idx = if let Some(vacant_slot) = graph.vacant_slots[self.index].pop_front() {
            self.content[vacant_slot] = component;
            vacant_slot
        } else {
            self.content.push(component);
            graph.edge_counts[self.index].push(0);
            self.content.len() - 1
        };

        AnyNode {
            layer_idx: self.index,
            item_idx,
        }
        .typed()
    }

    pub fn get(&self, node: TypedNode<T>) -> Option<&T> {
        if node.node.layer_idx == self.index {
            self.content.get(node.node.item_idx)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, node: TypedNode<T>) -> Option<&mut T> {
        if node.node.layer_idx == self.index {
            self.content.get_mut(node.node.item_idx)
        } else {
            None
        }
    }

    pub fn iter<'s, 'g: 's>(&'s self, graph: &'g Graph) -> LayerIter<'s, T> {
        LayerIter {
            iter: self.content.iter().enumerate(),
            layer_idx: self.index,
            refcounts: &graph.edge_counts[self.index],
        }
    }

    pub fn iter_mut<'s, 'g: 's>(&'s mut self, graph: &'g Graph) -> LayerIterMut<'s, T> {
        LayerIterMut {
            iter: self.content.iter_mut().enumerate(),
            layer_idx: self.index,
            refcounts: &graph.edge_counts[self.index],
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
    refcounts: &'a Vec<EdgeCount>,
}
impl<'a, T> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;
        if self.refcounts[item_idx] > 0 {
            Some((
                item,
                AnyNode {
                    item_idx,
                    layer_idx: self.layer_idx,
                }
                .typed(),
            ))
        } else {
            self.next()
        }
    }
}

pub struct LayerIterMut<'a, T> {
    iter: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
    layer_idx: LayerIdx,
    refcounts: &'a Vec<EdgeCount>,
}
impl<'a, T> Iterator for LayerIterMut<'a, T> {
    type Item = NodeRefMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;
        if self.refcounts[item_idx] > 0 {
            Some((
                item,
                AnyNode {
                    item_idx,
                    layer_idx: self.layer_idx,
                }
                .typed(),
            ))
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

        let everyones_shape = shapes.insert(Shape(69), &mut graph);
        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(i), &mut graph);
            graph.connect(vel_node, tr_node);
            graph.connect(rb_node, tr_node);
            graph.connect(rb_node, vel_node);
            graph.connect_oneway(rb_node, everyones_shape);
            assert_eq!(
                graph.get_neighbor(rb_node, &shapes).map(|(n, _)| n),
                Some(&Shape(69))
            );
            assert_eq!(
                graph.get_neighbor(tr_node, &rbs).map(|(n, _)| n),
                Some(&RigidBody(i))
            );
            assert!(graph.get_neighbor(tr_node, &shapes).is_none());
            // check edge counts
            assert_eq!(graph.get_edge_count(tr_node), 4);
            assert_eq!(graph.get_edge_count(rb_node), 5);
            assert_eq!(graph.get_edge_count(everyones_shape), i + 1);

            // spawn something with different connections in between
            let tr_node_ = trs.insert(Transform(42 + i), &mut graph);
            let shape_node_ = shapes.insert(Shape(i), &mut graph);
            graph.connect(tr_node_, shape_node_);
            assert_eq!(
                graph.get_neighbor(tr_node_, &shapes).map(|(n, _)| n),
                Some(&Shape(i))
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

        let everyones_shape = shapes.insert(Shape(69), &mut graph);

        for i in 0..10 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(rb_node, tr_node);
            if i % 2 == 0 {
                graph.connect(tr_node, vel_node);
            }
            if i % 4 == 0 {
                graph.connect_oneway(rb_node, everyones_shape);
            }
        }

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for (mut rb, rb_pos) in rbs.iter_mut(&graph) {
            let (tr, tr_pos) = match graph.get_neighbor(rb_pos, &trs) {
                Some(tr) => tr,
                None => continue,
            };
            let (vel, _) = match graph.get_neighbor(tr_pos, &vels) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;
            rb.0 = 42;

            let mut shape = graph.get_neighbor_mut(rb_pos, &mut shapes);
            if let Some((shape, _)) = &mut shape {
                full_match_count += 1;
                shape.0 += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.0 % 2, 0);

            println!("{:?}, {:?}, {:?}, {:?}", rb, tr, vel, shape);
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);
        assert_eq!(shapes.get(everyones_shape).unwrap(), &Shape(72));

        println!("All rbs: {:?}", rbs.content);

        for (rb, rb_pos) in rbs.iter(&graph) {
            if graph
                .get_neighbor(rb_pos, &trs)
                .and_then(|(_, tr_pos)| graph.get_neighbor(tr_pos, &vels))
                .is_none()
            {
                assert_eq!(rb.0, 0);
            } else {
                assert_eq!(rb.0, 42);
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

        let everyones_shape = shapes.insert(Shape(69), &mut graph);

        for i in 0..10 {
            let tr_node = trs.insert(Transform(i), &mut graph);
            let vel_node = vels.insert(Velocity(i), &mut graph);
            let rb_node = rbs.insert(RigidBody(0), &mut graph);
            graph.connect(rb_node, tr_node);
            if i % 2 == 0 {
                graph.connect(tr_node, vel_node);
            } else {
                graph.connect_oneway(vel_node, vel_node); // connect vel to itself to "keep it alive"
            }
            if i % 3 == 0 {
                graph.connect_oneway(rb_node, everyones_shape);
            }
        }

        println!("Edge counts after creations {:?}", graph.edge_counts);

        assert_eq!(vels.iter(&graph).count(), 10);
        for vel_to_del in vels.iter(&graph).map(|(_, p)| p).collect::<Vec<_>>() {
            graph.delete(vel_to_del);
        }
        // all vels deleted (== have 0 referrers)
        assert_eq!(vels.iter(&graph).count(), 0);
        // half of trs had vels attached and have also been deleted
        assert_eq!(trs.iter(&graph).count(), 5);

        println!("Edge counts after first deletions {:?}", graph.edge_counts);

        // rbs are connected to everything so deleting all of them should delete everything
        for rb_to_del in rbs.iter(&graph).map(|(_, p)| p).collect::<Vec<_>>() {
            // everyones_shape should live until the last rb is deleted
            assert_eq!(shapes.iter(&graph).count(), 1);

            graph.delete(rb_to_del);
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
        // TODO: pinning API built into Graph
        let mut pins: Layer<()> = graph.create_layer();

        let everyones_shape = shapes.insert(Shape(69), &mut graph);
        let shape_pin = pins.insert((), &mut graph);
        graph.connect_oneway(shape_pin, everyones_shape);

        // delete and respawn stuff a few times to hopefully see if things get connected wrong at some point
        // (this doesn't prove things won't go wrong over a long enough time but fingers crossed)
        for i in 0..5 {
            for tr_node in trs
                .iter(&graph)
                // delete half of everything that was placed,
                // so every new iteration we should be reusing slots for half and pushing new for half
                .take(5)
                .map(|(_, n)| n)
                .collect::<Vec<_>>()
            {
                graph.delete(tr_node);
            }
            for j in 0..10 {
                let id = i * 20 + j;
                let tr_node = trs.insert(Transform(id), &mut graph);
                let vel_node = vels.insert(Velocity(id), &mut graph);
                let rb_node = rbs.insert(RigidBody(id), &mut graph);
                graph.connect(tr_node, vel_node);
                graph.connect(vel_node, rb_node);
                graph.connect_oneway(tr_node, everyones_shape);
                // delete and replace every other
                if i % 2 == 0 {
                    let tr_len_before = trs.content.len();

                    graph.delete(tr_node);
                    // delete twice to make sure we don't create garbage on the second go
                    graph.delete(tr_node);

                    let tr_node = trs.insert(Transform(100), &mut graph);
                    let vel_node = vels.insert(Velocity(100), &mut graph);
                    graph.connect(tr_node, vel_node);
                    if i % 4 == 0 {
                        let rb_node = rbs.insert(RigidBody(100), &mut graph);
                        graph.connect(rb_node, tr_node);
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
    }
}
