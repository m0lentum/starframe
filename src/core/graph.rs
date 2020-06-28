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

        NodeRef { layer: self, id }
    }
}

pub struct NodeRef<'a, T> {
    layer: &'a Layer<T>,
    id: ComponentIdx,
}

/// TODO: because this can be stored, it will cause big problems if deleted stuff is moved.
/// We'll worry about it when we implement deletions
pub struct WeakNodeRef<T> {
    id: ComponentIdx,
    _marker: PhantomData<T>,
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
        if edge_vec.len() <= self.id {
            if edge_vec.len() != self.id {
                edge_vec.resize_with(self.id, || None);
            }
            edge_vec.push(Some(other.id));
        } else {
            let prev_val = edge_vec[self.id].replace(other.id);
            assert!(
                prev_val.is_none(),
                "Attempted to overwrite an edge. \
                If you're trying to do shared ownership, use `connect_oneway`."
            );
        }
    }

    pub fn get_neighbor<'o, O>(&self, other_layer: &'o Layer<O>, graph: &Graph) -> Option<&'o O> {
        let edge_layer = &graph.edge_layers[self.layer.index][other_layer.index];
        if edge_layer.len() <= self.id {
            None
        } else {
            edge_layer[self.id].map(|other_id| &other_layer.content[other_id])
        }
    }

    pub fn downgrade(self) -> WeakNodeRef<T> {
        WeakNodeRef {
            id: self.id,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Pattern {
    layers: Vec<LayerIdx>,
    connections: Vec<(usize, usize)>, // start and end index in self.layers
}

impl Pattern {
    pub fn begin<'l, T>(root_layer: &'l Layer<T>) -> (Self, PatternMember<'l, T>) {
        let pattern = Pattern {
            layers: vec![root_layer.index],
            connections: vec![],
        };
        let root_member = PatternMember {
            layer: root_layer,
            idx_in_pattern: 0,
        };

        (pattern, root_member)
    }

    pub fn connect<'l, T>(&mut self, layer: &'l Layer<T>) -> PatternMember<'l, T> {
        let next_idx = self.layers.len();
        self.layers.push(layer.index);
        self.connections.push((0, next_idx));
        PatternMember {
            layer,
            idx_in_pattern: next_idx,
        }
    }

    pub fn connect_transitive<'l, T1, T2>(
        &mut self,
        l1: &PatternMember<'l, T1>,
        l2: &'l Layer<T2>,
    ) -> PatternMember<'l, T2> {
        let next_idx = self.layers.len();
        self.layers.push(l2.index);
        self.connections.push((l1.idx_in_pattern, next_idx));
        PatternMember {
            layer: l2,
            idx_in_pattern: next_idx,
        }
    }

    pub fn into_iter(self, graph: &Graph) -> PatternIter {
        let layer_count = self.layers.len();
        PatternIter {
            pattern: self,
            graph,
            next_root_idx: 0,
            current: PatternItem {
                indices: vec![0; layer_count],
            },
        }
    }
}

pub struct PatternMember<'a, T> {
    layer: &'a Layer<T>,
    idx_in_pattern: usize,
}

pub struct PatternIter<'g> {
    pattern: Pattern,
    graph: &'g Graph,
    next_root_idx: usize,
    // store the item here to
    // 1. avoid allocating a new Vec every iteration
    // 2. allow us to return a reference so the user isn't allowed to own these
    current: PatternItem,
}
impl<'g> PatternIter<'g> {
    pub fn next(&mut self) -> Option<&PatternItem> {
        // root index is special, we use it to traverse forward and connect to everything else
        self.current.indices[0] = self.next_root_idx;
        self.next_root_idx += 1;
        for conn in &self.pattern.connections {
            let (l0, l1) = (self.pattern.layers[conn.0], self.pattern.layers[conn.1]);
            let this_conns_edges = &self.graph.edge_layers[l0][l1];

            let start_idx = self.current.indices[conn.0];

            if conn.0 == 0 && start_idx >= this_conns_edges.len() {
                // We've reached the end of one of our _root layer's_ edge lists.
                // This means there can't be any more instances of this pattern going forward.
                return None;
            }

            // because connections are naturally sorted in rising order by starting layer
            // (the API guarantees this by only allowing to connect to the root or something that was already added),
            // we've already updated this index earlier in this loop
            match this_conns_edges.get(start_idx) {
                // None if the layer doesn't have edges allocated up to here,
                // Some(None) if it does but doesn't have this edge
                None | Some(None) => {
                    // pattern was not fulfilled, recursively traverse forward
                    return self.next();
                }
                Some(Some(end_idx)) => {
                    // pattern holds for now, check next connection
                    self.current.indices[conn.1] = *end_idx;
                }
            }
        }

        // every connection was present so we have a complete item
        Some(&self.current)
    }
}

#[derive(Debug)]
pub struct PatternItem {
    indices: Vec<ComponentIdx>,
}
impl PatternItem {
    pub fn get<'m, T>(&self, member: &'m PatternMember<'_, T>) -> &'m T {
        &member.layer.content[self.indices[member.idx_in_pattern]]
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
            assert_eq!(rb_node.get_neighbor(&shapes, &graph), Some(&Shape(69)));
            assert_eq!(tr_node.get_neighbor(&rbs, &graph), Some(&RigidBody(i)));
            assert!(tr_node.get_neighbor(&shapes, &graph).is_none());

            // spawn something with different connections in between
            let tr_node_ = trs.push(Transform(42 + i));
            let shape_node_ = shapes.push(Shape(i));
            tr_node_.connect(&shape_node_, &mut graph);
            assert_eq!(tr_node_.get_neighbor(&shapes, &graph), Some(&Shape(i)));
        }

        println!("Contents after `connect_nodes`:");
        println!("{:?}", trs.content);
        println!("{:?}", vels.content);
        println!("{:?}", rbs.content);
        println!("{:?}", shapes.content);
    }

    /// We can iterate over a layer along with edges it has towards a specific other layer.
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
            let vel_node = vels.push(Velocity(10 - i));
            let rb_node = rbs.push(RigidBody(i));
            rb_node.connect(&tr_node, &mut graph);
            if i % 2 == 0 {
                tr_node.connect(&vel_node, &mut graph);
            }
            if i % 4 == 0 {
                rb_node.connect_oneway(&everyones_shape.upgrade(&shapes), &mut graph);
            }
        }

        println!("{:?}", &graph);

        let (mut pattern, pat_trs) = Pattern::begin(&trs);
        let pat_vels = pattern.connect(&vels);
        let pat_rbs = pattern.connect(&rbs);
        let pat_shapes = pattern.connect_transitive(&pat_rbs, &shapes);

        println!("{:?}", &pattern);

        let mut iter = pattern.into_iter(&graph);
        while let Some(item) = iter.next() {
            assert_eq!(item.get(&pat_trs).0 + item.get(&pat_vels).0, 10);
            println!("{:?}", item.get(&pat_trs));
            println!("{:?}", item.get(&pat_vels));
            println!("{:?}", item.get(&pat_rbs));
            println!("{:?}", item.get(&pat_shapes));
        }
    }
}
