use std::marker::PhantomData;

//
// Index & ref types
//

type ComponentIdx = usize;
type LayerIdx = usize;
type Refcount = usize;

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
    edge_layers: Vec<Vec<Vec<Option<ComponentIdx>>>>,
    /// 2D array:
    /// * 1st dimension is the layer
    /// * 2nd dimension is the component
    refcounts: Vec<Vec<Refcount>>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            edge_layers: Vec::new(),
            refcounts: Vec::new(),
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

        // add refcounts for the layer
        self.refcounts.push(Vec::new());

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

        // increase refcount for the target, not the source
        self.refcounts[end.layer_idx][end.item_idx] += 1;
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

    pub fn get_refcount(&self, node: impl Into<AnyNode>) -> Refcount {
        let node = node.into();
        self.refcounts[node.layer_idx][node.item_idx]
    }

    pub fn delete(&mut self, root: impl Into<AnyNode>) {
        let root = root.into();
        let mut visited = Vec::new();
        self.recursive_delete(root, &mut visited);
    }

    fn recursive_delete(&mut self, node: AnyNode, visited: &mut Vec<AnyNode>) {
        visited.push(node);

        for other_layer_idx in 0..self.edge_layers.len() {
            let edges_to_other = &mut self.edge_layers[node.layer_idx][other_layer_idx];
            if node.item_idx < edges_to_other.len() {
                if let Some(other_item_idx) = edges_to_other[node.item_idx] {
                    edges_to_other[node.item_idx] = None;
                    self.refcounts[other_layer_idx][other_item_idx] -= 1;

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

pub struct Layer<T> {
    index: LayerIdx,
    content: Vec<T>,
}

impl<T> Layer<T> {
    pub fn insert(&mut self, component: T, graph: &mut Graph) -> TypedNode<T> {
        let item_idx = self.content.len();
        self.content.push(component);
        graph.refcounts[self.index].push(0);

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
            refcounts: &graph.refcounts[self.index],
        }
    }

    pub fn iter_mut(&mut self) -> LayerIterMut<'_, T> {
        LayerIterMut {
            iter: self.content.iter_mut().enumerate(),
            layer_idx: self.index,
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
}
impl<'a, T> Iterator for LayerIterMut<'a, T> {
    type Item = NodeRefMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;
        Some((
            item,
            AnyNode {
                item_idx,
                layer_idx: self.layer_idx,
            }
            .typed(),
        ))
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
            // check refcounts
            assert_eq!(graph.get_refcount(tr_node), 2);
            assert_eq!(graph.get_refcount(rb_node), 2);
            assert_eq!(graph.get_refcount(everyones_shape), i + 1);

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
        for (mut rb, rb_pos) in rbs.iter_mut() {
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

        println!("Refcounts after creations {:?}", graph.refcounts);

        assert_eq!(vels.iter(&graph).count(), 10);
        for vel_to_del in vels.iter(&graph).map(|(_, p)| p).collect::<Vec<_>>() {
            graph.delete(vel_to_del);
        }
        // all vels deleted (== have 0 referrers)
        assert_eq!(vels.iter(&graph).count(), 0);
        // half of trs had vels attached and have also been deleted
        assert_eq!(trs.iter(&graph).count(), 5);

        println!("Refcounts after first deletions {:?}", graph.refcounts);

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
    }
}
