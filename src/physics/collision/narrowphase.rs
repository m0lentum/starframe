use super::collider::ColliderShape;
use crate::math::{self, uv, Pose, Unit};
use crate::physics::Collider;

/// 0-2 points of contact can occur between two 2D objects.
#[derive(Clone, Copy, Debug)]
pub enum ContactResult {
    Zero,
    One(Contact),
    Two(Contact, Contact),
}

impl ContactResult {
    pub fn iter(&self) -> ContactIterator<'_> {
        ContactIterator { cr: self, idx: 0 }
    }

    /// Execute a function on every contact in the result.
    pub fn map(self, f: impl Fn(Contact) -> Contact) -> Self {
        match self {
            ContactResult::Zero => ContactResult::Zero,
            ContactResult::One(c) => ContactResult::One(f(c)),
            ContactResult::Two(c1, c2) => ContactResult::Two(f(c1), f(c2)),
        }
    }
}

/// An iterator over the contacts in a ContactResult.
pub struct ContactIterator<'a> {
    cr: &'a ContactResult,
    idx: u8,
}
impl<'a> Iterator for ContactIterator<'a> {
    type Item = &'a Contact;

    fn next(&mut self) -> Option<Self::Item> {
        self.idx += 1;
        use ContactResult::*;
        match (self.cr, self.idx - 1) {
            (Zero, _) => None,
            (One(c), 0) => Some(c),
            (One(_), _) => None,
            (Two(c1, _), 0) => Some(c1),
            (Two(_, c2), 1) => Some(c2),
            (Two(_, _), _) => None,
        }
    }
}

/// An intersection between two objects.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// The normal, facing away from obj1
    pub normal: math::Unit<uv::Vec2>,
    /// Points of contact on the surface of each object, in object-local space.
    pub offsets: [uv::Vec2; 2],
}

/// Checks two colliders for intersection.
pub fn intersection_check(
    pose1: &Pose,
    coll1: &Collider,
    pose2: &Pose,
    coll2: &Collider,
) -> ContactResult {
    use ColliderShape::*;
    match (coll1.shape(), coll2.shape()) {
        (Circle { r: r1 }, Circle { r: r2 }) => circle_circle(pose1, *r1, pose2, *r2),
        (Rect { hw, hh }, Circle { r }) => rect_circle(pose1, *hw, *hh, pose2, *r),
        (Circle { r }, Rect { hw, hh }) => flip_contacts(rect_circle(pose2, *hw, *hh, pose1, *r)),
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            rect_rect(pose1, *hw1, *hh1, pose2, *hw2, *hh2)
        }
    }
}

fn flip_contacts(contacts: ContactResult) -> ContactResult {
    contacts.map(|c| Contact {
        normal: -c.normal,
        offsets: [c.offsets[1], c.offsets[0]],
    })
}

fn circle_circle(pose1: &uv::Isometry2, r1: f32, pose2: &uv::Isometry2, r2: f32) -> ContactResult {
    let pos1 = pose1.translation;
    let pos2 = pose2.translation;

    let dist = pos2 - pos1;
    let dist_sq = dist.mag_sq();

    let normal = if dist_sq < 0.001 {
        // same position, consider penetration to be on x axis
        Unit::unit_x()
    } else if dist_sq < (r1 + r2) * (r1 + r2) {
        // typical collision
        Unit::new_normalize(dist)
    } else {
        return ContactResult::Zero;
    };

    ContactResult::One(Contact {
        normal,
        offsets: [
            pose1.rotation.reversed() * (r1 * *normal),
            pose2.rotation.reversed() * (-r2 * *normal),
        ],
    })
}

