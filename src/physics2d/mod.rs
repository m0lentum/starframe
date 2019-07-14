pub mod collision;
pub mod constraint;
pub mod integrator;
pub mod rigidbody;

pub use collision::{Collider, Collision};
// pub use constraint::ConstraintSolver;
pub use integrator::Integrator;
pub use rigidbody::RigidBody;

use nalgebra::Vector2;

#[derive(Copy, Clone)]
pub struct Velocity {
    pub linear: Vector2<f32>,
    pub angular: f32,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: Vector2::zeros(),
            angular: 0.0,
        }
    }
}
