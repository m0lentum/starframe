//! Intersection queries for points, rays, etc. vs. colliders.

use super::{Collider, ColliderShape};
use crate::math as m;

/// Check whether or not a point intersects with a collider.
pub fn point_collider_bool(point: m::Vec2, pose: &m::Pose, coll: &Collider) -> bool {
    let p_wrt_c = pose.inversed() * point;
    match coll.shape {
        ColliderShape::Circle { r } => p_wrt_c.mag_sq() < r * r,
        ColliderShape::Rect { hw, hh } => p_wrt_c.x.abs() < hw && p_wrt_c.y.abs() < hh,
        ColliderShape::Capsule { hl, r } => {
            let x_dist = (p_wrt_c.x.abs() - hl).max(0.0);
            let y_dist = p_wrt_c.y.abs();
            x_dist * x_dist + y_dist * y_dist < r * r
        }
    }
}
