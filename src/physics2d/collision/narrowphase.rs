use super::{broadphase::Collidable, collider::ColliderShape};
use crate::util::Transform;

use nalgebra::{Point2, Unit, Vector2};
use std::f32::consts::PI;

/// determines how close to parallel two surfaces need to be to generate two contacts
const FLAT_COLLISION_ANGLE_THRESHOLD: f32 = 0.005;

/// An intersection between two objects.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// The normal, facing away from obj1
    pub normal: Unit<Vector2<f32>>,
    /// Penetration depth
    pub depth: f32,
    /// Point of contact on the surface of obj1
    pub point: Point2<f32>,
}

/// Checks two transformed colliders for intersection.
pub fn intersection_check<'a>(obj1: Collidable<'a>, obj2: Collidable<'a>) -> Vec<Contact> {
    use ColliderShape::*;
    match (obj1.coll.shape(), obj2.coll.shape()) {
        (Circle { r: r1 }, Circle { r: r2 }) => circle_circle(obj1.tr, *r1, obj2.tr, *r2),
        (Circle { .. }, Rect { .. }) => vec![],
        (Rect { .. }, Circle { .. }) => vec![],
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            rect_rect(obj1.tr, *hw1, *hh1, obj2.tr, *hw2, *hh2)
        }
    }
}

fn circle_circle(tr1: &Transform, r1: f32, tr2: &Transform, r2: f32) -> Vec<Contact> {
    let pos1 = tr1.0 * Point2::origin();
    let pos2 = tr2.0 * Point2::origin();

    let r1_s = r1 * tr1.scaling();
    let r2_s = r2 * tr2.scaling();

    let dist = pos2 - pos1;
    let dist_sq = dist.norm_squared();

    let depth;
    let normal;
    if dist_sq < 0.001 {
        // same position, consider penetration to be on x axis
        depth = r1_s + r2_s;
        normal = Vector2::x_axis();
    } else if dist_sq < (r1_s + r2_s) * (r1_s + r2_s) {
        // normal collision
        depth = (r1_s + r2_s) - dist.norm();
        normal = Unit::new_normalize(dist);
    } else {
        return vec![];
    }

    vec![Contact {
        normal,
        depth,
        point: pos1 + (normal.as_ref() * r1_s),
    }]
}

