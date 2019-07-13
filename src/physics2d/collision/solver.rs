use super::{
    super::integrator::{Integrator, IntegratorState},
    intersection_check, BroadPhase, RigidBodyFilter,
};
use crate::ecs::{event::EventQueue, system::*, Space};
use std::marker::PhantomData;

pub struct CollisionSolver<I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    timestep: f32,
    integrator_marker: PhantomData<I>,
    broad_phase_marker: PhantomData<B>,
}

impl<I, B> CollisionSolver<I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    pub fn with_timestep(timestep: f32) -> Self {
        CollisionSolver {
            timestep,
            integrator_marker: PhantomData,
            broad_phase_marker: PhantomData,
        }
    }

    pub fn set_timestep(&mut self, timestep: f32) {
        self.timestep = timestep;
    }
}

impl<'a, I, B> System<'a> for CollisionSolver<I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    type Filter = RigidBodyFilter<'a>;

    fn run_system(&mut self, items: &mut [Self::Filter], space: &Space, queue: &mut EventQueue) {
        let mut integrator = I::begin_step(self.timestep);

        while let IntegratorState::NeedsDerivatives = integrator.substep(
            items
                .iter_mut()
                .map(|rbf| (&mut *rbf.tr, &mut rbf.body.velocity)),
        ) {
            // TODO: use the broad phase
            space.write_global_state(|colls| {
                let mut collisions = Vec::new();
                // ugly brute force for now
                let mut iter = items.iter();
                while let Some(o1) = iter.next() {
                    for o2 in iter.clone() {
                        match (o1.coll, o2.coll) {
                            (Some(coll1), Some(coll2)) => {
                                if let Some(colls) =
                                    intersection_check(o1.id, o1.tr, coll1, o2.id, o2.tr, coll2)
                                {
                                    // testing
                                    collisions.push(colls[0]);
                                    collisions.push(colls[1]);

                                    queue.push(Box::new(colls[0]));
                                    queue.push(Box::new(colls[1]));
                                }
                            }

                            _ => (),
                        }
                    }
                }

                std::mem::replace(colls, collisions);
            });
        }
    }
}
