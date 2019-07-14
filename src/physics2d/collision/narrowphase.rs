use super::{broadphase::{BodyPair, Collidable}, queries, Collider, Collision};
use crate::{ecs::IdType, util::Transform};

// I don't think there are meaningfully different narrow phase algorithms,
// so this probably won't actually need to be a trait
pub trait NarrowPhase {
    fn contacts<'a>(pairs: Vec<BodyPair>) -> Vec<Collision>;
}

/// Checks two transformed colliders for intersection. If one is found,
/// returns two Collisions, one relative to each of the participating objects.
pub fn intersection_check(
    obj1: IdType,
    tr1: &Transform,
    coll1: &Collider,
    obj2: IdType,
    tr2: &Transform,
    coll2: &Collider,
) -> Option<[Collision; 2]> {
    use Collider::*;
    match (coll1, coll2) {
        (Circle { r: r1 }, Circle { r: r2 }) => {
            queries::circle_circle(obj1, tr1, *r1, obj2, tr2, *r2)
        }
        (Circle { .. }, Rect { .. }) => None,
        (Rect { .. }, Circle { .. }) => None,
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            queries::rect_rect(obj1, tr1, *hw1, *hh1, obj2, tr2, *hw2, *hh2)
        }
    }
}
