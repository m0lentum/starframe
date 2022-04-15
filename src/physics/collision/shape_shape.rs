use super::collider::{ColliderPolygon, ColliderShape};
use crate::math::{self as m, Pose, Unit};

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

    pub fn is_zero(&self) -> bool {
        matches!(self, ContactResult::Zero)
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
    /// The normal, facing away from the first object
    pub normal: m::Unit<m::Vec2>,
    /// Points of contact on the surface of each object, in object-local space.
    pub offsets: [m::Vec2; 2],
}

/// Checks two colliders for intersection.
pub fn intersection_check(poses: [Pose; 2], shapes: [ColliderShape; 2]) -> ContactResult {
    let r0 = shapes[0].circle_r;
    let r1 = shapes[1].circle_r;
    type P = ColliderPolygon;
    match [shapes[0].polygon, shapes[1].polygon] {
        [P::Point, P::Point] => circle_circle(poses[0], r0, poses[1], r1),
        [P::Point, _] => circle_any(poses[0], r0, poses[1], shapes[1], r1),
        [_, P::Point] => flip_contacts(circle_any(poses[1], r1, poses[0], shapes[0], r0)),
        _ => any_any(poses, shapes),
    }
}

#[inline]
fn flip_contacts(contacts: ContactResult) -> ContactResult {
    contacts.map(|c| Contact {
        normal: -c.normal,
        offsets: [c.offsets[1], c.offsets[0]],
    })
}

//
// simplified special cases for circles
//