fn rect_circle(
    pose_rect: &uv::Isometry2,
    hw: f32,
    hh: f32,
    pose_circle: &uv::Isometry2,
    r: f32,
) -> ContactResult {
    let pose_c_wrt_rect = pose_rect.inversed() * *pose_circle;
    let dist = pose_c_wrt_rect.translation;
    let dist_abs = uv::Vec2::new(dist.x.abs(), dist.y.abs());
    let dist_signums = uv::Vec2::new(dist.x.signum(), dist.y.signum());

    let c_to_corner = uv::Vec2::new(hw - dist_abs.x, hh - dist_abs.y);
    if c_to_corner.x < -r || c_to_corner.y < -r {
        // too far to possibly intersect
        return ContactResult::Zero;
    }
    let point_abs: uv::Vec2;
    let normal_abs: Unit<uv::Vec2>;
    if c_to_corner.x > 0.0 && c_to_corner.y > 0.0 {
        // circle center is inside the rect
        if c_to_corner.x < c_to_corner.y {
            point_abs = uv::Vec2::new(hw, dist_abs.y);
            normal_abs = Unit::unit_x();
        } else {
            point_abs = uv::Vec2::new(dist_abs.x, hh);
            normal_abs = Unit::unit_y();
        };
    } else if c_to_corner.x > 0.0 {
        // inside in the x direction but not y
        point_abs = uv::Vec2::new(dist_abs.x, hh);
        normal_abs = Unit::unit_y();
    } else if c_to_corner.y > 0.0 {
        // inside in the y direction but not x
        point_abs = uv::Vec2::new(hw, dist_abs.y);
        normal_abs = Unit::unit_x();
    } else {
        // outside both edges, possible intersection with the corner point
        let depth = r - c_to_corner.mag();
        if depth > 0.0 {
            point_abs = uv::Vec2::new(hw, hh);
            normal_abs = Unit::new_normalize(-c_to_corner);
        } else {
            return ContactResult::Zero;
        }
    }

    let normal_wrt_rect = Unit::new_unchecked(uv::Vec2::new(
        dist_signums.x * normal_abs.x,
        dist_signums.y * normal_abs.y,
    ));

    ContactResult::One(Contact {
        normal: pose_rect.rotation * normal_wrt_rect,
        offsets: [
            uv::Vec2::new(dist_signums.x * point_abs.x, dist_signums.y * point_abs.y),
            pose_c_wrt_rect.rotation.reversed() * (-r * *normal_wrt_rect),
        ],
    })
}

