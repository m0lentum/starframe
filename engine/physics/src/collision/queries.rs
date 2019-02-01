use super::{Collision, Manifold};
use moleengine::ecs::IdType;
use moleengine::Transform;

use nalgebra::{Point2, Unit, Vector2};

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
            manifold: Manifold(pos1 + (normal.as_ref() * r1), None),
        },
        Collision {
            source: obj2,
            other: obj1,
            normal: -normal,
            depth,
            manifold: Manifold(pos2 - (normal.as_ref() * r2), None),
        },
    ])
}
