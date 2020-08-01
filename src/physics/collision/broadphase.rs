//! Broad phase collision detection algorithms
//! are responsible for detecting pairs of possibly intersecting objects
//! for further, more accurate narrow phase inspection.

use crate::physics::{BodyNodes, BodyRef};

/// A broad phase algorithm.
pub trait BroadPhase {
    /// Returns pairs of potentially intersecting objects.
    fn pairs<'a>(items: impl Iterator<Item = BodyRef<'a>> + Clone) -> Vec<[BodyNodes; 2]>;
}

/// The simplest possible broad phase algorithm,
/// which pairs every object with every other object.
/// Very inefficient, but can work for small systems.
pub struct BruteForce;

impl BroadPhase for BruteForce {
    fn pairs<'a>(mut items: impl Iterator<Item = BodyRef<'a>> + Clone) -> Vec<[BodyNodes; 2]> {
        let mut pairs = Vec::new();
        while let Some(b1) = items.next() {
            for b2 in items.clone() {
                pairs.push([(&b1).into(), (&b2).into()]);
            }
        }

        pairs
    }
}
