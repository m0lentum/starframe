use std::marker::PhantomData;

type ComponentIdx = usize;
type LayerIdx = usize;

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
}

pub struct Layer<T> {
    index: LayerIdx,
    content: Vec<T>,
}

impl<T> Layer<T> {
    pub fn push(&mut self, component: T) -> NodeRef<T> {
        let id = self.content.len();
        self.content.push(component);

        NodeRef {
            item: &self.content[id],
            item_idx: id,
            layer_idx: self.index,
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            layer: self,
            idx: 0,
        }
    }
}

pub struct LayerIter<'a, T> {
    layer: &'a Layer<T>,
    idx: usize,
}
impl<'a, T> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.layer.content.len() {
            return None;
        }

        let item = NodeRef {
            item: &self.layer.content[self.idx],
            item_idx: self.idx,
            layer_idx: self.layer.index,
        };

        self.idx += 1;
        Some(item)
    }
}

pub struct NodeRef<'a, T> {
    item: &'a T,
    item_idx: ComponentIdx,
    layer_idx: LayerIdx,
}
impl<'a, T> std::ops::Deref for NodeRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.item
    }
}

impl<'a, T> NodeRef<'a, T> {
    pub fn connect<O>(&self, other: &NodeRef<O>, graph: &mut Graph) {
        self.connect_oneway(other, graph);
        other.connect_oneway(self, graph);
    }

    pub fn connect_oneway<O>(&self, other: &NodeRef<O>, graph: &mut Graph) {
        let edge_vec = &mut graph.edge_layers[self.layer_idx][other.layer_idx];
        // extend the edge vec when adding an edge past its current end.
        // we don't allocate all the space at the start because it's likely to not get used
        if edge_vec.len() <= self.item_idx {
            if edge_vec.len() != self.item_idx {
                edge_vec.resize_with(self.item_idx, || None);
            }
            edge_vec.push(Some(other.item_idx));
        } else {
            let prev_val = edge_vec[self.item_idx].replace(other.item_idx);
            assert!(
                prev_val.is_none(),
                "Attempted to overwrite an edge. \
                If you're trying to do shared ownership, use `connect_oneway`."
            );
        }
    }

    pub fn get_neighbor<'o, O>(
        &self,
        other_layer: &'o Layer<O>,
        graph: &Graph,
    ) -> Option<NodeRef<'o, O>> {
        let edge_layer = &graph.edge_layers[self.layer_idx][other_layer.index];
        if edge_layer.len() <= self.item_idx {
            None
        } else {
            edge_layer[self.item_idx].map(|other_id| NodeRef {
                item: &other_layer.content[other_id],
                item_idx: other_id,
                layer_idx: other_layer.index,
            })
        }
    }

    pub fn downgrade(self) -> WeakNodeRef<T> {
        WeakNodeRef {
            layer_idx: self.layer_idx,
            item_idx: self.item_idx,
            _marker: PhantomData,
        }
    }
}

/// TODO: because this can be stored, it will cause big problems if deleted stuff is moved.
/// We'll worry about it when we implement deletions
pub struct WeakNodeRef<T> {
    layer_idx: LayerIdx,
    item_idx: ComponentIdx,
    _marker: PhantomData<T>,
}

impl<T> WeakNodeRef<T> {
    pub fn upgrade<'l>(&self, layer: &'l Layer<T>) -> NodeRef<'l, T> {
        assert_eq!(
            layer.index, self.layer_idx,
            "Layer was not the one this component belongs to"
        );
        NodeRef {
            item: &layer.content[self.item_idx],
            item_idx: self.item_idx,
            layer_idx: layer.index,
        }
    }
}

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

        let everyones_shape = shapes.push(Shape(69)).downgrade();
        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let tr_node = trs.push(Transform(i));
            let vel_node = vels.push(Velocity(i));
            let rb_node = rbs.push(RigidBody(i));
            let shape_node = everyones_shape.upgrade(&shapes);
            vel_node.connect(&tr_node, &mut graph);
            rb_node.connect(&tr_node, &mut graph);
            rb_node.connect(&vel_node, &mut graph);
            rb_node.connect_oneway(&shape_node, &mut graph);
            assert_eq!(
                rb_node.get_neighbor(&shapes, &graph).map(|n| *n),
                Some(Shape(69))
            );
            assert_eq!(
                tr_node.get_neighbor(&rbs, &graph).map(|n| *n),
                Some(RigidBody(i))
            );
            assert!(tr_node.get_neighbor(&shapes, &graph).is_none());

            // spawn something with different connections in between
            let tr_node_ = trs.push(Transform(42 + i));
            let shape_node_ = shapes.push(Shape(i));
            tr_node_.connect(&shape_node_, &mut graph);
            assert_eq!(
                tr_node_.get_neighbor(&shapes, &graph).map(|n| *n),
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

        let everyones_shape = shapes.push(Shape(69)).downgrade();

        for i in 0..10 {
            let tr_node = trs.push(Transform(i));
            let vel_node = vels.push(Velocity(i));
            let rb_node = rbs.push(RigidBody(10 - i));
            rb_node.connect(&tr_node, &mut graph);
            if i % 2 == 0 {
                tr_node.connect(&vel_node, &mut graph);
            }
            if i % 4 == 0 {
                rb_node.connect_oneway(&everyones_shape.upgrade(&shapes), &mut graph);
            }
        }

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for rb in rbs.iter() {
            let tr = match rb.get_neighbor(&trs, &graph) {
                Some(tr) => tr,
                None => continue,
            };
            let vel = match tr.get_neighbor(&vels, &graph) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;

            let shape = rb.get_neighbor(&shapes, &graph);
            if shape.is_some() {
                full_match_count += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.0 % 2, 0);

            println!("{:?}, {:?}, {:?}, {:?}", *rb, *tr, *vel, shape.map(|s| *s));
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);
    }
}
