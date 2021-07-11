use super::collider::ColliderShape;
use crate::math::{self as m, Pose, Unit};
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
    pub normal: m::Unit<m::Vec2>,
    /// Points of contact on the surface of each object, in object-local space.
    pub offsets: [m::Vec2; 2],
}

/// Checks two colliders for intersection.
pub fn intersection_check(
    pose1: &Pose,
    coll1: &Collider,
    pose2: &Pose,
    coll2: &Collider,
) -> ContactResult {
    use ColliderShape::*;
    match (coll1.shape, coll2.shape) {
        (Circle { r: r1 }, Circle { r: r2 }) => circle_circle(pose1, r1, pose2, r2),
        (Circle { r }, Rect { hw, hh }) => flip_contacts(rect_circle(pose2, hw, hh, pose1, r)),
        (Rect { hw, hh }, Circle { r }) => rect_circle(pose1, hw, hh, pose2, r),
        (Circle { r: rcirc }, Capsule { hl, r: rcap }) => {
            circle_capsule(pose1, rcirc, pose2, hl, rcap)
        }
        (Capsule { hl, r: rcap }, Circle { r: rcirc }) => {
            flip_contacts(circle_capsule(pose2, rcirc, pose1, hl, rcap))
        }
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            rect_rect(pose1, hw1, hh1, pose2, hw2, hh2)
        }
        (Rect { hw, hh }, Capsule { hl, r }) => rect_capsule(pose1, hw, hh, pose2, hl, r),
        (Capsule { hl, r }, Rect { hw, hh }) => {
            flip_contacts(rect_capsule(pose2, hw, hh, pose1, hl, r))
        }
        (Capsule { hl: hl1, r: r1 }, Capsule { hl: hl2, r: r2 }) => {
            capsule_capsule(pose1, hl1, r1, pose2, hl2, r2)
        }
    }
}

fn flip_contacts(contacts: ContactResult) -> ContactResult {
    contacts.map(|c| Contact {
        normal: -c.normal,
        offsets: [c.offsets[1], c.offsets[0]],
    })
}

//
// CIRCLE <-> CIRCLE
//

