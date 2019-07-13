use super::{BodyPair, Collision};

// I don't think there are meaningfully different narrow phase algorithms,
// so this probably won't actually need to be a trait
pub trait NarrowPhase {
    fn contacts<'a>(pairs: &'a [BodyPair<'a>]) -> &'a [Collision];
}
