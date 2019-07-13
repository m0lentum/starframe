use nalgebra::Vector2;
use crate::util::Transform;
use super::Velocity;

pub enum IntegratorState {
    NeedsDerivatives,
    Done,
}

// can I get all integrators to work with this signature?
// e.g. verlet needs two past positions but most need pos and vel
pub trait Integrator {
    fn step<'a>(
        &mut self,
        timestep: f32,
        variables: impl Iterator<Item = (&'a mut Transform, &'a mut Velocity)>,
    ) -> IntegratorState;
}
