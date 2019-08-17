use super::Collider;
use crate::{ecs::IdType, util::Transform};

/// A broad phase algorithm for collision detection,
/// responsible for generating pairs of possibly intersecting objects.
pub trait BroadPhase {
    /// Returns pairs of ids of potentially intersecting objects.
    /// Only returns ids and not Collidables because lookup by id is necessary
    /// in the narrow phase anyway; this way borrows aren't held for longer than needed.
    fn pairs<'a>(items: impl Iterator<Item = Collidable<'a>> + Clone) -> Vec<[IdType; 2]>;
}

/// The simplest possible broad phase algorithm,
/// which pairs every object with every other object.
/// Very inefficient, but can work for small systems.
pub struct BruteForce;

impl BroadPhase for BruteForce {
    fn pairs<'a>(mut items: impl Iterator<Item = Collidable<'a>> + Clone) -> Vec<[IdType; 2]> {
        let mut pairs = Vec::new();
        while let Some(item) = items.next() {
            for other in items.clone() {
                pairs.push([item.id, other.id]);
            }
        }

        pairs
    }
}

#[derive(Clone, Copy)]
pub struct Collidable<'a> {
    pub id: IdType,
    pub tr: &'a Transform,
    pub coll: &'a Collider,
}
