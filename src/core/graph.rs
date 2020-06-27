type InternalId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeId(InternalId);

impl From<InternalId> for NodeId {
    fn from(index: InternalId) -> Self {
        NodeId(index)
    }
}

pub struct Graph {
    /// 3D array:
    /// * 1st dimension is the starting layer
    /// * 2nd dimension is the target layer
    /// * 3rd dimension is the component on the starting layer
    /// * and the stored value is the index of the component on the ending layer
    edge_layers: Vec<Vec<Vec<Option<InternalId>>>>,
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
    index: usize,
    content: Vec<T>,
}

impl<T> Layer<T> {
    pub fn push(&mut self, component: T) -> NodeRef<T> {
        let id = self.content.len().into();
        self.content.push(component);

        NodeRef { layer: self, id }
    }
}

pub struct NodeRef<'a, T> {
    layer: &'a Layer<T>,
    id: NodeId,
}

/// TODO: because this can be stored, it will cause big problems if deleted stuff is moved.
/// We'll worry about it when we implement deletions
pub struct WeakNodeRef<T> {
    id: NodeId,
    _marker: std::marker::PhantomData<T>,
}

impl<T> WeakNodeRef<T> {
    pub fn upgrade<'l>(&self, layer: &'l Layer<T>) -> NodeRef<'l, T> {
        NodeRef { layer, id: self.id }
    }
}

impl<'a, T> NodeRef<'a, T> {
    pub fn connect<O>(&self, other: &NodeRef<O>, graph: &mut Graph) {
        self.connect_oneway(other, graph);
        other.connect_oneway(self, graph);
    }

    pub fn connect_oneway<O>(&self, other: &NodeRef<O>, graph: &mut Graph) {
        let edge_vec = &mut graph.edge_layers[self.layer.index][other.layer.index];
        // extend the edge vec when adding an edge past its current end.
        // we don't allocate all the space at the start because it's likely to not get used
        if edge_vec.len() <= self.id.0 {
            if edge_vec.len() != self.id.0 {
                edge_vec.resize_with(self.id.0, || None);
            }
            edge_vec.push(Some(other.id.0));
        } else {
            let prev_val = edge_vec[self.id.0].replace(other.id.0);
            assert!(
                prev_val.is_none(),
                "Attempted to overwrite an edge. \
                If you're trying to do shared ownership, use `connect_oneway`."
            );
        }
    }

    pub fn get_neighbor<'o, O>(&self, other_layer: &'o Layer<O>, graph: &Graph) -> Option<&'o O> {
        let edge_layer = &graph.edge_layers[self.layer.index][other_layer.index];
        if edge_layer.len() <= self.id.0 {
            None
        } else {
            edge_layer[self.id.0].map(|other_id| &other_layer.content[other_id])
        }
    }

    pub fn downgrade(self) -> WeakNodeRef<T> {
        WeakNodeRef {
            id: self.id,
            _marker: std::marker::PhantomData,
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
            assert_eq!(rb_node.get_neighbor(&shapes, &graph), Some(&Shape(69)));
            assert_eq!(tr_node.get_neighbor(&rbs, &graph), Some(&RigidBody(i)));
            assert!(tr_node.get_neighbor(&shapes, &graph).is_none());

            // spawn something with different connections in between
            let tr_node_ = trs.push(Transform(42 + i));
            let shape_node_ = shapes.push(Shape(i));
            tr_node_.connect(&shape_node_, &mut graph);
            assert_eq!(tr_node_.get_neighbor(&shapes, &graph), Some(&Shape(i)));
        }

        eprintln!("{:?}", trs.content);
        eprintln!("{:?}", vels.content);
        eprintln!("{:?}", rbs.content);
        eprintln!("{:?}", shapes.content);
    }
}
