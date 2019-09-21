use crate::ecs::{event::SpaceEvent, space::Space, IdType};
use nalgebra::{Point2, Unit, Vector2};

pub mod broadphase;

mod collider;
pub use collider::{Collider, ColliderShape};

mod narrowphase;
mod solver;
pub use solver::{CollisionSolver, SolverLoopCondition};

pub use crate::util::Transform;

/// Event containing information about a collision relative to one of the objects involved.
/// # Listener behavior
/// Only the listener for the involved object is called.
#[derive(Clone, Copy, Debug)]
pub struct CollisionEvent {
    pub source: IdType,
    /// The id of the object that was collided with.
    pub other: IdType,
    /// The normal of the plane of collision, pointing towards this object.
    pub normal: Unit<Vector2<f32>>,
    /// The depth of penetration.
    pub depth: f32,
    /// The world-space coordinates of the exact point on the surface of this object where the collision occurred.
    pub point: Point2<f32>,
}

impl SpaceEvent for CollisionEvent {
    fn handle(&self, space: &mut Space) {
        space.run_listener(self.source, self);
    }
}