fn circle_circle(pose1: &m::Pose, r1: f64, pose2: &m::Pose, r2: f64) -> ContactResult {
    let pos1 = pose1.translation;
    let pos2 = pose2.translation;

    let dist = pos2 - pos1;
    let dist_sq = dist.mag_sq();
    let r_sum = r1 + r2;

    let normal = if dist_sq < 0.001 {
        // same position, consider penetration to be on x axis
        Unit::unit_x()
    } else if dist_sq < r_sum * r_sum {
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

//
// RECT <-> CIRCLE
//

fn rect_circle(
    pose_rect: &m::Pose,
    hw: f64,
    hh: f64,
    pose_circle: &m::Pose,
    r: f64,
) -> ContactResult {
    let pose_c_wrt_rect = pose_rect.inversed() * *pose_circle;
    let dist = pose_c_wrt_rect.translation;
    let dist_abs = m::Vec2::new(dist.x.abs(), dist.y.abs());
    let dist_signums = m::Vec2::new(dist.x.signum(), dist.y.signum());

    let c_to_corner = m::Vec2::new(hw - dist_abs.x, hh - dist_abs.y);
    if c_to_corner.x < -r || c_to_corner.y < -r {
        // too far to possibly intersect
        return ContactResult::Zero;
    }
    let point_abs: m::Vec2;
    let normal_abs: Unit<m::Vec2>;
    if c_to_corner.x > 0.0 && c_to_corner.y > 0.0 {
        // circle center is inside the rect
        if c_to_corner.x < c_to_corner.y {
            point_abs = m::Vec2::new(hw, dist_abs.y);
            normal_abs = Unit::unit_x();
        } else {
            point_abs = m::Vec2::new(dist_abs.x, hh);
            normal_abs = Unit::unit_y();
        };
    } else if c_to_corner.x > 0.0 {
        // inside in the x direction but not y
        point_abs = m::Vec2::new(dist_abs.x, hh);
        normal_abs = Unit::unit_y();
    } else if c_to_corner.y > 0.0 {
        // inside in the y direction but not x
        point_abs = m::Vec2::new(hw, dist_abs.y);
        normal_abs = Unit::unit_x();
    } else {
        // outside both edges, possible intersection with the corner point
        let depth = r - c_to_corner.mag();
        if depth > 0.0 {
            point_abs = m::Vec2::new(hw, hh);
            normal_abs = Unit::new_normalize(-c_to_corner);
        } else {
            return ContactResult::Zero;
        }
    }

    let normal_wrt_rect = Unit::new_unchecked(m::Vec2::new(
        dist_signums.x * normal_abs.x,
        dist_signums.y * normal_abs.y,
    ));

    ContactResult::One(Contact {
        normal: pose_rect.rotation * normal_wrt_rect,
        offsets: [
            m::Vec2::new(dist_signums.x * point_abs.x, dist_signums.y * point_abs.y),
            pose_c_wrt_rect.rotation.reversed() * (-r * *normal_wrt_rect),
        ],
    })
}

//
// CIRCLE <-> CAPSULE
//

fn circle_capsule(
    pose_circ: &m::Pose,
    r_circ: f64,
    pose_cap: &m::Pose,
    hl: f64,
    r_cap: f64,
) -> ContactResult {
    let pose_circ_wrt_cap = pose_cap.inversed() * *pose_circ;
    let center_dist = pose_circ_wrt_cap.translation;

    let dist = m::Vec2::new(
        // x distance is 0 if the circle is along the line segment defining the capsule
        (center_dist.x.abs() - hl).max(0.0) * center_dist.x.signum(),
        center_dist.y,
    );
    let dist_sq = dist.mag_sq();
    let r_sum = r_circ + r_cap;

    let normal = if dist_sq < 0.001 {
        Unit::unit_y()
    } else if dist_sq < r_sum * r_sum {
        // normal must be away from the circle, dist is from cap to circle
        Unit::new_normalize(-dist)
    } else {
        return ContactResult::Zero;
    };

    let depth = r_sum - dist_sq.sqrt();

    ContactResult::One(Contact {
        normal: pose_cap.rotation * normal,
        offsets: [
            pose_circ_wrt_cap.rotation.reversed() * (r_circ * *normal),
            center_dist + (r_circ - depth) * *normal,
        ],
    })
}

//
// RECT <-> RECT
//

fn rect_rect(
    pose1: &m::Pose,
    hw1: f64,
    hh1: f64,
    pose2: &m::Pose,
    hw2: f64,
    hh2: f64,
) -> ContactResult {
    let pose2_wrt_pose1 = pose1.inversed() * *pose2;

    // obj1 is axis-aligned at origin, these are obj2's values
    let dist = pose2_wrt_pose1.translation;

    let x2_axis = pose2_wrt_pose1.rotation * Unit::unit_x();
    let hw2_v = hw2 * (*x2_axis);

    let y2_axis = Unit::new_unchecked(m::left_normal(*x2_axis));
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
                start: m::Vec2::new(-hw1, axis.y.signum() * hh1),
                dir: Unit::unit_x(),
                length: hw1 * 2.0,
            }
        } else {
            Edge {
                start: m::Vec2::new(axis.x.signum() * hw1, -hh1),
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
                // due to floating point inaccuracy there's a possible case
                // where we get to this point but the edge clip misses.
                // just bail if that happens
                if enter_depth <= 0.0 || exit_depth <= 0.0 {
                    return ContactResult::Zero;
                }

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
            EdgeClipResult::Misses => ContactResult::Zero,
        }
    } else {
        // copy-paste-modified from above, if there's a bug it's probably here
        // axis is on obj2, penetrating point(s) are on obj1
        let x1_axis_facing_point = Unit::new_unchecked(axis.x.signum() * m::Vec2::unit_x());
        let y1_axis_facing_point = Unit::new_unchecked(axis.y.signum() * m::Vec2::unit_y());
        let extreme_point_on_obj1 = m::Vec2::new(axis.x.signum() * hw1, axis.y.signum() * hh1);
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
                let enter_depth = depth + enters * edge_dot_axis;
                let exit_depth = depth + exits * edge_dot_axis;
                if enter_depth <= 0.0 || exit_depth <= 0.0 {
                    return ContactResult::Zero;
                }

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
            EdgeClipResult::Misses => ContactResult::Zero,
        }
    }
}

