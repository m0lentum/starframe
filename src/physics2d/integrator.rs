use super::Velocity;
use crate::core::transform::{Angle, Transform};

pub enum IntegratorState {
    NeedsDerivatives,
    Done,
}

/// A time stepper for equations of motion, used with physics solvers.
/// Can have multiple substeps and request the solver for derivatives in between.
pub trait Integrator {
    /// Create an integrator in its initial state.
    fn begin_step(timestep: f32) -> Self;

    /// Execute a step of the integration algorithm.
    /// Returns NeedsDerivatives if the next step requires solving constraints,
    /// Done if the step is complete.
    fn substep<'a>(
        &mut self,
        variables: impl Iterator<Item = (&'a mut Transform, &'a mut Velocity)>,
    ) -> IntegratorState;
}

/// Explicit Euler integrator.
/// Uses velocity and position at the start of a step to determine
/// position at the end of a step.
/// Unconditionally unstable, should generally not be used.
pub struct ExplicitEuler {
    timestep: f32,
    done: bool,
}

impl Integrator for ExplicitEuler {
    fn begin_step(timestep: f32) -> Self {
        ExplicitEuler {
            timestep,
            done: false,
        }
    }

    fn substep<'a>(
        &mut self,
        variables: impl Iterator<Item = (&'a mut Transform, &'a mut Velocity)>,
    ) -> IntegratorState {
        if self.done {
            return IntegratorState::Done;
        }

        for (tr, vel) in variables {
            tr.0.append_translation(self.timestep * vel.linear);
            tr.0.prepend_rotation(Angle::Radians(self.timestep * vel.angular).into());
        }

        self.done = true;
        IntegratorState::NeedsDerivatives
    }
}

/// Semi-implicit Euler integrator.
/// Solves velocity first, then applies it to position.
/// Preserves energy and handles discontinuous constraints well.
/// Generally used for rigid body systems.
pub struct SemiImplicitEuler {
    timestep: f32,
    ready: bool,
}

impl Integrator for SemiImplicitEuler {
    fn begin_step(timestep: f32) -> Self {
        SemiImplicitEuler {
            timestep,
            ready: false,
        }
    }

    fn substep<'a>(
        &mut self,
        variables: impl Iterator<Item = (&'a mut Transform, &'a mut Velocity)>,
    ) -> IntegratorState {
        if !self.ready {
            self.ready = true;
            return IntegratorState::NeedsDerivatives;
        }

        for (tr, vel) in variables {
            tr.0.append_translation(self.timestep * vel.linear);
            tr.0.prepend_rotation(Angle::Radians(self.timestep * vel.angular).into());
        }

        IntegratorState::Done
    }
}
