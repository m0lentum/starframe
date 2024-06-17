//! Intersection queries for points, rays, etc. vs. colliders.

use super::{Collider, ColliderPolygon, AABB};
use crate::math::{unit_left_normal, uv, PhysicsPose, UnitDVec2};

/// Check whether or not a point intersects with a collider.
pub fn point_collider_bool(point: uv::DVec2, pose: PhysicsPose, coll: Collider) -> bool {
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
            (x_dist <= 0.0 && y_dist <= 0.0) || x_dist * x_dist + y_dist * y_dist < r * r
        }
        // this will probably be what I do for all other polygons,
        // but keeping the match explicit so I have to look here every time and think about it
        poly @ ColliderPolygon::Triangle { .. } | poly @ ColliderPolygon::Hexagon { .. } => {
            let closest = poly.closest_boundary_point(p_wrt_c);
            closest.is_interior || (closest.pt - p_wrt_c).mag_sq() < r * r
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub start: uv::DVec2,
    pub dir: UnitDVec2,
}
impl std::ops::Mul<Ray> for PhysicsPose {
    type Output = Ray;

    fn mul(self, rhs: Ray) -> Self::Output {
        Ray {
            start: self * rhs.start,
            dir: self.rotation * rhs.dir,
        }
    }
}
impl Ray {
    /// Get the point at `(start + t * dir)`, i.e. the point reache by travelling
    /// `t` units along the ray.
    pub fn point_at_t(&self, t: f64) -> uv::DVec2 {
        self.start + t * *self.dir
    }

    /// Mirror by the x axis, i.e. flip the y values.
    pub fn mirrored_by_x(self) -> Self {
        Self {
            start: uv::DVec2::new(self.start.x, -self.start.y),
            dir: UnitDVec2::new_unchecked(uv::DVec2::new(self.dir.x, -self.dir.y)),
        }
    }

    /// Mirror by the y axis, i.e. flip the x values.
    pub fn mirrored_by_y(self) -> Self {
        Self {
            start: uv::DVec2::new(-self.start.x, self.start.y),
            dir: UnitDVec2::new_unchecked(uv::DVec2::new(-self.dir.x, self.dir.y)),
        }
    }
}

/// Find the value of t where the ray `start + t * dir` intersects with the AABB.
pub fn ray_aabb(ray: Ray, aabb: AABB) -> Option<f64> {
    let ray_to_min = aabb.min - ray.start;
    let ray_to_max = aabb.max - ray.start;

    let mut t_enter = 0.0_f64;
    let mut t_exit = f64::MAX;
    for (ray_to_min, ray_to_max, ray_speed) in [
        (ray_to_min.x, ray_to_max.x, ray.dir.x),
        (ray_to_min.y, ray_to_max.y, ray.dir.y),
    ] {
        if ray_speed.abs() < 0.00001 && ray_to_min.signum() == ray_to_max.signum() {
            // ray is parallel to this slab and starts outside of it
            return None;
        }
        let speed_inv = 1.0 / ray_speed;
        let t = [ray_to_min * speed_inv, ray_to_max * speed_inv];
        let t = if t[0] < t[1] { t } else { [t[1], t[0]] };
        t_enter = t_enter.max(t[0]);
        t_exit = t_exit.min(t[1]);
        if t_enter > t_exit {
            return None;
        }
    }
    Some(t_enter)
}

#[derive(Clone, Copy, Debug)]
pub struct CastHit {
    pub t: f64,
    pub normal: UnitDVec2,
    pub point: uv::DVec2,
}

/// Find the value of t where the sphere with radius `r` swept along the ray
/// `start + t * dir` intersects with the collider.
#[inline]
pub fn spherecast_collider(
    ray: Ray,
    r: f64,
    pose: PhysicsPose,
    mut coll: Collider,
) -> Option<CastHit> {
    coll.shape = coll.shape.expanded(r);
    ray_collider(ray, pose, coll).map(|hit| CastHit {
        point: hit.point - r * *hit.normal,
        ..hit
    })
}

