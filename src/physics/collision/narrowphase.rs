use super::collider::ColliderShape;
use crate::core::math as m;
use crate::physics::BodyRef;

use nalgebra as na;
use std::f32::consts::PI;

/// determines how close to parallel two surfaces need to be to generate two contacts
const FLAT_COLLISION_ANGLE_THRESHOLD: f32 = 0.005;

/// An intersection between two objects.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// The normal, facing away from obj1
    pub normal: na::Unit<m::Vec2>,
    /// Penetration depth
    pub depth: f32,
    /// Point of contact on the surface of obj1, in world space
    pub point: m::Point2,
    /// Offsets from each object's position to the point of contact, in object-local space
    pub offsets: [m::Vec2; 2],
}

// Intermediate structure so we don't have to carry everything around through the worker functions
struct Contact_ {
    normal: na::Unit<m::Vec2>,
    depth: f32,
    point: m::Point2,
}

/// Checks two transformed colliders for intersection.
pub fn intersection_check<'a>(obj1: BodyRef<'a>, obj2: BodyRef<'a>) -> Vec<Contact> {
    let complete = |cs: Vec<Contact_>| {
        cs.iter()
            .map(|c| Contact {
                normal: c.normal,
                depth: c.depth,
                point: c.point,
                offsets: [
                    c.point.coords - obj1.tr.0.isometry.translation.vector,
                    c.point.coords - obj2.tr.0.isometry.translation.vector,
                ],
            })
            .collect()
    };

    use ColliderShape::*;
    match (obj1.coll.0.shape(), obj2.coll.0.shape()) {
        (Circle { r: r1 }, Circle { r: r2 }) => {
            complete(circle_circle(&obj1.tr.0, *r1, &obj2.tr.0, *r2))
        }
        (Rect { hw, hh }, Circle { r }) => {
            complete(rect_circle(&obj1.tr.0, *hw, *hh, &obj2.tr.0, *r))
        }
        (Circle { r }, Rect { hw, hh }) => {
            flip_contacts(complete(rect_circle(&obj2.tr.0, *hw, *hh, &obj1.tr.0, *r)))
        }
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            complete(rect_rect(&obj1.tr.0, *hw1, *hh1, &obj2.tr.0, *hw2, *hh2))
        }
    }
}

fn flip_contacts(mut contacts: Vec<Contact>) -> Vec<Contact> {
    for c in &mut contacts {
        c.point -= c.depth * (*c.normal);
        c.normal = -c.normal;
    }
    contacts
}

fn circle_circle(tr1: &m::Transform, r1: f32, tr2: &m::Transform, r2: f32) -> Vec<Contact_> {
    let pos1 = m::Point2::from(tr1.isometry.translation.vector);
    let pos2 = m::Point2::from(tr2.isometry.translation.vector);

    let r1_s = r1 * tr1.scaling();
    let r2_s = r2 * tr2.scaling();

    let dist = pos2 - pos1;
    let dist_sq = dist.norm_squared();

    let depth;
    let normal;
    if dist_sq < 0.001 {
        // same position, consider penetration to be on x axis
        depth = r1_s + r2_s;
        normal = m::Vec2::x_axis();
    } else if dist_sq < (r1_s + r2_s) * (r1_s + r2_s) {
        // normal collision
        depth = (r1_s + r2_s) - dist.norm();
        normal = na::Unit::new_normalize(dist);
    } else {
        return vec![];
    }

    vec![Contact_ {
        normal,
        depth,
        point: pos1 + (r1_s * (*normal)),
    }]
}

fn rect_circle(
    tr_rect: &m::Transform,
    hw: f32,
    hh: f32,
    tr_circle: &m::Transform,
    r: f32,
) -> Vec<Contact_> {
    let tr_c_wrt_rect = tr_rect.inverse() * tr_circle;
    let dist = tr_c_wrt_rect.isometry.translation.vector;
    let dist_abs = m::Vec2::new(dist.x.abs(), dist.y.abs());
    let dist_signums = m::Vec2::new(dist.x.signum(), dist.y.signum());
    let r = tr_c_wrt_rect.scaling() * r;

    let c_to_corner = m::Vec2::new(hw - dist_abs.x, hh - dist_abs.y);
    if c_to_corner.x < -r || c_to_corner.y < -r {
        // too far to possibly intersect
        return vec![];
    }
    let point_abs: m::Point2;
    let depth: f32;
    let normal_abs: na::Unit<m::Vec2>;
    if c_to_corner.x > 0.0 && c_to_corner.y > 0.0 {
        // circle center is inside the rect
        if c_to_corner.x < c_to_corner.y {
            point_abs = m::Point2::new(hw, dist_abs.y);
            depth = c_to_corner.x + r;
            normal_abs = m::Vec2::x_axis();
        } else {
            point_abs = m::Point2::new(dist_abs.x, hh);
            depth = c_to_corner.y + r;
            normal_abs = m::Vec2::y_axis();
        };
    } else if c_to_corner.x > 0.0 {
        // inside in the x direction but not y
        point_abs = m::Point2::new(dist_abs.x, hh);
        depth = c_to_corner.y + r; // c_to_corner.y is negative
        normal_abs = m::Vec2::y_axis();
    } else if c_to_corner.y > 0.0 {
        // inside in the y direction but not x
        point_abs = m::Point2::new(hw, dist_abs.y);
        depth = c_to_corner.x + r;
        normal_abs = m::Vec2::x_axis();
    } else {
        // outside both edges, possible intersection with the corner point
        depth = r - c_to_corner.norm();
        if depth > 0.0 {
            point_abs = m::Point2::new(hw, hh);
            normal_abs = na::Unit::new_normalize(-c_to_corner);
        } else {
            return vec![];
        }
    }

    vec![Contact_ {
        normal: tr_rect.isometry.rotation
            * na::Unit::new_unchecked(m::Vec2::new(
                dist_signums.x * normal_abs.x,
                dist_signums.y * normal_abs.y,
            )),
        depth: tr_rect.scaling() * depth,
        point: tr_rect * m::Point2::new(dist_signums.x * point_abs.x, dist_signums.y * point_abs.y),
    }]
}

