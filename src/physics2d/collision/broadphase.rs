use super::Collider;
use crate::{ecs::IdType, util::Transform};

/// A broad phase algorithm for collision detection,
/// responsible for generating pairs of possibly intersecting objects.
pub trait BroadPhase {
    /// Run collision checks on pairs produced by this broad phase.
    fn run<'a>(
        items: impl Iterator<Item = Collidable<'a>> + Clone,
    ) -> Vec<super::narrowphase::Contact> {
        let mut contacts = Vec::new();
        for pair in Self::pairs(items) {
            contacts.append(&mut super::narrowphase::intersection_check(
                pair[0], pair[1],
            ));
        }
        contacts
    }

    /// Returns pairs of potentially intersecting objects.
    /// This is the part that differs in different broad phase implementations.
    fn pairs<'a>(items: impl Iterator<Item = Collidable<'a>> + Clone) -> Vec<[Collidable<'a>; 2]>;
}

/// The simplest possible broad phase algorithm,
/// which pairs every object with every other object.
/// Very inefficient, but can work for small systems.
pub struct BruteForce;

impl BroadPhase for BruteForce {
    fn pairs<'a>(
        mut items: impl Iterator<Item = Collidable<'a>> + Clone,
    ) -> Vec<[Collidable<'a>; 2]> {
        let mut pairs = Vec::new();
        while let Some(item) = items.next() {
            for other in items.clone() {
                pairs.push([item, other]);
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
    pub responds_to_collisions: bool,
}
