use ultraviolet as uv;

//

pub mod broadphase;
pub use broadphase::BroadPhase;

mod collider;
pub use collider::{Collider, ColliderShape};

mod narrowphase;
pub use narrowphase::Contact;
mod solver;
pub use solver::ContactSolver;

// TODO: reimplement events

// #[derive(Clone, Copy, Debug)]
// pub struct CollisionEvent {
//     pub source: usize,
// /// The id of the object that was collided with.
// pub other: usize,
// /// The normal of the plane of collision, pointing towards this object.
// pub normal: uv::Vec2,
// /// The depth of penetration.
// pub depth: f32,
// /// The world-space coordinates of the exact point on the surface of this object where the collision occurred.
// pub point: uv::Vec2,
// }

// impl SpaceEvent for CollisionEvent {
//     fn handle(&self, space: &mut Space) {
//         space.run_listener(self.source, self);
//     }
// }
