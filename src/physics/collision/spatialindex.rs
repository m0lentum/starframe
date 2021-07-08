use crate::{
    graph::{Graph, Layer, Node, NodeRef},
    physics::Collider,
};

/// The spatial index is responsible for detecting pairs of possibly
/// intersecting objects for further, more accurate narrow phase inspection.
///
/// For now, this is just brute force pairing every collider with every other collider.
pub struct SpatialIndex {
    // TODO
}

impl SpatialIndex {
    /// Return all collider pairs that *might* intersect according to the spatial structure.
    pub fn pairs(l_collider: &Layer<Collider>, graph: &Graph) -> Vec<[Node<Collider>; 2]> {
        let mut pairs = Vec::new();
        let mut iter = l_collider
            .iter(graph)
            .map(|cref| NodeRef::as_node(&cref, graph));
        while let Some(c0) = iter.next() {
            let rest_iter = iter.clone();
            for c1 in rest_iter {
                pairs.push([c0, c1]);
            }
        }
        pairs
    }
}
