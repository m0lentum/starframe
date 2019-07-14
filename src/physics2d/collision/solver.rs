use super::{
    super::{
        integrator::{Integrator, IntegratorState},
        RigidBody,
    },
    broadphase::{BroadPhase, Collidable},
    narrowphase::intersection_check,
    Collider, Transform,
};
use crate::ecs::{event::EventQueue, system::*, IdType, Space};
use std::marker::PhantomData;

/// A System that calculates movement for rigid bodies
/// while taking collisions into account.
/// Integrators and broad phase algorithms are interchangeable.
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
    /// Create a CollisionSolver with the given timestep value.
    /// When used with a constant timestep this can be called once and stored;
    /// otherwise timestep should be updated every frame either by creating
    /// a new solver with this function or using `set_timestep`.
    pub fn with_timestep(timestep: f32) -> Self {
        CollisionSolver {
            timestep,
            integrator_marker: PhantomData,
            broad_phase_marker: PhantomData,
        }
    }

    /// Set the timestep on an exising CollisionSolver.
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
            let iter = items.iter().filter_map(|rbf| {
                rbf.coll.map(|coll| Collidable {
                    id: rbf.id,
                    tr: rbf.tr,
                    coll: coll,
                })
            });

            let mut collisions = Vec::new();

            for (o1, o2) in B::pairs(iter) {
                if let Some(colls) =
                    intersection_check(o1.id, o1.tr, o1.coll, o2.id, o2.tr, o2.coll)
                {
                    // testing
                    collisions.push(colls[0]);
                    collisions.push(colls[1]);

                    queue.push(Box::new(colls[0]));
                    queue.push(Box::new(colls[1]));
                }
            }

            space.write_global_state(|colls| {
                std::mem::replace(colls, collisions);
            });
        }
    }
}

#[derive(ComponentFilter)]
pub struct RigidBodyFilter<'a> {
    #[id]
    id: IdType,
    tr: &'a mut Transform,
    body: &'a mut RigidBody,
    coll: Option<&'a Collider>,
}
