pub(crate) mod hgrid;
pub use hgrid::{HGrid, HGridParams};

mod collider;
pub use collider::*;

mod compound_collider;
pub use compound_collider::CompoundColliderSetup;

pub mod shape_shape;
pub use shape_shape::{Contact, ContactIterator, ContactResult};

pub mod query;
pub use query::Ray;

//

use crate::math as m;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub struct AABB {
    pub min: m::Vec2,
    pub max: m::Vec2,
}

impl AABB {
    /// Move the AABB by the given vector without changing its size.
    pub fn translated(self, translation: m::Vec2) -> Self {
        Self {
            min: self.min + translation,
            max: self.max + translation,
        }
    }

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

    /// Create an AABB with both extents at the origin. Useful as a filler default in cache.
    pub(crate) fn zero() -> Self {
        AABB {
            min: m::Vec2::zero(),
            max: m::Vec2::zero(),
        }
    }
}

/// A set of bitmasks that determines which collider layers are allowed to
/// collide with each other.
///
/// There are 64 collision layers; using a value greater than that on any operation
/// will cause an out of bounds error.
#[derive(Clone, Copy, Debug)]
pub struct MaskMatrix([u64; 64]);

/// A mask determining which collision layers are considered in a query.
#[derive(Clone, Copy, Debug)]
pub struct LayerMask(pub u64);
impl Default for LayerMask {
    fn default() -> Self {
        Self(!0)
    }
}
impl LayerMask {
    /// Check whether the given layer is enabled in this mask.
    pub fn get(&self, other_layer: usize) -> bool {
        self.0 & (1 << other_layer) != 0
    }
}

/// A reserved layer for ropes. By default, all ropes are on the same layer
/// and do not collide with each other.
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
    #[inline]
    pub fn ignore(&mut self, layer1: usize, layer2: usize) {
        self.0[layer1] &= !(1 << layer2);
        self.0[layer2] &= !(1 << layer1);
    }

    /// Stop collision detection between members of the same layer.
    #[inline]
    pub fn ignore_within(&mut self, layer: usize) {
        self.0[layer] &= !(1 << layer);
    }

    /// Ignore all collisions involving members of this layer.
    #[inline]
    pub fn ignore_all(&mut self, layer: usize) {
        for other in 0..self.0.len() {
            self.ignore(layer, other);
        }
    }

    /// Re-enable collision detection between a pair of layers.
    #[inline]
    pub fn unignore(&mut self, layer1: usize, layer2: usize) {
        self.0[layer1] |= 1 << layer2;
        self.0[layer2] |= 1 << layer1;
    }

    /// Check whether or not two layers have collision enabled between them.
    #[inline]
    pub fn get(&self, layer1: usize, layer2: usize) -> bool {
        self.0[layer1] & (1 << layer2) != 0
    }

    /// Get the mask for a single layer.
    #[inline]
    pub fn get_mask(&self, layer: usize) -> LayerMask {
        LayerMask(self.0[layer])
    }
}