//
// RECT <-> CAPSULE
//

fn rect_capsule(
    pose_rect: &m::Pose,
    hw: f64,
    hh: f64,
    pose_cap: &m::Pose,
    hl: f64,
    r: f64,
) -> ContactResult {
    let pose_cap_wrt_rect = pose_rect.inversed() * *pose_cap;

    // four possible separating axes:
    // rect's principal axes, axis normal to the capsule's line segment,
    // and axis between the closest cap end point and the closest rect corner

    let dist = pose_cap_wrt_rect.translation;
    let cap_dir = pose_cap_wrt_rect.rotation * m::Vec2::unit_x();
    // orient normal away from rect
    let cap_normal = m::left_normal(cap_dir);
    let cap_normal = if cap_normal.dot(dist) < 0.0 {
        -cap_normal
    } else {
        cap_normal
    };

    let pen_rect_x = (hw + cap_dir.x.abs() * hl + r) - dist.x.abs();
    if pen_rect_x <= 0.0 {
        return ContactResult::Zero;
    }
    let pen_rect_y = (hh + cap_dir.y.abs() * hl + r) - dist.y.abs();
    if pen_rect_y <= 0.0 {
        return ContactResult::Zero;
    }
    let pen_cap_normal =
        (cap_normal.x.abs() * hw + cap_normal.y.abs() * hh + r) - dist.dot(cap_normal);
    if pen_cap_normal <= 0.0 {
        return ContactResult::Zero;
    }
    let cap_ends = [dist + hl * cap_dir, dist - hl * cap_dir];
    let cap_end_dists = [
        cap_ends[0].abs() - m::Vec2::new(hw, hh),
        cap_ends[1].abs() - m::Vec2::new(hw, hh),
    ];
    let closer_cap_end = if cap_end_dists[0].mag_sq() <= cap_end_dists[1].mag_sq() {
        cap_ends[0]
    } else {
        cap_ends[1]
    };
    let (closest_rect_corner, axis_cap_end, pen_cap_end) =
        if closer_cap_end.x.abs() <= hw || closer_cap_end.y.abs() <= hh {
            // cap end is only relevant if it's in the voronoi region outside both rect faces.
            // we aren't, so set axis to whatever and depth to max so it doesn't get selected
            (
                m::Vec2::zero(),
                Unit::new_unchecked(m::Vec2::unit_x()),
                std::f64::MAX,
            )
        } else {
            let closest_rect_corner = m::Vec2::new(
                hw * closer_cap_end.x.signum(),
                hh * closer_cap_end.y.signum(),
            );
            let rect_corner_to_cap_end = closer_cap_end - closest_rect_corner;
            let axis = Unit::new_normalize(rect_corner_to_cap_end);
            let axis = if axis.dot(dist) < 0.0 { -axis } else { axis };
            let pen = (axis.abs().dot(m::Vec2::new(hw, hh)) + axis.dot(cap_dir).abs() * hl + r)
                - axis.dot(dist);
            if pen <= 0.0 {
                return ContactResult::Zero;
            }
            (closest_rect_corner, axis, pen)
        };

    let (lowest_pen_axis, _) = [pen_rect_x, pen_rect_y, pen_cap_normal, pen_cap_end]
        .iter()
        .enumerate()
        .min_by(|(_, d1), (_, d2)| d1.partial_cmp(d2).expect("There was a NaN somewhere"))
        .unwrap();

    match lowest_pen_axis {
        // rect x axis
        0 => {
            let normal = Unit::new_unchecked(m::Vec2::new(dist.x.signum(), 0.0));
            let normal_worldspace = pose_rect.rotation * normal;
            // update cap_dir to match the axis direction
            let cap_dir = if normal.dot(cap_dir) < 0.0 {
                -cap_dir
            } else {
                cap_dir
            };
            // we might have two contact points along the straight edge of the capsule
            let rect_edge = Edge {
                start: m::Vec2::new(dist.x.signum() * hw, -hh),
                dir: Unit::new_unchecked(m::Vec2::unit_y()),
                length: hh * 2.0,
            };
            let cap_edge = Edge {
                start: dist - cap_normal * r - cap_dir * hl,
                dir: Unit::new_unchecked(cap_dir),
                length: hl * 2.0,
            };
            match clip_edge(rect_edge, cap_edge) {
                EdgeClipResult::Intersects | EdgeClipResult::Misses => {
                    // contact point is on the circle at the closer end of the capsule
                    let point_on_cap = dist - cap_dir * hl - r * *normal;
                    ContactResult::One(Contact {
                        normal: normal_worldspace,
                        offsets: [
                            point_on_cap + m::Vec2::new(pen_rect_x * dist.x.signum(), 0.0),
                            pose_cap_wrt_rect.inversed() * point_on_cap,
                        ],
                    })
                }
                EdgeClipResult::Passes { enters, exits } => {
                    let edge_dot_axis = cap_edge.dir.x * dist.x.signum();
                    let start_depth = hw - cap_edge.start.x.abs();
                    let enter_depth = start_depth - enters * edge_dot_axis;
                    let exit_depth = start_depth - exits * edge_dot_axis;
                    if enter_depth <= 0.0 || exit_depth <= 0.0 {
                        // flat edge missed, so the point is on the circular part
                        let point_on_cap = dist - cap_dir * hl - r * *normal;
                        return ContactResult::One(Contact {
                            normal: normal_worldspace,
                            offsets: [
                                point_on_cap + m::Vec2::new(pen_rect_x * dist.x.signum(), 0.0),
                                pose_cap_wrt_rect.inversed() * point_on_cap,
                            ],
                        });
                    }

                    let enter_point = cap_edge.start + (enters * *cap_edge.dir);
                    let exit_point = cap_edge.start + (exits * *cap_edge.dir);
                    let pc_wrt_pr_inv = pose_cap_wrt_rect.inversed();
                    ContactResult::Two(
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                enter_point + m::Vec2::new(dist.x.signum() * enter_depth, 0.0),
                                pc_wrt_pr_inv * enter_point,
                            ],
                        },
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                exit_point + m::Vec2::new(dist.x.signum() * exit_depth, 0.0),
                                pc_wrt_pr_inv * exit_point,
                            ],
                        },
                    )
                }
            }
        }
        // rect y axis
        1 => {
            let normal = Unit::new_unchecked(m::Vec2::new(0.0, dist.y.signum()));
            let normal_worldspace = pose_rect.rotation * normal;
            // update cap_dir to match the axis direction
            let cap_dir = if normal.dot(cap_dir) < 0.0 {
                -cap_dir
            } else {
                cap_dir
            };
            let rect_edge = Edge {
                start: m::Vec2::new(-hw, dist.y.signum() * hh),
                dir: Unit::new_unchecked(m::Vec2::unit_x()),
                length: hw * 2.0,
            };
            let cap_edge = Edge {
                start: dist - cap_normal * r - cap_dir * hl,
                dir: Unit::new_unchecked(cap_dir),
                length: hl * 2.0,
            };
            match clip_edge(rect_edge, cap_edge) {
                EdgeClipResult::Intersects | EdgeClipResult::Misses => {
                    // contact point is on the circle at the closer end of the capsule
                    let point_on_cap = dist - cap_dir * hl - r * *normal;
                    ContactResult::One(Contact {
                        normal: normal_worldspace,
                        offsets: [
                            point_on_cap + m::Vec2::new(0.0, pen_rect_y * dist.y.signum()),
                            pose_cap_wrt_rect.inversed() * point_on_cap,
                        ],
                    })
                }
                EdgeClipResult::Passes { enters, exits } => {
                    let edge_dot_axis = cap_edge.dir.y * dist.y.signum();
                    let start_depth = hh - cap_edge.start.y.abs();
                    let enter_depth = start_depth - enters * edge_dot_axis;
                    let exit_depth = start_depth - exits * edge_dot_axis;
                    if enter_depth <= 0.0 || exit_depth <= 0.0 {
                        // flat edge missed, so the point is on the circular part
                        let point_on_cap = dist - cap_dir * hl - r * *normal;
                        return ContactResult::One(Contact {
                            normal: normal_worldspace,
                            offsets: [
                                point_on_cap + m::Vec2::new(0.0, pen_rect_y * dist.y.signum()),
                                pose_cap_wrt_rect.inversed() * point_on_cap,
                            ],
                        });
                    }

                    let enter_point = cap_edge.start + (enters * *cap_edge.dir);
                    let exit_point = cap_edge.start + (exits * *cap_edge.dir);
                    let pc_wrt_pr_inv = pose_cap_wrt_rect.inversed();
                    ContactResult::Two(
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                enter_point + m::Vec2::new(0.0, dist.y.signum() * enter_depth),
                                pc_wrt_pr_inv * enter_point,
                            ],
                        },
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                exit_point + m::Vec2::new(0.0, dist.y.signum() * exit_depth),
                                pc_wrt_pr_inv * exit_point,
                            ],
                        },
                    )
                }
            }
        }
        // capsule normal direction
        2 => {
            let normal_worldspace = pose_rect.rotation * Unit::new_unchecked(cap_normal);
            let rect_edge = if cap_normal.x.abs() > cap_normal.y.abs() {
                Edge {
                    start: m::Vec2::new(-hw, cap_normal.y.signum() * hh),
                    dir: Unit::new_unchecked(m::Vec2::unit_x()),
                    length: hw * 2.0,
                }
            } else {
                Edge {
                    start: m::Vec2::new(cap_normal.x.signum() * hw, -hh),
                    dir: Unit::new_unchecked(m::Vec2::unit_y()),
                    length: hh * 2.0,
                }
            };
            let cap_edge = Edge {
                start: dist - cap_normal * r - cap_dir * hl,
                dir: Unit::new_unchecked(cap_dir),
                length: hl * 2.0,
            };
            match clip_edge(cap_edge, rect_edge) {
                EdgeClipResult::Misses => ContactResult::Zero,
                EdgeClipResult::Intersects => {
                    // contact point is at the tip of the rect
                    let point_on_rect =
                        m::Vec2::new(cap_normal.x.signum() * hw, cap_normal.y.signum() * hh);
                    ContactResult::One(Contact {
                        normal: normal_worldspace,
                        offsets: [
                            point_on_rect,
                            pose_cap_wrt_rect.inversed()
                                * (point_on_rect - cap_normal * pen_cap_normal),
                        ],
                    })
                }
                EdgeClipResult::Passes { enters, exits } => {
                    let edge_dot_axis = rect_edge.dir.dot(cap_normal);
                    let enter_depth = pen_cap_normal - enters * edge_dot_axis;
                    let exit_depth = pen_cap_normal - exits * edge_dot_axis;
                    if enter_depth <= 0.0 || exit_depth <= 0.0 {
                        return ContactResult::Zero;
                    }

                    let enter_point = rect_edge.start + (enters * *rect_edge.dir);
                    let exit_point = rect_edge.start + (exits * *rect_edge.dir);
                    let pc_wrt_pr_inv = pose_cap_wrt_rect.inversed();
                    ContactResult::Two(
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                enter_point,
                                pc_wrt_pr_inv * (enter_point - cap_normal * enter_depth),
                            ],
                        },
                        Contact {
                            normal: normal_worldspace,
                            offsets: [
                                exit_point,
                                pc_wrt_pr_inv * (exit_point - cap_normal * exit_depth),
                            ],
                        },
                    )
                }
            }
        }
        // capsule closest corner
        3 => ContactResult::One(Contact {
            normal: pose_rect.rotation * axis_cap_end,
            offsets: [
                closest_rect_corner,
                pose_cap_wrt_rect.inversed() * (closest_rect_corner - pen_cap_end * *axis_cap_end),
            ],
        }),
        _ => unreachable!(),
    }
}

