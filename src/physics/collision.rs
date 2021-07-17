mod spatialindex;
pub(crate) use spatialindex::SpatialIndex;

mod collider;
pub use collider::*;

pub mod shape_shape;
pub use shape_shape::{Contact, ContactIterator, ContactResult};

pub mod query;

/// A set of bitmasks that determines which collider layers are allowed to
/// collide with each other.
///
/// There are 64 collision layers; using a value greater than that on any operation
/// will cause an out of bounds error.
pub struct MaskMatrix([u64; 64]);

/// A reserved layer for ropes. By default, all ropes are on the same layer
/// and thus do not collide with each other.
pub const ROPE_LAYER: usize = 63;

impl Default for MaskMatrix {
    fn default() -> Self {
        let mut s = Self([u64::MAX; 64]);
        s.ignore_within(ROPE_LAYER);
        s
    }
}

impl MaskMatrix {
    /// Stop collision detection between a pair of collision layers.
    pub fn ignore(&mut self, layer1: usize, layer2: usize) {
        self.0[layer1] &= !(1 << layer2);
        self.0[layer2] &= !(1 << layer1);
    }

    /// Stop collision detection between members of the same layer.
    pub fn ignore_within(&mut self, layer: usize) {
        self.0[layer] &= !(1 << layer);
    }

    /// Check whether or not two layers have collision enabled between them.
    pub fn get(&self, layer1: usize, layer2: usize) -> bool {
        self.0[layer1] & (1 << layer2) != 0
    }
}
