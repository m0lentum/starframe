use super::collider::ColliderShape;
use crate::math::{self, uv, Unit};
use crate::physics::BodyRef;

/// An intersection between two objects.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// The normal, facing away from obj1
    pub normal: math::Unit<uv::Vec2>,
    /// Penetration depth
    pub depth: f32,
    /// Point of contact on the surface of obj1, in world space
    pub point: uv::Vec2,
    /// Offsets from each object's position to the point of contact, in object-local space
    pub offsets: [uv::Vec2; 2],
}

// Intermediate structure so we don't have to carry everything around through the worker functions
#[derive(Clone, Copy, Debug)]
struct Contact_ {
    normal: Unit<uv::Vec2>,
    depth: f32,
    point: uv::Vec2,
}

/// Checks two transformed colliders for intersection.
pub fn intersection_check(obj1: &BodyRef<'_>, obj2: &BodyRef<'_>) -> Vec<Contact> {
    let complete = |cs: Vec<Contact_>| {
        cs.iter()
            .map(|c| Contact {
                normal: c.normal,
                depth: c.depth,
                point: c.point,
                offsets: [
                    c.point - obj1.tr.item.translation,
                    c.point - obj2.tr.item.translation,
                ],
            })
            .collect()
    };

    use ColliderShape::*;
    match (obj1.coll.item.shape(), obj2.coll.item.shape()) {
        (Circle { r: r1 }, Circle { r: r2 }) => {
            complete(circle_circle(&obj1.tr.item, *r1, &obj2.tr.item, *r2))
        }
        (Rect { hw, hh }, Circle { r }) => {
            complete(rect_circle(&obj1.tr.item, *hw, *hh, &obj2.tr.item, *r))
        }
        (Circle { r }, Rect { hw, hh }) => flip_contacts(complete(rect_circle(
            &obj2.tr.item,
            *hw,
            *hh,
            &obj1.tr.item,
            *r,
        ))),
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => complete(rect_rect(
            &obj1.tr.item,
            *hw1,
            *hh1,
            &obj2.tr.item,
            *hw2,
            *hh2,
        )),
    }
}

fn flip_contacts(mut contacts: Vec<Contact>) -> Vec<Contact> {
    for c in &mut contacts {
        c.point -= c.depth * (*c.normal);
        c.normal = -c.normal;
    }
    contacts
}

fn circle_circle(tr1: &uv::Isometry2, r1: f32, tr2: &uv::Isometry2, r2: f32) -> Vec<Contact_> {
    let pos1 = tr1.translation;
    let pos2 = tr2.translation;

    let dist = pos2 - pos1;
    let dist_sq = dist.mag_sq();

    let depth;
    let normal;
    if dist_sq < 0.001 {
        // same position, consider penetration to be on x axis
        depth = r1 + r2;
        normal = Unit::unit_x();
    } else if dist_sq < (r1 + r2) * (r1 + r2) {
        // normal collision
        depth = (r1 + r2) - dist.mag();
        normal = Unit::new_normalize(dist);
    } else {
        return vec![];
    }

    vec![Contact_ {
        normal,
        depth,
        point: pos1 + (r1 * (*normal)),
    }]
}

fn rect_circle(
    tr_rect: &uv::Isometry2,
    hw: f32,
    hh: f32,
    tr_circle: &uv::Isometry2,
    r: f32,
) -> Vec<Contact_> {
    let tr_c_wrt_rect = tr_rect.inversed() * *tr_circle;
    let dist = tr_c_wrt_rect.translation;
    let dist_abs = uv::Vec2::new(dist.x.abs(), dist.y.abs());
    let dist_signums = uv::Vec2::new(dist.x.signum(), dist.y.signum());

    let c_to_corner = uv::Vec2::new(hw - dist_abs.x, hh - dist_abs.y);
    if c_to_corner.x < -r || c_to_corner.y < -r {
        // too far to possibly intersect
        return vec![];
    }
    let point_abs: uv::Vec2;
    let depth: f32;
    let normal_abs: Unit<uv::Vec2>;
    if c_to_corner.x > 0.0 && c_to_corner.y > 0.0 {
        // circle center is inside the rect
        if c_to_corner.x < c_to_corner.y {
            point_abs = uv::Vec2::new(hw, dist_abs.y);
            depth = c_to_corner.x + r;
            normal_abs = Unit::unit_x();
        } else {
            point_abs = uv::Vec2::new(dist_abs.x, hh);
            depth = c_to_corner.y + r;
            normal_abs = Unit::unit_y();
        };
    } else if c_to_corner.x > 0.0 {
        // inside in the x direction but not y
        point_abs = uv::Vec2::new(dist_abs.x, hh);
        depth = c_to_corner.y + r; // c_to_corner.y is negative
        normal_abs = Unit::unit_y();
    } else if c_to_corner.y > 0.0 {
        // inside in the y direction but not x
        point_abs = uv::Vec2::new(hw, dist_abs.y);
        depth = c_to_corner.x + r;
        normal_abs = Unit::unit_x();
    } else {
        // outside both edges, possible intersection with the corner point
        depth = r - c_to_corner.mag();
        if depth > 0.0 {
            point_abs = uv::Vec2::new(hw, hh);
            normal_abs = Unit::new_normalize(-c_to_corner);
        } else {
            return vec![];
        }
    }

    vec![Contact_ {
        normal: tr_rect.rotation
            * Unit::new_unchecked(uv::Vec2::new(
                dist_signums.x * normal_abs.x,
                dist_signums.y * normal_abs.y,
            )),
        depth,
        point: *tr_rect * uv::Vec2::new(dist_signums.x * point_abs.x, dist_signums.y * point_abs.y),
    }]
}