fn circle_circle(pose1: m::Pose, r1: f64, pose2: m::Pose, r2: f64) -> ContactResult {
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

fn circle_any(
    pose_circ: Pose,
    r_circ: f64,
    pose_other: Pose,
    shape_other: ColliderShape,
    r_other: f64,
) -> ContactResult {
    // working in the local space of the other shape
    let pose_circ_local = pose_other.inversed() * pose_circ;
    let dist = pose_circ_local.translation;

    let closest_pt = shape_other.polygon.closest_boundary_point(dist);
    let dist_from_closest = dist - closest_pt.pt;
    if closest_pt.is_interior {
        let dir_from_closest = Unit::new_normalize(dist_from_closest);
        ContactResult::One(Contact {
            // remember: normal away from the circle!
            // in this case, dir_from_closest points inward to the other shape
            normal: pose_other.rotation * dir_from_closest,
            offsets: [
                pose_circ_local.rotation.reversed() * (r_circ * *dir_from_closest),
                closest_pt.pt - r_other * *dir_from_closest,
            ],
        })
    } else {
        let r_sum = r_circ + r_other;
        let dist_sq = dist_from_closest.mag_sq();
        if dist_sq >= r_sum * r_sum {
            ContactResult::Zero
        } else {
            let dir_from_closest = Unit::new_unchecked(dist_from_closest / dist_sq.sqrt());
            ContactResult::One(Contact {
                normal: pose_other.rotation * (-dir_from_closest),
                offsets: [
                    pose_circ_local.rotation.reversed() * (r_circ * *(-dir_from_closest)),
                    closest_pt.pt + r_other * *dir_from_closest,
                ],
            })
        }
    }
}

//
// generic test for all other shape pairs
//

fn any_any(poses: [Pose; 2], shapes: [ColliderShape; 2]) -> ContactResult {
    let po2_wrt_po1 = poses[0].inversed() * poses[1];
    let relative_poses = [po2_wrt_po1.inversed(), po2_wrt_po1];

    // first, do a separating axis test along all the polygon axes.
    // this doesn't guarantee a collision, but gives us an early out
    // and access to the closest pair of polygon edges

    let mut pen_depth = f64::MAX;
    struct EdgeAndExtent(PolygonEdge, f64);
    let mut pen_axis: Option<EdgeAndExtent> = None;
    // shape owning the current axis comes first
    let mut shape_order = [0, 1];
    for (edge_idx, s_order) in itertools::chain(
        (0..shapes[0].polygon.edge_count()).map(|i| (i, [0, 1])),
        (0..shapes[1].polygon.edge_count()).map(|i| (i, [1, 0])),
    ) {
        let dist = relative_poses[s_order[1]].translation;
        let edge = shapes[s_order[0]].polygon.get_edge(edge_idx);
        // check that the axis points towards the other object
        let edge = if edge.normal.dot(dist) >= 0.0 {
            edge
        } else if shapes[s_order[0]].polygon.is_rotationally_symmetrical() {
            edge.mirrored()
        } else {
            // we can skip axes that point away and don't have an associated symmetry
            continue;
        };
        let edge_extent = shapes[s_order[0]].polygon.get_edge_extent(edge_idx);

        // transform axis such that it's in the other object's local space
        // and points towards the first
        let axis_wrt_other = -(relative_poses[s_order[0]].rotation * edge.normal);
        let depth = edge_extent
            + shapes[0].circle_r
            + shapes[s_order[1]].polygon.projected_extent(axis_wrt_other)
            + shapes[1].circle_r
            - dist.dot(*edge.normal);

        if depth <= 0.0 {
            return ContactResult::Zero;
        }
        if depth < pen_depth {
            pen_depth = depth;
            pen_axis = Some(EdgeAndExtent(edge, edge_extent));
            shape_order = s_order;
        }
    }

    // the only way we can have None here is if we called this for a circle-circle pair,
    // which has a more efficient special case available so shouldn't be used
    let EdgeAndExtent(pen_edge, pen_edge_extent) =
        pen_axis.expect("Don't use generic test for circle-circle pairs");

    // flip returned result if the axis of penetration is on the second shape
    let orient_result = if shape_order[0] == 0 {
        |r: ContactResult| r
    } else {
        flip_contacts
    };

    // first check for a two-point contact by clipping the closest two straight edges

    // clip done on edges offset to the outer edge of the sum shape
    let owning_edge = if shapes[shape_order[0]].circle_r == 0.0 {
        pen_edge.edge
    } else {
        let offset = *pen_edge.normal * shapes[shape_order[0]].circle_r;
        pen_edge.edge.offset(offset)
    };
    // rotated to second shape's local space and flipped towards the first
    let pen_axis_wrt_snd = -(relative_poses[shape_order[0]].rotation * pen_edge.normal);

    let incident_edge_inner_local = shapes[shape_order[1]]
        .polygon
        .supporting_edge(*pen_axis_wrt_snd);
    // if neither shape has a circle component, we can possibly early out after this
    let both_simple_polygons =
        shapes[shape_order[0]].circle_r == 0.0 && shapes[shape_order[1]].circle_r == 0.0;

    // edge on the polygon part, without the circle part, in the first object's local space
    let incident_edge_inner = incident_edge_inner_local.transformed(relative_poses[shape_order[1]]);
    // edge on the actual shape, with the circle part applied
    let incident_edge_outer = if shapes[shape_order[1]].circle_r == 0.0 {
        incident_edge_inner.edge
    } else {
        let offset = *incident_edge_inner.normal * shapes[shape_order[1]].circle_r;
        incident_edge_inner.edge.offset(offset)
    };

    match clip_edge(owning_edge, incident_edge_outer) {
        // check if the edge passes on the "inside" of the other,
        // if so this is a two-point contact
        EdgeClipResult::Passes { enters, exits } => {
            let start_depth = pen_edge_extent + shapes[shape_order[0]].circle_r
                - incident_edge_outer.start.dot(*pen_edge.normal);
            let dir_dot_axis = incident_edge_outer.dir.dot(*pen_edge.normal);

            let enter_depth = start_depth - enters * dir_dot_axis;
            let exit_depth = start_depth - exits * dir_dot_axis;

            if enter_depth > 0.0 && exit_depth > 0.0 {
                let enter_point = incident_edge_outer.start + (enters * *incident_edge_outer.dir);
                let exit_point = incident_edge_outer.start + (exits * *incident_edge_outer.dir);

                let normal_worldspace = poses[shape_order[0]].rotation * pen_edge.normal;

                return orient_result(ContactResult::Two(
                    Contact {
                        normal: normal_worldspace,
                        offsets: [
                            enter_point + enter_depth * *pen_edge.normal,
                            relative_poses[shape_order[0]] * enter_point,
                        ],
                    },
                    Contact {
                        normal: normal_worldspace,
                        offsets: [
                            exit_point + exit_depth * *pen_edge.normal,
                            relative_poses[shape_order[0]] * exit_point,
                        ],
                    },
                ));
            } else if both_simple_polygons {
                return ContactResult::Zero;
            }
        }
        EdgeClipResult::Misses if both_simple_polygons => {
            return ContactResult::Zero;
        }
        // no two-point collision, a single-point is still possible.
        // we'll check for that next
        _ => {}
    }

    let closest_point_on_other = incident_edge_inner.edge.start;

    // possibly need one more check for distance between closest points

    let edge_start_to_closest = closest_point_on_other - pen_edge.edge.start;
    let t_to_closest_projected = edge_start_to_closest.dot(*pen_edge.edge.dir);
    let t_clamped = if t_to_closest_projected < 0.0 {
        0.0
    } else if t_to_closest_projected > pen_edge.edge.length {
        pen_edge.edge.length
    } else {
        // closest point is along the edge;
        // this is a collision along the axis we've already computed
        // and we don't have to do a point-point comparison
        return orient_result(ContactResult::One(Contact {
            normal: poses[shape_order[0]].rotation * pen_edge.normal,
            offsets: [
                pen_edge.edge.start
                    + t_to_closest_projected * *pen_edge.edge.dir
                    + shapes[shape_order[0]].circle_r * *pen_edge.normal,
                relative_poses[shape_order[0]]
                    * (closest_point_on_other - shapes[shape_order[1]].circle_r * *pen_edge.normal),
            ],
        }));
    };
    // closest point is at the end of the edge, meaning this is a circle-corner collision (or miss)
    let closest_on_pen_edge = pen_edge.edge.start + t_clamped * *pen_edge.edge.dir;

    let dist_btw_closest_points = closest_point_on_other - closest_on_pen_edge;
    let dist_sq = dist_btw_closest_points.mag_sq();
    if dist_sq >= (shapes[0].circle_r + shapes[1].circle_r).powi(2) {
        // closest points were far apart.
        // this is a missed collision on a circular corner
        return ContactResult::Zero;
    }

    // there was a collision on a circular corner
    let axis = m::Unit::new_unchecked(dist_btw_closest_points / dist_sq.sqrt());
    let axis_worldspace = poses[shape_order[0]].rotation * axis;

    orient_result(ContactResult::One(Contact {
        normal: axis_worldspace,
        offsets: [
            closest_on_pen_edge + shapes[shape_order[0]].circle_r * *axis,
            relative_poses[shape_order[0]]
                * (closest_point_on_other - shapes[shape_order[1]].circle_r * *axis),
        ],
    }))
}

//
// utility types & operations
//

/// Closest point to a query point on the boundary of a polygon,
/// plus whether the query point is inside the polygon or not.
#[derive(Clone, Copy, Debug)]
pub(super) struct ClosestBoundaryPoint {
    pub pt: m::Vec2,
    pub is_interior: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PolygonEdge {
    pub normal: m::Unit<m::Vec2>,
    pub edge: Edge,
}
impl PolygonEdge {
    /// Transform the edge by a Pose.
    pub fn transformed(self, pose: m::Pose) -> Self {
        Self {
            edge: self.edge.transformed(pose),
            normal: pose.rotation * self.normal,
        }
    }

    /// Mirror the edge with respect to the point at the origin.
    #[inline]
    pub fn mirrored(self) -> Self {
        Self {
            normal: -self.normal,
            edge: self.edge.mirrored(),
        }
    }
}

//
// edge clip
//

#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub start: m::Vec2,
    pub dir: Unit<m::Vec2>,
    pub length: f64,
}
impl Edge {
    #[inline]
    pub fn transformed(self, pose: Pose) -> Self {
        Self {
            start: pose * self.start,
            dir: pose.rotation * self.dir,
            length: self.length,
        }
    }

    #[inline]
    pub fn mirrored(self) -> Self {
        Self {
            start: -self.start,
            dir: -self.dir,
            length: self.length,
        }
    }

    #[inline]
    pub fn offset(self, amount: m::Vec2) -> Self {
        Self {
            start: self.start + amount,
            dir: self.dir,
            length: self.length,
        }
    }

    // currently only used in tests
    #[allow(dead_code)]
    #[inline]
    pub fn flipped(self) -> Self {
        Self {
            start: self.end_point(),
            dir: -self.dir,
            length: self.length,
        }
    }

    #[allow(dead_code)]
    #[inline]
    pub fn end_point(&self) -> m::Vec2 {
        self.start + self.length * *self.dir
    }
}

#[derive(Clone, Copy, Debug)]
enum EdgeClipResult {
    /// If edges intersect, we don't care about anything else
    /// as this means a single contact point at an already known location
    Intersects,
    /// If they don't intersect, we want the distances at which edge 1 intersects
    /// with the lines perpendicular to edge 2 going through edge 2's endpoints.
    ///
    /// The values are the `t`s at which the *second* edge passed to `clip_edge`
    /// crosses the boundaries defined by the *first* edge.
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

    // cases here where exactly the bound value is expected
    #[allow(clippy::float_cmp)]
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
