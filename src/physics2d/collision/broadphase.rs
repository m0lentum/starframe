use super::{BodyPair, RigidBodyFilter};

// TODO: figure out a good API for this

pub trait BroadPhase {
    fn pairs<'a>(items: &'a [RigidBodyFilter<'a>]) -> &'a [BodyPair<'a>];
}

pub struct PlaceholderBroadPhase {}

impl BroadPhase for PlaceholderBroadPhase {
    fn pairs<'a>(_items: &'a [RigidBodyFilter<'a>]) -> &'a [BodyPair<'a>] {
        &[]
    }
}