/// Find the value of t where the ray `start + t * dir` intersects with the collider.
pub fn ray_collider(ray: Ray, pose: PhysicsPose, coll: Collider) -> Option<CastHit> {
    let r = coll.shape.circle_r;
    match coll.shape.polygon {
        // special cases for circles and line segments
        // because they don't have a well-formed outer polygon to clip against
        // (they aren't actually polygons, but I couldn't come up with a better name for the type)
        ColliderPolygon::Point => ray_circle(ray, pose.translation, r),
        ColliderPolygon::LineSegment { hl } => {
            let ray_worldspace = ray;
            let ray = pose.inversed() * ray;

            // special case where ray is parallel to the capsule
            if ray.dir.y.abs() < 0.0001 {
                // outside in y direction, can't possibly hit
                if ray.start.y.abs() >= r 
                    // inside, return None by convention
                    || ray.start.x.abs() < hl
                {
                    return None;
                } else {
                    return ray_circle(
                        ray,
                        uv::DVec2::new(hl.copysign(ray.start.x), 0.0),
                        coll.shape.circle_r,
                    );
                }
            }

            let facing_edge_y = coll.shape.circle_r.copysign(-ray.dir.y);
            let t_to_facing_edge = (facing_edge_y - ray.start.y) / ray.dir.y;
            // ray started inside or past the capsule
            if t_to_facing_edge < 0.0 {
                return None;
            }

            let x_at_edge_hit = ray.start.x + t_to_facing_edge * ray.dir.x;
            if x_at_edge_hit.abs() <= hl {
                // hit the flat edge
                Some(CastHit {
                    t: t_to_facing_edge,
                    normal: pose.rotation
                        * UnitDVec2::new_unchecked(uv::DVec2::new(0.0, ray.start.y.signum())),
                    point: ray_worldspace.point_at_t(t_to_facing_edge),
                })
            } else {
                // missed the flat edge, check circle cap on the side where we missed
                ray_circle(
                    ray,
                    uv::DVec2::new(hl.copysign(x_at_edge_hit), 0.0),
                    coll.shape.circle_r,
                )
            }
        }
        // this works for all actual polygons
        _ => {
            // work in object-local space
            let ray_worldspace = ray;
            let ray = pose.inversed() * ray;

            // first do a separating axis test against the perpendicular of the ray
            // to quickly check if an intersection occurs at all

            let ray_dir_perp = unit_left_normal(ray.dir);
            let ray_dist = ray.start.dot(*ray_dir_perp);
            // orient away from object center
            let (ray_dir_perp, ray_dist) = if ray_dist >= 0.0 {
                (ray_dir_perp, ray_dist)
            } else {
                (-ray_dir_perp, -ray_dist)
            };

            let poly_extent = coll.shape.polygon.projected_extent(ray_dir_perp);
            if poly_extent + coll.shape.circle_r <= ray_dist {
                return None;
            }

            // the line hits, find the point where that happens by clipping against edges
            // of the outer polygon (polygon expanded by circle_r).
            // the ray still might not hit if the point is behind its starting point
            // (TODO handle that case)

            // amount that edges extend over the circle caps before intersecting
            let outer_edge_extra_length = if coll.shape.circle_r == 0.0 {
                0.0
            } else {
                // to find the corner points of the expanded polygon we need the angles between edges
                let angle_tan = coll.shape.polygon.half_angle_between_edges_tan();
                coll.shape.circle_r / angle_tan
            };

            // if the closest edge hit was hit outside of the flat part,
            // we'll need to check against the circle at the closest vertex
            let mut vertex_for_circle_check: Option<uv::DVec2> = None;
            let mut closest_hit_t = f64::MAX;
            let mut closest_edge_normal = UnitDVec2::unit_x();
            for edge_idx in 0..coll.shape.polygon.edge_count() {
                let edge = coll.shape.polygon.get_edge(edge_idx);
                // only consider edges that point towards the ray start direction
                // (this doesn't catch if the ray starts inside the shape, that
                // needs to be handled separately)
                let edge = if edge.normal.dot(*ray.dir) <= 0.0 {
                    edge
                } else if coll.shape.polygon.is_rotationally_symmetrical() {
                    edge.mirrored()
                } else {
                    continue;
                };
                let outer_edge = edge.edge.offset(coll.shape.circle_r * *edge.normal);

                let edge_dist_from_ray = outer_edge.start - ray.start;
                let ray_speed_to_edge = ray.dir.dot(*(-edge.normal));
                if ray_speed_to_edge == 0.0 {
                    // ray is parallel to edge
                    continue;
                }
                let ray_t_to_edge = edge_dist_from_ray.dot(*(-edge.normal)) / ray_speed_to_edge;
                if ray_t_to_edge < 0.0 {
                    // edge is behind the ray start
                    continue;
                }

                let ray_speed_along_edge = ray.dir.dot(*edge.edge.dir);
                let edge_t_to_intersection =
                    ray_t_to_edge * ray_speed_along_edge - edge_dist_from_ray.dot(*edge.edge.dir);

                if edge_t_to_intersection < -outer_edge_extra_length
                    || edge_t_to_intersection > edge.edge.length + outer_edge_extra_length
                {
                    // edge was missed
                    continue;
                }
                if closest_hit_t <= ray_t_to_edge {
                    // already hit a closer edge
                    continue;
                }

                closest_hit_t = ray_t_to_edge;
                closest_edge_normal = edge.normal;
                vertex_for_circle_check = if edge_t_to_intersection < 0.0 {
                    Some(edge.edge.start)
                } else if edge_t_to_intersection > edge.edge.length {
                    Some(edge.edge.start + edge.edge.length * *edge.edge.dir)
                } else {
                    None
                };
            }

            if closest_hit_t == f64::MAX {
                None
            } else {
                match vertex_for_circle_check {
                    Some(vert) => ray_circle(ray, vert, coll.shape.circle_r),
                    None => Some(CastHit {
                        t: closest_hit_t,
                        normal: closest_edge_normal,
                        point: ray_worldspace.point_at_t(closest_hit_t),
                    }),
                }
            }
        }
    }
}

