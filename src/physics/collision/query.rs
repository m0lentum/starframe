//! Intersection queries for points, rays, etc. vs. colliders.

use super::{Collider, ColliderPolygon};
use crate::math as m;

/// Check whether or not a point intersects with a collider.
pub fn point_collider_bool(point: m::Vec2, pose: &m::Pose, coll: &Collider) -> bool {
    let r = coll.shape.circle_r;
    let p_wrt_c = pose.inversed() * point;
    match coll.shape.polygon {
        ColliderPolygon::Point => p_wrt_c.mag_sq() < r * r,
        ColliderPolygon::LineSegment { hl } => {
            let x_dist = (p_wrt_c.x.abs() - hl).max(0.0);
            let y_dist = p_wrt_c.y.abs();
            x_dist * x_dist + y_dist * y_dist < r * r
        }
        ColliderPolygon::Rect { hw, hh } => {
            let x_dist = p_wrt_c.x.abs() - hw;
            let y_dist = p_wrt_c.y.abs() - hh;
            x_dist <= 0.0 || y_dist <= 0.0 || x_dist * x_dist + y_dist * y_dist < r * r
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub start: m::Vec2,
    pub dir: m::Unit<m::Vec2>,
}
impl std::ops::Mul<Ray> for m::Pose {
    type Output = Ray;

    fn mul(self, rhs: Ray) -> Self::Output {
        Ray {
            start: self * rhs.start,
            dir: self.rotation * rhs.dir,
        }
    }
}
impl Ray {
    /// Get the point `(start + t * dir)` along the ray.
    pub fn point_at_t(&self, t: f64) -> m::Vec2 {
        self.start + t * *self.dir
    }

    /// Mirror by the x axis, i.e. flip the y values.
    pub fn mirrored_by_x(self) -> Self {
        Self {
            start: m::Vec2::new(self.start.x, -self.start.y),
            dir: m::Unit::new_unchecked(m::Vec2::new(self.dir.x, -self.dir.y)),
        }
    }

    /// Mirror by the y axis, i.e. flip the x values.
    pub fn mirrored_by_y(self) -> Self {
        Self {
            start: m::Vec2::new(-self.start.x, self.start.y),
            dir: m::Unit::new_unchecked(m::Vec2::new(-self.dir.x, self.dir.y)),
        }
    }
}

/// Find the value of t where the ray start + t * dir intersects with the collider.
pub fn ray_collider(ray: Ray, pose: &m::Pose, coll: &Collider) -> Option<f64> {
    let r = coll.shape.circle_r;
    match coll.shape.polygon {
        // source for circle and rect: Real-Time Collision Detection chapter 5
        ColliderPolygon::Point => ray_circle(ray, pose.translation, r),
        ColliderPolygon::LineSegment { hl } => {
            // work in object-local space to make use of symmetry
            let ray = pose.inversed() * ray;

            // first check against the rect part of the capsule
            if ray.dir.y.abs() < 0.00001 && ray.start.y.abs() > r {
                return None;
            }
            let yspeed_inv = 1.0 / ray.dir.y;
            let t = [
                (-r - ray.start.y) * yspeed_inv,
                (r - ray.start.y) * yspeed_inv,
            ];
            let t = if t[0] < t[1] { t } else { [t[1], t[0]] };
            if t[1] < 0.0 {
                // starts outside, going away
                return None;
            }
            let t_slab_hit = t[0].max(0.0);
            let x_slab_hit = ray.start.x + t_slab_hit * ray.dir.x;
            if x_slab_hit.abs() <= hl {
                // straight edge hit or started inside
                return Some(t_slab_hit);
            }

            // check against the closer circle cap
            let ray = if ray.start.x.is_sign_positive() {
                ray
            } else {
                ray.mirrored_by_y()
            };
            ray_circle(ray, m::Vec2::new(hl, 0.0), r)
        }
        // BIG TODO: account for the circle component
        ColliderPolygon::Rect { hw, hh } => {
            // work in object-local space to treat box as AABB
            let ray = pose.inversed() * ray;

            let mut t_min = 0.0_f64;
            let mut t_max = f64::MAX;
            for (ray_start, ray_speed, rect_dim) in
                [(ray.start.x, ray.dir.x, hw), (ray.start.y, ray.dir.y, hh)]
            {
                if ray_speed.abs() < 0.00001 && ray_start.abs() > rect_dim {
                    // ray is parallel to this slab and starts outside of it
                    return None;
                }
                let speed_inv = 1.0 / ray_speed;
                let t = [
                    (-rect_dim - ray_start) * speed_inv,
                    (rect_dim - ray_start) * speed_inv,
                ];
                let t = if t[0] < t[1] { t } else { [t[1], t[0]] };
                t_min = t_min.max(t[0]);
                t_max = t_max.min(t[1]);
                if t_min > t_max {
                    return None;
                }
            }
            Some(t_min)
        }
    }
}

fn ray_circle(ray: Ray, circ_pos: m::Vec2, r: f64) -> Option<f64> {
    // solve t from t^2 + 2(m*d)t + (m*m)-r^2 = 0
    // where m is ray start relative to circle and d its direction
    let ray_start_wrt_circ = ray.start - circ_pos;
    let b = ray_start_wrt_circ.dot(*ray.dir);
    let c = ray_start_wrt_circ.mag_sq() - r * r;
    if b > 0.0 && c > 0.0 {
        return None;
    }
    let discr = b * b - c;
    if discr < 0.0 {
        return None;
    }
    let t = -b - discr.sqrt();
    Some(t.max(0.0))
}