fn rect_rect(
    pose1: &uv::Isometry2,
    hw1: f32,
    hh1: f32,
    pose2: &uv::Isometry2,
    hw2: f32,
    hh2: f32,
) -> ContactResult {
    let pose2_wrt_pose1 = pose1.inversed() * *pose2;

    // obj1 is axis-aligned at origin, these are obj2's values
    let dist = pose2_wrt_pose1.translation;

    let x2_axis = pose2_wrt_pose1.rotation * Unit::unit_x();
    let hw2_v = hw2 * (*x2_axis);

    let y2_axis = Unit::new_unchecked(math::left_normal(*x2_axis));
    let hh2_v = hh2 * (*y2_axis);

    let axes = [Unit::unit_x(), Unit::unit_y(), x2_axis, y2_axis];

    // penetration
    let x1_pen = hw1 + hw2_v.x.abs() + hh2_v.x.abs() - dist.x.abs();
    if x1_pen <= 0.0 {
        return ContactResult::Zero;
    }
    let y1_pen = hh1 + hw2_v.y.abs() + hh2_v.y.abs() - dist.y.abs();
    if y1_pen <= 0.0 {
        return ContactResult::Zero;
    }

    let x2_pen = hw2 + x2_axis.x.abs() * hw1 + x2_axis.y.abs() * hh1 - (dist.dot(*x2_axis)).abs();
    if x2_pen <= 0.0 {
        return ContactResult::Zero;
    }
    let y2_pen = hh2 + y2_axis.x.abs() * hw1 + y2_axis.y.abs() * hh1 - (dist.dot(*y2_axis)).abs();
    if y2_pen <= 0.0 {
        return ContactResult::Zero;
    }

    let depths = [x1_pen, y1_pen, x2_pen, y2_pen];

    let ((axis_i, axis), &depth) = axes
        .iter()
        .enumerate()
        .zip(depths.iter())
        .min_by(|(_, d1), (_, d2)| d1.partial_cmp(d2).expect("There was a NaN somewhere"))
        .unwrap();

    // orient axis of penetration towards obj2
    let axis = Unit::new_unchecked(dist.dot(**axis).signum() * **axis);
    // transform normal to world space
    let normal = pose1.rotation * axis;

    if axis_i <= 1 {
        // axis is on obj1, penetrating point(s) are on obj2
        let axis_dot_x2 = axis.dot(*x2_axis);
        let axis_dot_y2 = axis.dot(*y2_axis);
        let x2_axis_facing_point = Unit::new_unchecked(-axis_dot_x2.signum() * *x2_axis);
        let y2_axis_facing_point = Unit::new_unchecked(-axis_dot_y2.signum() * *y2_axis);
        let extreme_point_on_obj2 =
            dist + (*x2_axis_facing_point * hw2) + (*y2_axis_facing_point * hh2);
        // clip incident edges to find possible second contact point
        let incident_edge = if axis_dot_x2.abs() < axis_dot_y2.abs() {
            Edge {
                start: extreme_point_on_obj2,
                dir: -x2_axis_facing_point,
                length: hw2 * 2.0,
            }
        } else {
            Edge {
                start: extreme_point_on_obj2,
                dir: -y2_axis_facing_point,
                length: hh2 * 2.0,
            }
        };
        let owning_edge = if axis_i == 1 {
            Edge {
                start: uv::Vec2::new(-hw1, axis.y.signum() * hh1),
                dir: Unit::unit_x(),
                length: hw1 * 2.0,
            }
        } else {
            Edge {
                start: uv::Vec2::new(axis.x.signum() * hw1, -hh1),
                dir: Unit::unit_y(),
                length: hh1 * 2.0,
            }
        };
        match clip_edge(owning_edge, incident_edge) {
            EdgeClipResult::Intersects => ContactResult::One(Contact {
                normal,
                offsets: [
                    incident_edge.start + depth * *axis,
                    pose2_wrt_pose1.inversed() * incident_edge.start,
                ],
            }),
            EdgeClipResult::Passes { enters, exits } => {
                let edge_dot_axis = incident_edge.dir.dot(*axis);
                let enter_depth = depth - enters * edge_dot_axis;
                let exit_depth = depth - exits * edge_dot_axis;

                let enter_point = incident_edge.start + (enters * *incident_edge.dir);
                let exit_point = incident_edge.start + (exits * *incident_edge.dir);
                let p2_wrt_p1_inv = pose2_wrt_pose1.inversed();
                ContactResult::Two(
                    Contact {
                        normal,
                        offsets: [
                            enter_point + (enter_depth * *axis),
                            p2_wrt_p1_inv * enter_point,
                        ],
                    },
                    Contact {
                        normal,
                        offsets: [
                            exit_point + (exit_depth * *axis),
                            p2_wrt_p1_inv * exit_point,
                        ],
                    },
                )
            }
        }
    } else {
        // copy-paste-modified from above, if there's a bug it's probably here
        // axis is on obj2, penetrating point(s) are on obj1
        let x1_axis_facing_point = Unit::new_unchecked(axis.x.signum() * uv::Vec2::unit_x());
        let y1_axis_facing_point = Unit::new_unchecked(axis.y.signum() * uv::Vec2::unit_y());
        let extreme_point_on_obj1 = uv::Vec2::new(axis.x.signum() * hw1, axis.y.signum() * hh1);
        let incident_edge = if axis.x.abs() < axis.y.abs() {
            Edge {
                start: extreme_point_on_obj1,
                dir: -x1_axis_facing_point,
                length: hw1 * 2.0,
            }
        } else {
            Edge {
                start: extreme_point_on_obj1,
                dir: -y1_axis_facing_point,
                length: hh1 * 2.0,
            }
        };
        let owning_edge = if axis_i == 3 {
            Edge {
                // remember axis is oriented towards body 2
                start: dist - *axis * hh2 - *axes[2] * hw2,
                dir: axes[2],
                length: hw2 * 2.0,
            }
        } else {
            Edge {
                start: dist - *axis * hw2 - *axes[3] * hh2,
                dir: axes[3],
                length: hh2 * 2.0,
            }
        };
        match clip_edge(owning_edge, incident_edge) {
            EdgeClipResult::Intersects => ContactResult::One(Contact {
                normal,
                offsets: [
                    incident_edge.start,
                    pose2_wrt_pose1.inversed() * (incident_edge.start - depth * *axis),
                ],
            }),
            EdgeClipResult::Passes { enters, exits } => {
                let edge_dot_axis = incident_edge.dir.dot(*axis);
                let enter_depth = depth - enters * edge_dot_axis;
                let exit_depth = depth - exits * edge_dot_axis;

                let enter_point = incident_edge.start + (enters * *incident_edge.dir);
                let exit_point = incident_edge.start + (exits * *incident_edge.dir);
                let p2_wrt_p1_inv = pose2_wrt_pose1.inversed();
                ContactResult::Two(
                    Contact {
                        normal,
                        offsets: [
                            enter_point,
                            p2_wrt_p1_inv * (enter_point - enter_depth * *axis),
                        ],
                    },
                    Contact {
                        normal,
                        offsets: [
                            exit_point,
                            p2_wrt_p1_inv * (exit_point - exit_depth * *axis),
                        ],
                    },
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    start: uv::Vec2,
    dir: Unit<uv::Vec2>,
    length: f32,
}

enum EdgeClipResult {
    /// If edges intersect, we don't care about anything else
    /// as this means a single contact point at an already known location
    Intersects,
    /// If they don't intersect, we want the distances at which edge 1 intersects
    /// with the lines perpendicular to edge 2 going through edge 2's endpoints.
    Passes { enters: f32, exits: f32 },
}

fn clip_edge(target: Edge, edge: Edge) -> EdgeClipResult {
    let start_dist = target.start - edge.start;
    // cramer's rule solution for t in At = b
    // where A = [dir1, -dir2] and b = start_dist.
    // denom is 0 and t is NaN if dir1 and dir2 are parallel, but this is ok because the following
    // comparison correctly evaluates to false and t isn't used after that.
    let denom = edge.dir.x * (-target.dir.y) - (-target.dir.x) * edge.dir.y;
    let t = [
        (start_dist.x * (-target.dir.y) - (-target.dir.x) * start_dist.y) / denom,
        (edge.dir.x * start_dist.y - start_dist.x * edge.dir.y) / denom,
    ];
    if t[0] >= 0.0 && t[0] <= edge.length && t[1] >= 0.0 && t[1] <= target.length {
        EdgeClipResult::Intersects
    } else {
        let dist_dot_dir2 = start_dist.dot(*target.dir);
        let dirs_dot = edge.dir.dot(*target.dir);
        let start_clip_t = dist_dot_dir2 / dirs_dot;
        let end_clip_t = (target.length + dist_dot_dir2) / dirs_dot;
        let (enters, exits) = if start_clip_t < end_clip_t {
            (start_clip_t.max(0.0), end_clip_t.min(edge.length))
        } else {
            (end_clip_t.max(0.0), start_clip_t.min(edge.length))
        };
        EdgeClipResult::Passes { enters, exits }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn clip_various_edges() {
        // intersection
        match clip_edge(
            Edge {
                start: uv::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: uv::Vec2::new(1.0, 0.0),
                dir: Unit::new_normalize(uv::Vec2::new(1.0, 1.0)),
                length: 2.0,
            },
        ) {
            EdgeClipResult::Intersects => (),
            _ => panic!("Didn't intersect"),
        }
        // miss that starts at 0
        match clip_edge(
            Edge {
                start: uv::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: uv::Vec2::new(2.0, 0.0),
                dir: uv::Rotor2::from_angle(PI / 6.0) * Unit::unit_x(),
                length: 2.0,
            },
        ) {
            EdgeClipResult::Passes { enters, exits } => {
                assert_eq!(enters, 0.0);
                assert!((exits - 1.0 / (PI / 6.0).cos()).abs() < 0.001);
            }
            _ => panic!("Intersected but shouldn't have"),
        }
        // miss that starts before 0 but ends at length
        // and also starts at the end of the other one
        match clip_edge(
            Edge {
                start: uv::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: uv::Vec2::new(4.0, 0.0),
                dir: uv::Rotor2::from_angle(7.0 * PI / 8.0) * Unit::unit_x(),
                length: 2.0,
            },
        ) {
            EdgeClipResult::Passes { enters, exits } => {
                assert!((enters - 1.0 / (PI / 8.0).cos()).abs() < 0.001);
                assert_eq!(exits, 2.0);
            }
            _ => panic!("Intersected but shouldn't have"),
        }
    }
}
