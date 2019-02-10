use super::{Collision, Manifold};
use moleengine::ecs::IdType;
use moleengine::util::Transform;

use nalgebra::{Point2, Unit, Vector2};
use std::f32::consts::PI;

/// determines how close to parallel two surfaces need to be for their collision
/// manifold to have two points
pub const FLAT_COLLISION_ANGLE_THRESHOLD: f32 = 0.02;

pub fn circle_circle(
    obj1: IdType,
    tr1: &Transform,
    r1: f32,
    obj2: IdType,
    tr2: &Transform,
    r2: f32,
) -> Option<[Collision; 2]> {
    let pos1 = tr1.0 * Point2::origin();
    let pos2 = tr2.0 * Point2::origin();

    let r1_s = r1 * tr1.0.scaling();
    let r2_s = r2 * tr2.0.scaling();

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
        return None;
    }

    Some([
        Collision {
            source: obj1,
            other: obj2,
            normal,
            depth,
            manifold: Manifold(pos1 + (normal.as_ref() * r1_s), None),
        },
        Collision {
            source: obj2,
            other: obj1,
            normal: -normal,
            depth,
            manifold: Manifold(pos2 - (normal.as_ref() * r2_s), None),
        },
    ])
}

pub fn rect_rect(
    obj1: IdType,
    tr1: &Transform,
    hw1: f32,
    hh1: f32,
    obj2: IdType,
    tr2: &Transform,
    hw2: f32,
    hh2: f32,
) -> Option<[Collision; 2]> {
    let tr2_wrt_tr1 = tr1.0.inverse() * tr2.0;

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
        return aabb_aabb(dist.coords, obj1, hw1, hh1, obj2, hw2, hh2).map(|mut colls| {
            transform_collision(&tr1.0, &mut colls[0]);
            transform_collision(&tr1.0, &mut colls[1]);
            colls
        });
    } else if (rot_ang - 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
        || (rot_ang + 0.5 * PI).abs() < FLAT_COLLISION_ANGLE_THRESHOLD
    {
        return aabb_aabb(dist.coords, obj1, hw1, hh1, obj2, hh2, hw2).map(|mut colls| {
            transform_collision(&tr1.0, &mut colls[0]);
            transform_collision(&tr1.0, &mut colls[1]);
            colls
        });
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
        return None;
    }
    let y1_pen = hh1 + hw2_v[1].abs() + hh2_v[1].abs() - dist.coords[1].abs();
    if y1_pen <= 0.0 {
        return None;
    }

    let x2_pen =
        hw2 + x2_axis[0].abs() * hw1 + x2_axis[1].abs() * hh1 - (dist.coords.dot(&x2_axis)).abs();
    if x2_pen <= 0.0 {
        return None;
    }
    let y2_pen =
        hh2 + y2_axis[0].abs() * hw1 + y2_axis[1].abs() * hh1 - (dist.coords.dot(&y2_axis)).abs();
    if y2_pen <= 0.0 {
        return None;
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

    // orient axis of penetration towards obj2 (remember to flip if axis is on obj2!)
    let axis = Unit::new_unchecked(axis.dot(&dist.coords).signum() * (**axis));
    let depth_s = *depth * tr1.0.scaling();
    let normal = tr1.0.isometry.rotation * axis;

    if axis_i <= 1 {
        // axis is on obj1, penetrating point is on obj2
        let point = Point2::from(
            dist.coords - (axis.dot(&hw2_v).signum() * hw2_v) - (axis.dot(&hh2_v).signum() * hh2_v),
        );
        Some([
            Collision {
                source: obj1,
                other: obj2,
                normal,
                depth: depth_s,
                manifold: Manifold(tr1.0 * (point + (*depth) * (*axis)), None),
            },
            Collision {
                source: obj2,
                other: obj1,
                normal: -normal,
                depth: depth_s,
                manifold: Manifold(tr1.0 * point, None),
            },
        ])
    } else {
        // axis is on obj2, penetrating point is on obj1
        let point = Point2::new(axis[0].signum() * hw1, axis[1].signum() * hh1);
        Some([
            Collision {
                source: obj1,
                other: obj2,
                normal,
                depth: depth_s,
                manifold: Manifold(tr1.0 * point, None),
            },
            Collision {
                source: obj2,
                other: obj1,
                normal: -normal,
                depth: depth_s,
                manifold: Manifold(tr1.0 * (point - (*depth) * (*axis)), None),
            },
        ])
    }
}

fn transform_collision(tr: &nalgebra::Similarity2<f32>, coll: &mut Collision) {
    coll.normal = tr.isometry.rotation * coll.normal;
    coll.manifold.0 = tr * coll.manifold.0;
    coll.manifold.1 = coll.manifold.1.map(|p| tr * p);
}

fn aabb_aabb(
    dist: Vector2<f32>,
    obj1: IdType,
    hw1: f32,
    hh1: f32,
    obj2: IdType,
    hw2: f32,
    hh2: f32,
) -> Option<[Collision; 2]> {
    let x_pen = hw1 + hw2 - dist[0].abs();
    if x_pen <= 0.0 {
        return None;
    }
    let y_pen = hh1 + hh2 - dist[1].abs();
    if y_pen <= 0.0 {
        return None;
    }

    let x_dir = dist[0].signum();
    let y_dir = dist[1].signum();

    if x_pen < y_pen {
        let x1 = x_dir * hw1;
        let x2 = dist[0] - x_dir * hw2;
        let y1 = (-hh1).max(dist[1] - hh2);
        let y2 = hh1.min(dist[1] + hh2);

        let m1 = Manifold(Point2::new(x1, y1), Some(Point2::new(x2, y1)));
        let m2 = Manifold(Point2::new(x1, y2), Some(Point2::new(x2, y2)));

        Some([
            Collision {
                source: obj1,
                other: obj2,
                normal: Unit::new_unchecked(Vector2::new(x_dir, 0.0)),
                depth: x_pen,
                manifold: m1,
            },
            Collision {
                source: obj2,
                other: obj1,
                normal: Unit::new_unchecked(Vector2::new(-x_dir, 0.0)),
                depth: x_pen,
                manifold: m2,
            },
        ])
    } else {
        let y1 = y_dir * hh1;
        let y2 = dist[1] - y_dir * hh2;
        let x1 = (-hw1).max(dist[0] - hw2);
        let x2 = hw1.min(dist[0] + hw2);

        let m1 = Manifold(Point2::new(x1, y1), Some(Point2::new(x1, y2)));
        let m2 = Manifold(Point2::new(x2, y1), Some(Point2::new(x2, y2)));

        Some([
            Collision {
                source: obj1,
                other: obj2,
                normal: Unit::new_unchecked(Vector2::new(y_dir, 0.0)),
                depth: y_pen,
                manifold: m1,
            },
            Collision {
                source: obj2,
                other: obj1,
                normal: Unit::new_unchecked(Vector2::new(-y_dir, 0.0)),
                depth: y_pen,
                manifold: m2,
            },
        ])
    }
}