//
// CAPSULE <-> CAPSULE
//

fn capsule_capsule(
    pose1: &m::Pose,
    hl1: f64,
    r1: f64,
    pose2: &m::Pose,
    hl2: f64,
    r2: f64,
) -> ContactResult {
    let pose2_wrt_pose1 = pose1.inversed() * *pose2;

    let dist = pose2_wrt_pose1.translation;
    let cap2_dir = pose2_wrt_pose1.rotation * m::Vec2::unit_x();

    // get the closest points on the line segments defining the capsules
    let closest_points = if cap2_dir.y == 0.0 {
        // the capsules are perfectly collinear.
        // pick one end of cap2 and the closest point to it on cap1
        let closer_cap2_end = if cap2_dir.x.signum() == dist.x.signum() {
            dist - cap2_dir * hl2
        } else {
            dist + cap2_dir * hl2
        };
        [
            m::Vec2::new(closer_cap2_end.x.max(-hl1).min(hl1), 0.0),
            closer_cap2_end,
        ]
    } else {
        // intersection of the whole lines from cap 2's POV,
        // clamped to the extents of the line segment
        let t2 = (-dist.y / cap2_dir.y).min(hl2).max(-hl2);
        // closest point on cap 1 to point t2, clamped
        let t1 = (dist.x + t2 * cap2_dir.x).min(hl1).max(-hl1);
        // set t2 to closest point to clamped t1, clamping again to make sure we stay
        // within cap 2's limits
        let t2 = m::Vec2::new(t1 - dist.x, -dist.y)
            .dot(cap2_dir)
            .min(hl2)
            .max(-hl2);
        [m::Vec2::new(t1, 0.0), dist + t2 * cap2_dir]
    };
    // knowing exact depth isn't necessary here, just whether it's over 0,
    // so we save some cycles with mag_sq
    let pen = (r1 + r2).powi(2) - (closest_points[0] - closest_points[1]).mag_sq();
    if pen <= 0.0 {
        return ContactResult::Zero;
    }

    // check closest straight edges for intersection
    let cap1_edge = Edge {
        start: m::Vec2::new(-hl1, dist.y.signum() * r1),
        dir: Unit::new_unchecked(m::Vec2::unit_x()),
        length: hl1 * 2.0,
    };
    let cap2_normal = m::left_normal(cap2_dir);
    let cap2_normal = if cap2_normal.dot(dist) < 0.0 {
        -cap2_normal
    } else {
        cap2_normal
    };
    let cap2_edge = Edge {
        start: dist - hl2 * cap2_dir - r2 * cap2_normal,
        dir: Unit::new_unchecked(cap2_dir),
        length: hl2 * 2.0,
    };
    match clip_edge(cap1_edge, cap2_edge) {
        EdgeClipResult::Intersects | EdgeClipResult::Misses => {
            let normal = Unit::new_normalize(closest_points[1] - closest_points[0]);
            ContactResult::One(Contact {
                normal: pose1.rotation * normal,
                offsets: [
                    closest_points[0] + r1 * *normal,
                    pose2_wrt_pose1.inversed() * (closest_points[1] - r2 * *normal),
                ],
            })
        }
        EdgeClipResult::Passes { enters, exits } => {
            let enter_depth = r1 - (cap2_edge.start.y + enters * cap2_edge.dir.y).abs();
            let exit_depth = r1 - (cap2_edge.start.y + exits * cap2_edge.dir.y).abs();
            if enter_depth <= 0.0 || exit_depth <= 0.0 {
                let normal = Unit::new_normalize(closest_points[1] - closest_points[0]);
                ContactResult::One(Contact {
                    normal: pose1.rotation * normal,
                    offsets: [
                        closest_points[0] + r1 * *normal,
                        pose2_wrt_pose1.inversed() * (closest_points[1] - r2 * *normal),
                    ],
                })
            } else {
                let normal = Unit::new_unchecked(m::Vec2::new(0.0, dist.y.signum()));
                let normal_worldspace = pose1.rotation * normal;
                ContactResult::Two(
                    Contact {
                        normal: normal_worldspace,
                        offsets: [
                            m::Vec2::new(
                                cap2_edge.start.x + enters * cap2_edge.dir.x,
                                dist.y.signum() * r1,
                            ),
                            pose2_wrt_pose1.inversed()
                                * (cap2_edge.start + enters * *cap2_edge.dir),
                        ],
                    },
                    Contact {
                        normal: normal_worldspace,
                        offsets: [
                            m::Vec2::new(
                                cap2_edge.start.x + exits * cap2_edge.dir.x,
                                dist.y.signum() * r1,
                            ),
                            pose2_wrt_pose1.inversed() * (cap2_edge.start + exits * *cap2_edge.dir),
                        ],
                    },
                )
            }
        }
    }
}

