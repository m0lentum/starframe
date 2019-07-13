use super::{BodyPair, RigidBodyFilter};

// there will be many broad phase options (grid, hgrid, quadtree etc.)
// and generating possibly colliding pairs is their only purpose.
// making these pluggable was what inpired this whole pipeline idea
pub trait BroadPhase {
    fn pairs<'a>(items: &'a [RigidBodyFilter<'a>]) -> &'a [BodyPair<'a>];
}
