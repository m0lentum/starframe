use super::collider::ColliderShape;
use crate::math::{self, uv, Unit};
use crate::physics::BodyRef;

use std::f32::consts::PI;

/// determines how close to parallel two surfaces need to be to generate two contacts
const FLAT_COLLISION_ANGLE_THRESHOLD: f32 = 0.005;

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

    // aligned special cases
    // TODO: this isn't robust, clip the incident edges instead

    let rot_ang = (tr2_wrt_tr1.rotation * uv::Vec2::unit_x()).x.acos();

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

    if axis_i <= 1 {
        // axis is on obj1, penetrating point is on obj2
        let point = dist - (axis.dot(hw2_v).signum() * hw2_v) - (axis.dot(hh2_v).signum() * hh2_v);
        vec![Contact_ {
            normal,
            depth,
            point: *tr1 * (point + depth * (*axis)),
        }]
    } else {
        // axis is on obj2, penetrating point is on obj1
        let point = uv::Vec2::new(axis.x.signum() * hw1, axis.y.signum() * hh1);
        vec![Contact_ {
            normal,
            depth,
            point: *tr1 * point,
        }]
    }
}

fn transform_contact(tr: &uv::Isometry2, cont: Contact_) -> Contact_ {
    Contact_ {
        normal: tr.rotation * cont.normal,
        depth: cont.depth,
        point: *tr * cont.point,
    }
}

fn aabb_aabb(dist: uv::Vec2, hw1: f32, hh1: f32, hw2: f32, hh2: f32) -> Vec<Contact_> {
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
                normal: Unit::new_unchecked(uv::Vec2::new(x_dir, 0.0)),
                depth: x_pen,
                point: uv::Vec2::new(x1, y1),
            },
            Contact_ {
                normal: Unit::new_unchecked(uv::Vec2::new(x_dir, 0.0)),
                depth: x_pen,
                point: uv::Vec2::new(x1, y2),
            },
        ]
    } else {
        let y1 = y_dir * hh1;
        let x1 = (-hw1).max(dist.x - hw2);
        let x2 = hw1.min(dist.x + hw2);

        vec![
            Contact_ {
                normal: Unit::new_unchecked(uv::Vec2::new(0.0, y_dir)),
                depth: y_pen,
                point: uv::Vec2::new(x1, y1),
            },
            Contact_ {
                normal: Unit::new_unchecked(uv::Vec2::new(0.0, y_dir)),
                depth: y_pen,
                point: uv::Vec2::new(x2, y1),
            },
        ]
    }
}
