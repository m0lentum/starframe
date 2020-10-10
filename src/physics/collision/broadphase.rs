//! Broad phase collision detection algorithms
//! are responsible for detecting pairs of possibly intersecting objects
//! for further, more accurate narrow phase inspection.

use crate::physics::BodyRef;

/// A broad phase algorithm.
pub trait BroadPhase {
    /// Returns pairs of indices to potentially intersecting objects.
    fn pairs<'a>(items: &[BodyRef]) -> Vec<[usize; 2]>;
}

/// The simplest possible broad phase algorithm,
/// which pairs every object with every other object.
/// Very inefficient, but can work for small systems.
pub struct BruteForce;

impl BroadPhase for BruteForce {
    fn pairs<'a>(items: &[BodyRef]) -> Vec<[usize; 2]> {
        (0..items.len())
            .flat_map(|b1| (b1 + 1..items.len()).map(move |b2| [b1, b2]))
            .collect()
    }
}