//
// EDGE CLIP
//

#[derive(Clone, Copy, Debug)]
struct Edge {
    start: m::Vec2,
    dir: Unit<m::Vec2>,
    length: f64,
}

#[derive(Clone, Copy, Debug)]
enum EdgeClipResult {
    /// If edges intersect, we don't care about anything else
    /// as this means a single contact point at an already known location
    Intersects,
    /// If they don't intersect, we want the distances at which edge 1 intersects
    /// with the lines perpendicular to edge 2 going through edge 2's endpoints.
    Passes { enters: f64, exits: f64 },
    /// If edge 1 is completely outside the slab defined by edge 1, this is returned.
    Misses,
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
        if (start_clip_t <= 0.0 && end_clip_t <= 0.0)
            || (start_clip_t >= edge.length && end_clip_t >= edge.length)
        {
            return EdgeClipResult::Misses;
        }
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
    use std::f64::consts::PI;

    #[test]
    fn clip_various_edges() {
        // intersection
        match clip_edge(
            Edge {
                start: m::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: m::Vec2::new(1.0, 0.0),
                dir: Unit::new_normalize(m::Vec2::new(1.0, 1.0)),
                length: 2.0,
            },
        ) {
            EdgeClipResult::Intersects => (),
            _ => panic!("Didn't intersect"),
        }
        // miss that starts at 0
        match clip_edge(
            Edge {
                start: m::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: m::Vec2::new(2.0, 0.0),
                dir: m::Rotor2::from_angle(PI / 6.0) * Unit::unit_x(),
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
                start: m::Vec2::new(1.0, 1.0),
                dir: Unit::unit_x(),
                length: 2.0,
            },
            Edge {
                start: m::Vec2::new(4.0, 0.0),
                dir: m::Rotor2::from_angle(7.0 * PI / 8.0) * Unit::unit_x(),
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