fn rect_rect(
    tr1: &m::Transform,
    hw1: f32,
    hh1: f32,
    tr2: &m::Transform,
    hw2: f32,
    hh2: f32,
) -> Vec<Contact_> {
    let tr2_wrt_tr1 = tr1.inverse() * tr2;

    // obj1 is axis-aligned at origin, these are obj2's values
    let dist = tr2_wrt_tr1.isometry.translation.vector;

    let hw2 = tr2_wrt_tr1.scaling() * hw2;
    let hh2 = tr2_wrt_tr1.scaling() * hh2;

    // aligned special cases

    let rot_ang = tr2_wrt_tr1.isometry.rotation.angle();

    if rot_ang.abs() < FLAT_COLLISION_ANGLE_THRESHOLD
        || (rot_ang.abs() - PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
    {
        return aabb_aabb(dist, hw1, hh1, hw2, hh2)
            .into_iter()
            .map(|cont| transform_contact(&tr1, cont))
            .collect();
    } else if (rot_ang - 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
        || (rot_ang + 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
    {
        return aabb_aabb(dist, hw1, hh1, hh2, hw2)
            .into_iter()
            .map(|cont| transform_contact(&tr1, cont))
            .collect();
    }

    // unaligned general case with one collision point

    let x2_axis = tr2_wrt_tr1.isometry.rotation * m::Vec2::x_axis();
    let hw2_v = hw2 * (*x2_axis);

    let y2_axis = na::Unit::new_unchecked(m::Vec2::new(-x2_axis.y, x2_axis.x));
    let hh2_v = hh2 * (*y2_axis);

    let axes = [m::Vec2::x_axis(), m::Vec2::y_axis(), x2_axis, y2_axis];

    // penetration
    let x1_pen = hw1 + hw2_v.x.abs() + hh2_v.x.abs() - dist.x.abs();
    if x1_pen <= 0.0 {
        return vec![];
    }
    let y1_pen = hh1 + hw2_v.y.abs() + hh2_v.y.abs() - dist.y.abs();
    if y1_pen <= 0.0 {
        return vec![];
    }

    let x2_pen = hw2 + x2_axis.x.abs() * hw1 + x2_axis.y.abs() * hh1 - (dist.dot(&x2_axis)).abs();
    if x2_pen <= 0.0 {
        return vec![];
    }
    let y2_pen = hh2 + y2_axis.x.abs() * hw1 + y2_axis.y.abs() * hh1 - (dist.dot(&y2_axis)).abs();
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
                .expect("Something went wrong comparing floats")
        })
        .unwrap();

    // orient axis of penetration towards obj2
    let axis = na::Unit::new_unchecked(axis.dot(&dist).signum() * axis.as_ref());
    let depth_s = depth * tr1.scaling();
    let normal = tr1.isometry.rotation * axis;

    if axis_i <= 1 {
        // axis is on obj1, penetrating point is on obj2
        let point = m::Point2::from(
            dist - (axis.dot(&hw2_v).signum() * hw2_v) - (axis.dot(&hh2_v).signum() * hh2_v),
        );
        vec![Contact_ {
            normal,
            depth: depth_s,
            point: tr1 * (point + depth * (*axis)),
        }]
    } else {
        // axis is on obj2, penetrating point is on obj1
        let point = m::Point2::new(axis.x.signum() * hw1, axis.y.signum() * hh1);
        vec![Contact_ {
            normal,
            depth: depth_s,
            point: tr1 * point,
        }]
    }
}

fn transform_contact(tr: &m::Transform, cont: Contact_) -> Contact_ {
    Contact_ {
        normal: tr.isometry.rotation * cont.normal,
        depth: cont.depth * tr.scaling(),
        point: tr * cont.point,
    }
}

fn aabb_aabb(dist: m::Vec2, hw1: f32, hh1: f32, hw2: f32, hh2: f32) -> Vec<Contact_> {
    let x_pen = hw1 + hw2 - dist.x.abs();
    if x_pen <= 0.0 {
        return vec![];
    }
    let y_pen = hh1 + hh2 - dist.y.abs();
    if y_pen <= 0.0 {
        return vec![];
    }

    let x_dir = dist.x.signum();
    let y_dir = dist.y.signum();

    if x_pen < y_pen {
        let x1 = x_dir * hw1;
        let y1 = (-hh1).max(dist.y - hh2);
        let y2 = hh1.min(dist.y + hh2);

        vec![
            Contact_ {
                normal: na::Unit::new_unchecked(m::Vec2::new(x_dir, 0.0)),
                depth: x_pen,
                point: m::Point2::new(x1, y1),
            },
            Contact_ {
                normal: na::Unit::new_unchecked(m::Vec2::new(x_dir, 0.0)),
                depth: x_pen,
                point: m::Point2::new(x1, y2),
            },
        ]
    } else {
        let y1 = y_dir * hh1;
        let x1 = (-hw1).max(dist.x - hw2);
        let x2 = hw1.min(dist.x + hw2);

        vec![
            Contact_ {
                normal: na::Unit::new_unchecked(m::Vec2::new(0.0, y_dir)),
                depth: y_pen,
                point: na::Point2::new(x1, y1),
            },
            Contact_ {
                normal: na::Unit::new_unchecked(m::Vec2::new(0.0, y_dir)),
                depth: y_pen,
                point: na::Point2::new(x2, y1),
            },
        ]
    }
}
