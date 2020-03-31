pub mod collision;
pub use collision::{Collider, ColliderShape, ContactSolver};

pub mod constraint;
pub use constraint::Constraint;

pub mod forcefield;
pub use forcefield::ForceField;

pub mod integrator;
pub use integrator::Integrator;

pub mod rigidbody;
pub use rigidbody::RigidBody;

//

use ultraviolet as uv;

#[derive(Copy, Clone, Debug)]
pub struct Velocity {
    /// Linear velocity in metres per second.
    pub linear: uv::Vec2,
    /// Angular velocity in radians per second.
    pub angular: f32,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: uv::Vec2::zero(),
            angular: 0.0,
        }
    }
}

impl Velocity {
    /// Get the linear velocity of a point offset from the center of mass.
    pub fn point_velocity(&self, offset: uv::Vec2) -> uv::Vec2 {
        let tangent = uv::Vec2::new(-offset[1], offset[0]) * self.angular;
        self.linear + tangent
    }
}
