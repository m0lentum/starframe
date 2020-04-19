//! Broad phase collision detection algorithms
//! are responsible for detecting pairs of possibly intersecting objects
//! for further, more accurate narrow phase inspection.
use super::BodyRef;

/// A broad phase algorithm.
pub trait BroadPhase {
    /// Returns pairs of potentially intersecting objects.
    ///
    /// Implementors should note that the pairs are of their indices in the iterator, not their IDs.
    fn pairs<'a>(items: impl Iterator<Item = BodyRef<'a>> + Clone) -> Vec<[usize; 2]>;
}

/// The simplest possible broad phase algorithm,
/// which pairs every object with every other object.
/// Very inefficient, but can work for small systems.
pub struct BruteForce;

impl BroadPhase for BruteForce {
    fn pairs<'a>(items: impl Iterator<Item = BodyRef<'a>> + Clone) -> Vec<[usize; 2]> {
        let mut pairs = Vec::new();
        let mut indexed_items = items.enumerate();
        while let Some((i, _)) = indexed_items.next() {
            for (j, _) in indexed_items.clone() {
                pairs.push([i, j]);
            }
        }

        pairs
    }
}