fn ray_circle(ray: Ray, circ_pos: uv::DVec2, r: f64) -> Option<CastHit> {
    // source: Real-Time Collision Detection chapter 5

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
    if t >= 0.0 {
        let point = ray.point_at_t(t);
        let normal = UnitDVec2::new_normalize(point - circ_pos);
        Some(CastHit { t, normal, point })
    } else {
        // ray started inside the circle, we consider that a miss here
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Angle;

    // This is hard to test thorougly, just a quick smoketest to make sure
    // an obvious hit hits and an obvious miss misses.
    // Other shapes' tests depend on this so other tests will confirm this works
    #[test]
    fn ray_circle_() {
        assert!(ray_circle(
            Ray {
                start: uv::DVec2::zero(),
                dir: UnitDVec2::unit_y(),
            },
            uv::DVec2::new(0.0, 2.0),
            1.0,
        )
        .is_some());
        assert!(ray_circle(
            Ray {
                start: uv::DVec2::zero(),
                dir: UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0)),
            },
            uv::DVec2::new(0.0, 2.0),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn ray_capsule() {
        // whatever pose to make sure poses are being applied
        let pose = PhysicsPose::new(uv::DVec2::new(5.0, 3.5), Angle::Deg(65.0).into());
        let cap = Collider::new_capsule(4.0, 1.0);

        let should_hit = |ray, expected_t| {
            // tranform the ray with the same pose to keep calculations easy
            let hit = ray_collider(pose * ray, pose, cap).unwrap();
            assert_t_eq(hit.t, expected_t);
        };
        let should_hit_circle = |ray, circ_pos| {
            let cap_hit = ray_collider(pose * ray, pose, cap);
            let circ_hit = ray_circle(ray, circ_pos, cap.shape.circle_r);
            match (cap_hit, circ_hit) {
                (Some(b), Some(c)) => assert_t_eq(b.t, c.t),
                (None, None) => {}
                _ => panic!("one of circle / cap missed but other didn't"),
            }
        };
        let should_miss = |ray| assert!(ray_collider(pose * ray, pose, cap).is_none());

        let mut ray = Ray {
            start: uv::DVec2::new(0.0, -2.0),
            dir: UnitDVec2::unit_y(),
        };
        should_hit(ray, 1.0);
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit(ray, 2_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.1, 1.0));
        should_hit_circle(ray, uv::DVec2::new(2.0, 0.0));
        ray.dir = UnitDVec2::unit_x();
        should_miss(ray);
        ray.start.x = -3.0;
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit_circle(ray, uv::DVec2::new(-2.0, 0.0));
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.0, 1.0));
        should_hit(ray, 5_f64.sqrt());
        ray.start.x = -2.5;
        ray.dir = UnitDVec2::unit_y();
        should_hit_circle(ray, uv::DVec2::new(-2.0, 0.0));
        ray.start = uv::DVec2::new(-500.0, 0.0);
        ray.dir = UnitDVec2::unit_x();
        should_hit_circle(ray, uv::DVec2::new(-2.0, 0.0));
        ray.start.y = 3.0;
        should_miss(ray);
    }

    #[test]
    fn ray_rect() {
        let pose = PhysicsPose::new(uv::DVec2::new(-5.0, 8.3), Angle::Deg(2.0).into());
        let rect = Collider::new_rect(4.0, 2.0);

        let should_hit = |ray, expected_t| {
            let hit = ray_collider(pose * ray, pose, rect).unwrap();
            assert_t_eq(hit.t, expected_t);
        };
        let should_miss = |ray| assert!(ray_collider(pose * ray, pose, rect).is_none());

        let mut ray = Ray {
            start: uv::DVec2::new(0.0, -2.0),
            dir: UnitDVec2::unit_y(),
        };
        should_hit(ray, 1.0);
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit(ray, 2_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.1, 1.0));
        should_miss(ray);
        ray.dir = UnitDVec2::unit_x();
        should_miss(ray);
        ray.start.x = -3.0;
        should_miss(ray);
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit(ray, 2_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.0, 1.0));
        should_hit(ray, 5_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 2.0));
        should_hit(ray, 5_f64.sqrt());
    }

    #[test]
    fn ray_rounded_rect() {
        let pose = PhysicsPose::new(uv::DVec2::new(500.0, 8.5), Angle::Deg(23.0).into());
        let rect = Collider::new_rounded_rect(6.0, 4.0, 1.0);

        let should_hit = |ray, expected_t| {
            let hit = ray_collider(pose * ray, pose, rect).unwrap();
            assert_t_eq(hit.t, expected_t);
        };
        let should_hit_circle = |ray, circ_pos| {
            let box_hit = ray_collider(pose * ray, pose, rect);
            let circ_hit = ray_circle(ray, circ_pos, rect.shape.circle_r);
            match (box_hit, circ_hit) {
                (Some(b), Some(c)) => assert_t_eq(b.t, c.t),
                (None, None) => {}
                _ => panic!("one of circle / box missed but other didn't"),
            }
        };
        let should_miss = |ray| assert!(ray_collider(pose * ray, pose, rect).is_none());

        let mut ray = Ray {
            start: uv::DVec2::new(0.0, -3.0),
            dir: UnitDVec2::unit_y(),
        };
        should_hit(ray, 1.0);
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit(ray, 2_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.1, 1.0));
        should_hit_circle(ray, uv::DVec2::new(2.0, -1.0));
        ray.dir = UnitDVec2::unit_x();
        should_miss(ray);
        ray.start.x = -4.0;
        should_miss(ray);
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 1.0));
        should_hit_circle(ray, uv::DVec2::new(-2.0, -1.0));
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(2.0, 1.0));
        should_hit(ray, 5_f64.sqrt());
        ray.dir = UnitDVec2::new_normalize(uv::DVec2::new(1.0, 2.0));
        should_hit(ray, 5_f64.sqrt());
        ray.start.x = -2.5;
        ray.dir = UnitDVec2::unit_y();
        should_hit_circle(ray, uv::DVec2::new(-2.0, -1.0));
    }

    /// Convention: ray always misses if it starts inside the collider
    #[test]
    fn inside_always_misses() {
        let pose = PhysicsPose::identity();
        for coll in [
            Collider::new_circle(1.0),
            Collider::new_capsule(2.0, 0.5),
            Collider::new_rect(2.0, 1.0),
            Collider::new_rounded_rect(2.0, 1.0, 0.25),
        ] {
            let mut ray = Ray {
                // start at origin and hope it works for all other interior points.
                // this could be more robustly tested with fuzzing but I can't be bothered
                start: uv::DVec2::zero(),
                dir: UnitDVec2::unit_x(),
            };
            let mut angle = 0.0;
            while angle < 2.0 * std::f64::consts::TAU {
                let (y, x) = angle.sin_cos();
                ray.dir = UnitDVec2::new_unchecked(uv::DVec2::new(x, y));
                let hit = ray_collider(ray, pose, coll);
                assert!(hit.is_none(), "hit shape {:?} from the inside", coll.shape);
                angle += 0.05;
            }
        }
    }

    fn assert_t_eq(t: f64, expected: f64) {
        assert!(
            (t - expected).abs() < 0.0001,
            "hit the wrong thing at t {} (expected {}",
            t,
            expected,
        );
    }
}
