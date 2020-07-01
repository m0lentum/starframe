type ComponentIdx = usize;
type LayerIdx = usize;

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
}

impl Graph {
    pub fn new() -> Self {
        Self {
            edge_layers: Vec::new(),
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

        Layer {
            index: next_idx,
            content: Vec::new(),
        }
    }

    pub fn connect(&mut self, node1: NodePosition, node2: NodePosition) {
        self.connect_oneway(node1, node2);
        self.connect_oneway(node2, node1);
    }

    pub fn connect_oneway(&mut self, start: NodePosition, end: NodePosition) {
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
    }

    pub fn get_neighbor<'to, To>(
        &self,
        node: impl Into<NodePosition>,
        to_layer: &'to Layer<To>,
    ) -> Option<NodeRef<'to, To>> {
        let node_pos = node.into();
        let edge_layer = &self.edge_layers[node_pos.layer_idx][to_layer.index];
        if edge_layer.len() <= node_pos.item_idx {
            None
        } else {
            let to_id = edge_layer[node_pos.item_idx]?;
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
        node: impl Into<NodePosition>,
        to_layer: &'to mut Layer<To>,
    ) -> Option<NodeRefMut<'to, To>> {
        let node_pos = node.into();
        let edge_layer = &self.edge_layers[node_pos.layer_idx][to_layer.index];
        if edge_layer.len() <= node_pos.item_idx {
            None
        } else {
            let to_id = edge_layer[node_pos.item_idx]?;
            Some(NodeRefMut {
                item: &mut to_layer.content[to_id],
                pos: NodePosition {
                    item_idx: to_id,
                    layer_idx: to_layer.index,
                },
            })
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
    pub fn push(&mut self, component: T) -> NodePosition {
        let item_idx = self.content.len();
        self.content.push(component);
        NodePosition {
            layer_idx: self.index,
            item_idx,
        }
    }

    pub fn get(&self, pos: NodePosition) -> Option<NodeRef<'_, T>> {
        if pos.layer_idx == self.index {
            let item = self.content.get(pos.item_idx)?;
            Some(NodeRef { item, pos })
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, pos: NodePosition) -> Option<NodeRefMut<'_, T>> {
        if pos.layer_idx == self.index {
            let item = self.content.get_mut(pos.item_idx)?;
            Some(NodeRefMut { item, pos })
        } else {
            None
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            iter: self.content.iter().enumerate(),
            layer_idx: self.index,
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
}
impl<'a, T> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        let (item_idx, item) = self.iter.next()?;

        Some(NodeRef {
            item,
            pos: NodePosition {
                item_idx,
                layer_idx: self.layer_idx,
            },
        })
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

        Some(NodeRefMut {
            item,
            pos: NodePosition {
                item_idx,
                layer_idx: self.layer_idx,
            },
        })
    }
}

//
// Ref types
//

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePosition {
    pub(crate) item_idx: ComponentIdx,
    layer_idx: LayerIdx,
}
impl<T> From<&NodeRef<'_, T>> for NodePosition {
    fn from(node_ref: &NodeRef<'_, T>) -> Self {
        node_ref.pos
    }
}
impl<T> From<&NodeRefMut<'_, T>> for NodePosition {
    fn from(node_ref: &NodeRefMut<'_, T>) -> Self {
        node_ref.pos
    }
}

pub struct NodeRef<'a, T> {
    item: &'a T,
    pos: NodePosition,
}
impl<'a, T> std::ops::Deref for NodeRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.item
    }
}

pub struct NodeRefMut<'a, T> {
    item: &'a mut T,
    pos: NodePosition,
}
impl<'a, T> std::ops::Deref for NodeRefMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.item
    }
}
impl<'a, T> std::ops::DerefMut for NodeRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.item
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

        let everyones_shape = shapes.push(Shape(69));
        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let tr_node = trs.push(Transform(i));
            let vel_node = vels.push(Velocity(i));
            let rb_node = rbs.push(RigidBody(i));
            graph.connect(vel_node, tr_node);
            graph.connect(rb_node, tr_node);
            graph.connect(rb_node, vel_node);
            graph.connect_oneway(rb_node, everyones_shape);
            assert_eq!(
                graph.get_neighbor(rb_node, &shapes).map(|n| *n),
                Some(Shape(69))
            );
            assert_eq!(
                graph.get_neighbor(tr_node, &rbs).map(|n| *n),
                Some(RigidBody(i))
            );
            assert!(graph.get_neighbor(tr_node, &shapes).is_none());

            // spawn something with different connections in between
            let tr_node_ = trs.push(Transform(42 + i));
            let shape_node_ = shapes.push(Shape(i));
            graph.connect(tr_node_, shape_node_);
            assert_eq!(
                graph.get_neighbor(tr_node_, &shapes).map(|n| *n),
                Some(Shape(i))
            );
        }

        println!("Contents after `connect_nodes`:");
        println!("{:?}", trs.content);
        println!("{:?}", vels.content);
        println!("{:?}", rbs.content);
        println!("{:?}", shapes.content);
    }

    #[test]
    fn iterate() {
        let mut graph = Graph::new();
        let mut trs: Layer<Transform> = graph.create_layer();
        let mut vels: Layer<Velocity> = graph.create_layer();
        let mut rbs: Layer<RigidBody> = graph.create_layer();
        let mut shapes: Layer<Shape> = graph.create_layer();

        let everyones_shape = shapes.push(Shape(69));

        for i in 0..10 {
            let tr_node = trs.push(Transform(i));
            let vel_node = vels.push(Velocity(i));
            let rb_node = rbs.push(RigidBody(0));
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
        for mut rb in rbs.iter_mut() {
            let tr = match graph.get_neighbor(&rb, &trs) {
                Some(tr) => tr,
                None => continue,
            };
            let vel = match graph.get_neighbor(&tr, &vels) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;
            rb.0 = 42;

            let mut shape = graph.get_neighbor_mut(&rb, &mut shapes);
            if let Some(shape) = &mut shape {
                full_match_count += 1;
                shape.0 += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.0 % 2, 0);

            println!("{:?}, {:?}, {:?}, {:?}", *rb, *tr, *vel, shape.map(|s| *s));
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);
        assert_eq!(*shapes.get(everyones_shape).unwrap(), Shape(72));

        println!("All rbs: {:?}", rbs.content);

        for rb in rbs.iter() {
            if graph
                .get_neighbor(&rb, &trs)
                .and_then(|tr| graph.get_neighbor(&tr, &vels))
                .is_none()
            {
                assert_eq!(rb.0, 0);
            } else {
                assert_eq!(rb.0, 42);
            }
        }
    }
}