fn rect_rect(
    tr1: &uv::Isometry2,
    hw1: f32,
    hh1: f32,
    tr2: &uv::Isometry2,
    hw2: f32,
    hh2: f32,
) -> Vec<Contact_> {
    let tr2_wrt_tr1 = tr1.inversed() * *tr2;

    // obj1 is axis-aligned at origin, these are obj2's values
    let dist = tr2_wrt_tr1.translation;

    let x2_axis = tr2_wrt_tr1.rotation * Unit::unit_x();
    let hw2_v = hw2 * (*x2_axis);

    let y2_axis = Unit::new_unchecked(math::left_normal(*x2_axis));
    let hh2_v = hh2 * (*y2_axis);

    let axes = [Unit::unit_x(), Unit::unit_y(), x2_axis, y2_axis];

    // penetration
    let x1_pen = hw1 + hw2_v.x.abs() + hh2_v.x.abs() - dist.x.abs();
    if x1_pen <= 0.0 {
        return vec![];
    }
    let y1_pen = hh1 + hw2_v.y.abs() + hh2_v.y.abs() - dist.y.abs();
    if y1_pen <= 0.0 {
        return vec![];
    }

    let x2_pen = hw2 + x2_axis.x.abs() * hw1 + x2_axis.y.abs() * hh1 - (dist.dot(*x2_axis)).abs();
    if x2_pen <= 0.0 {
        return vec![];
    }
    let y2_pen = hh2 + y2_axis.x.abs() * hw1 + y2_axis.y.abs() * hh1 - (dist.dot(*y2_axis)).abs();
    if y2_pen <= 0.0 {
        return vec![];
    }

    let depths = [x1_pen, y1_pen, x2_pen, y2_pen];

    let ((axis_i, axis), &depth) = axes
        .iter()
        .enumerate()
        .zip(depths.iter())
        .min_by(|(_, d1), (_, d2)| {
            d1.partial_cmp(d2)
                .expect("There was a NaN in a transform somewhere")
        })
        .unwrap();

    // orient axis of penetration towards obj2
    let axis = Unit::new_unchecked(dist.dot(**axis).signum() * **axis);
    // transform normal to world space
    let normal = tr1.rotation * axis;

    let mut contacts: Vec<Contact_> = Vec::with_capacity(1);
    if axis_i <= 1 {
        // axis is on obj1, penetrating point(s) are on obj2
        let axis_dot_x2 = axis.dot(*x2_axis);
        let axis_dot_y2 = axis.dot(*y2_axis);
        let x2_axis_facing_point = Unit::new_unchecked(-axis_dot_x2.signum() * *x2_axis);
        let y2_axis_facing_point = Unit::new_unchecked(-axis_dot_y2.signum() * *y2_axis);
        // this can be outside both objects! we need to clip on both edges later
        let farthest_point = dist + (*x2_axis_facing_point * hw2) + (*y2_axis_facing_point * hh2);
        // clip incident edges to find possible second contact point
        let incident_edge = if axis_dot_x2.abs() < axis_dot_y2.abs() {
            Edge {
                start: farthest_point,
                dir: -x2_axis_facing_point,
                length: hw2 * 2.0,
            }
        } else {
            Edge {
                start: farthest_point,
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
            EdgeClipResult::Intersects => {
                contacts.push(Contact_ {
                    normal,
                    depth,
                    // point on object 1's surface
                    point: *tr1 * (farthest_point + depth * *axis),
                });
            }
            EdgeClipResult::Passes { enters, exits } => {
                let edge_dot_axis = incident_edge.dir.dot(*axis);
                let enter_depth = depth - enters * edge_dot_axis;
                contacts.push(Contact_ {
                    normal,
                    depth: enter_depth,
                    point: *tr1
                        * (farthest_point + (enters * *incident_edge.dir) + (enter_depth * *axis)),
                });
                let exit_depth = depth - exits * edge_dot_axis;
                contacts.push(Contact_ {
                    normal,
                    depth: exit_depth,
                    point: *tr1
                        * (farthest_point + (exits * *incident_edge.dir) + (exit_depth * *axis)),
                });
            }
        }
    } else {
        // copy-paste-modified from above, if there's a bug it's probably here
        // axis is on obj2, penetrating point(s) are on obj1
        let x1_axis_facing_point = Unit::new_unchecked(axis.x.signum() * uv::Vec2::unit_x());
        let y1_axis_facing_point = Unit::new_unchecked(axis.y.signum() * uv::Vec2::unit_y());
        let farthest_point = uv::Vec2::new(axis.x.signum() * hw1, axis.y.signum() * hh1);
        let incident_edge = if axis.x.abs() < axis.y.abs() {
            Edge {
                start: farthest_point,
                dir: -x1_axis_facing_point,
                length: hw1 * 2.0,
            }
        } else {
            Edge {
                start: farthest_point,
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
            EdgeClipResult::Intersects => {
                contacts.push(Contact_ {
                    normal,
                    depth,
                    point: *tr1 * farthest_point,
                });
            }
            EdgeClipResult::Passes { enters, exits } => {
                let edge_dot_axis = incident_edge.dir.dot(*axis);
                let enter_depth = depth - enters * edge_dot_axis;
                contacts.push(Contact_ {
                    normal,
                    depth: enter_depth,
                    // same as above but minus adding the depth because we're already on body 1's surface
                    point: *tr1 * (farthest_point + (enters * *incident_edge.dir)),
                });
                let exit_depth = depth - exits * edge_dot_axis;
                contacts.push(Contact_ {
                    normal,
                    depth: exit_depth,
                    point: *tr1 * (farthest_point + (exits * *incident_edge.dir)),
                });
            }
        }
    }

    contacts
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
    // this is NaN if dir1 and dir2 are parallel, but this is ok because the following
    // comparison is still true in that case and the value isn't used later
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
