mod hgrid;
pub use hgrid::{HGrid, HGridParams};

mod collider;
pub use collider::*;

pub mod shape_shape;
pub use shape_shape::{Contact, ContactIterator, ContactResult};

pub mod query;

//

use crate::math as m;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug)]
pub struct AABB {
    pub min: m::Vec2,
    pub max: m::Vec2,
}

impl AABB {
    /// Increase the size of the AABB by the same amount in all directions.
    pub fn padded(mut self, amount: f64) -> Self {
        self.min.x -= amount;
        self.min.y -= amount;
        self.max.x += amount;
        self.max.y += amount;
        self
    }

    /// Increase the size of the AABB in the direction of a vector.
    pub fn extended(mut self, amount: m::Vec2) -> Self {
        if amount.x < 0.0 {
            self.min.x += amount.x;
        } else {
            self.max.x += amount.x;
        }

        if amount.y < 0.0 {
            self.min.y += amount.y;
        } else {
            self.max.y += amount.y;
        }

        self
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// The smallest box containing both given boxes.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: self.min.min_by_component(other.min),
            max: self.max.max_by_component(other.max),
        }
    }

    /// The area that is inside both boxes.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let min = self.min.max_by_component(other.min);
        let max = self.max.min_by_component(other.max);
        if min.x >= max.x || min.y >= max.y {
            None
        } else {
            Some(Self { min, max })
        }
    }

    pub fn contains_point(&self, p: m::Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

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
