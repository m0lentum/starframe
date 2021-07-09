//! Intersection queries for points, rays, etc. vs. colliders.

use super::Collider;
use crate::math as m;

/// Check whether or not a point intersects with a collider.
pub fn point_collider_bool(point: m::Vec2, pose: &m::Pose, coll: &Collider) -> bool {
    let p_wrt_c = pose.inversed() * point;

    match coll.shape {
        super::ColliderShape::Circle { r } => p_wrt_c.mag_sq() < r * r,
        super::ColliderShape::Rect { hw, hh } => p_wrt_c.x.abs() < hw && p_wrt_c.y.abs() < hh,
    }
}