fn rect_rect(
    tr1: &Transform,
    hw1: f32,
    hh1: f32,
    tr2: &Transform,
    hw2: f32,
    hh2: f32,
) -> Vec<Contact> {
    let tr2_wrt_tr1 = tr1.inverse() * tr2.0;

    // obj1 is axis-aligned at origin, these are obj2's values
    let dist = tr2_wrt_tr1 * Point2::origin();
    let rot = tr2_wrt_tr1.isometry.rotation;
    let rot_ang = rot.angle(); // ]-pi, pi]

    let hw2 = tr2_wrt_tr1.scaling() * hw2;
    let hh2 = tr2_wrt_tr1.scaling() * hh2;

    // aligned special cases

    if rot_ang.abs() < FLAT_COLLISION_ANGLE_THRESHOLD
        || (rot_ang.abs() - PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
    {
        return aabb_aabb(dist.coords, hw1, hh1, hw2, hh2)
            .into_iter()
            .map(|cont| transform_contact(&tr1, cont))
            .collect();
    } else if (rot_ang - 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
        || (rot_ang + 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
    {
        return aabb_aabb(dist.coords, hw1, hh1, hh2, hw2)
            .into_iter()
            .map(|cont| transform_contact(&tr1, cont))
            .collect();
    }

    // unaligned general case with one collision point

    let x2_axis = rot * Vector2::x_axis();
    let hw2_v = hw2 * (*x2_axis);

    let y2_axis = Unit::new_unchecked(Vector2::new(-x2_axis[1], x2_axis[0]));
    let hh2_v = hh2 * (*y2_axis);

    let axes = [Vector2::x_axis(), Vector2::y_axis(), x2_axis, y2_axis];

    // penetration
    let x1_pen = hw1 + hw2_v[0].abs() + hh2_v[0].abs() - dist.coords[0].abs();
    if x1_pen <= 0.0 {
        return vec![];
    }
    let y1_pen = hh1 + hw2_v[1].abs() + hh2_v[1].abs() - dist.coords[1].abs();
    if y1_pen <= 0.0 {
        return vec![];
    }

    let x2_pen =
        hw2 + x2_axis[0].abs() * hw1 + x2_axis[1].abs() * hh1 - (dist.coords.dot(&x2_axis)).abs();
    if x2_pen <= 0.0 {
        return vec![];
    }
    let y2_pen =
        hh2 + y2_axis[0].abs() * hw1 + y2_axis[1].abs() * hh1 - (dist.coords.dot(&y2_axis)).abs();
    if y2_pen <= 0.0 {
        return vec![];
    }

    let depths = [x1_pen, y1_pen, x2_pen, y2_pen];

    let ((axis_i, axis), depth) = axes
        .iter()
        .enumerate()
        .zip(depths.iter())
        .min_by(|(_, d1), (_, d2)| {
            d1.partial_cmp(d2)
                .expect("Something went wrong comparing floats")
        })
        .unwrap();

    // orient axis of penetration towards obj2
    let axis = Unit::new_unchecked(axis.dot(&dist.coords).signum() * (**axis));
    let depth_s = *depth * tr1.scaling();
    let normal = tr1.isometry.rotation * axis;

    if axis_i <= 1 {
        // axis is on obj1, penetrating point is on obj2
        let point = Point2::from(
            dist.coords - (axis.dot(&hw2_v).signum() * hw2_v) - (axis.dot(&hh2_v).signum() * hh2_v),
        );
        vec![Contact {
            normal,
            depth: depth_s,
            point: tr1.0 * (point + (*depth) * (*axis)),
        }]
    } else {
        // axis is on obj2, penetrating point is on obj1
        let point = Point2::new(axis[0].signum() * hw1, axis[1].signum() * hh1);
        vec![Contact {
            normal,
            depth: depth_s,
            point: tr1.0 * point,
        }]
    }
}

fn transform_contact(tr: &Transform, cont: Contact) -> Contact {
    Contact {
        normal: tr.isometry.rotation * cont.normal,
        depth: cont.depth * tr.scaling(),
        point: tr.0 * cont.point,
    }
}

fn aabb_aabb(dist: Vector2<f32>, hw1: f32, hh1: f32, hw2: f32, hh2: f32) -> Vec<Contact> {
    let x_pen = hw1 + hw2 - dist[0].abs();
    if x_pen <= 0.0 {
        return vec![];
    }
    let y_pen = hh1 + hh2 - dist[1].abs();
    if y_pen <= 0.0 {
        return vec![];
    }

    let x_dir = dist[0].signum();
    let y_dir = dist[1].signum();

    if x_pen < y_pen {
        let x1 = x_dir * hw1;
        let y1 = (-hh1).max(dist[1] - hh2);
        let y2 = hh1.min(dist[1] + hh2);

        vec![
            Contact {
                normal: Unit::new_unchecked(Vector2::new(x_dir, 0.0)),
                depth: x_pen,
                point: Point2::new(x1, y1),
            },
            Contact {
                normal: Unit::new_unchecked(Vector2::new(x_dir, 0.0)),
                depth: x_pen,
                point: Point2::new(x1, y2),
            },
        ]
    } else {
        let y1 = y_dir * hh1;
        let x1 = (-hw1).max(dist[0] - hw2);
        let x2 = hw1.min(dist[0] + hw2);

        vec![
            Contact {
                normal: Unit::new_unchecked(Vector2::new(0.0, y_dir)),
                depth: y_pen,
                point: Point2::new(x1, y1),
            },
            Contact {
                normal: Unit::new_unchecked(Vector2::new(0.0, y_dir)),
                depth: y_pen,
                point: Point2::new(x2, y1),
            },
        ]
    }
}
