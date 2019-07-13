use super::{
    super::{
        integrator::{Integrator, IntegratorState},
        ConstraintSolver,
    },
    BroadPhase, NarrowPhase, RigidBodyFilter,
};
use crate::ecs::{event::EventQueue, system::*, Space};

// idea: all implementations of one step of the physics process should be interchangeable
// maybe also ability to add extra steps?
// e.g. gravity, wind, other non-contact forces as custom extra steps
pub struct CollisionPipeline<I, B, N, S>
where
    I: Integrator,
    B: BroadPhase,
    N: NarrowPhase,
    S: ConstraintSolver,
{
    integrator: I,
    broad_phase: B,
    narrow_phase: N,
    constraint_solver: S,
}

impl<'a, I, B, N, S> System<'a> for CollisionPipeline<I, B, N, S>
where
    I: Integrator,
    B: BroadPhase,
    N: NarrowPhase,
    S: ConstraintSolver,
{
    type Filter = RigidBodyFilter<'a>;

    fn run_system(&mut self, items: &mut [Self::Filter], _space: &Space, _queue: &mut EventQueue) {
        let timestep = 0.1;

        while let IntegratorState::NeedsDerivatives = self.integrator.step(
            timestep,
            items
                .iter_mut()
                .map(|rbf| (&mut *rbf.tr, &mut rbf.body.velocity)),
        ) {
            // TODO: solve collisions, apply other forces
        }

        // broad phase
        // generating contacts does not require mutable access.
        // maybe this should be done with an immutable filter so other things can run in parallel?
        // transforms are used in a lot of places so this is potentially quite beneficial

        // narrow phase

        // constraint solve
        // potentially also other constraints than collision
        // how generic can this be?

        // events
    }
}
