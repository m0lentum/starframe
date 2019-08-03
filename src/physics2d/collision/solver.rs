use super::{
    super::{
        integrator::{Integrator, IntegratorState},
        rigidbody::RigidBody,
        CollisionEvent, Velocity,
    },
    broadphase::{BroadPhase, Collidable},
    narrowphase::intersection_check,
    Transform,
};
use crate::ecs::{event::EventQueue, system::*, IdType, Space};
use std::{collections::HashMap, marker::PhantomData};

use nalgebra::Vector2;

/// A System that calculates movement for rigid bodies
/// while taking collisions into account.
/// Integrators and broad phase algorithms are interchangeable.
pub struct CollisionSolver<I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    timestep: f32,
    iterations: usize,
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
    pub fn with_timestep(timestep: f32, iterations: usize) -> Self {
        CollisionSolver {
            timestep,
            iterations,
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
        // easy way to relate immutable collision pairs back to mutable items
        let id_index_map: HashMap<IdType, usize> = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id, index))
            .collect();

        let mut integrator = I::begin_step(self.timestep);

        while let IntegratorState::NeedsDerivatives = integrator.substep(
            items
                .iter_mut()
                .filter_map(|rbf| match rbf.body.velocity_mut() {
                    Some(vel) => Some((&mut *rbf.tr, vel)),
                    None => None,
                }),
        ) {
            let iter = items.iter().map(|rbf| Collidable {
                id: rbf.id,
                tr: rbf.tr,
                coll: &rbf.body.collider,
            });

            let mut events = Vec::new();

            let pairs = B::pairs(iter);
            let contacts: Vec<_> = pairs
                .iter()
                .map(|(o1, o2)| {
                    intersection_check(*o1, *o2)
                        .into_iter()
                        .map(move |c| (o1.id, o2.id, c))
                })
                .flatten()
                .collect();

            for _ in 0..self.iterations {
                for (o1_id, o2_id, contact) in contacts.iter() {
                    // every id is in the map so this can't fail
                    let i1 = *id_index_map.get(o1_id).unwrap();
                    let i2 = *id_index_map.get(o2_id).unwrap();
                    // ids guaranteed unequal -> we can do this trick to get mutable ref to both
                    let objs = if i1 < i2 {
                        let (l, r) = items.split_at_mut(i2);
                        [&mut l[i1], &mut r[0]]
                    } else {
                        let (l, r) = items.split_at_mut(i1);
                        [&mut r[0], &mut l[i2]]
                    };

                    if !objs[0].body.responds_to_collisions()
                        && !objs[1].body.responds_to_collisions()
                    {
                        // TODO: do this check before solving contacts
                        continue;
                    }

                    // begin actual collision process
                    let inv_mass = map_array_2(&objs, |o_| o_.body.inverse_mass());
                    let inv_mom_inertia =
                        map_array_2(&objs, |o_| o_.body.inverse_moment_of_inertia());

                    let force_offset =
                        map_array_2(&objs, |o_| contact.point - o_.tr.get_translation());

                    let offset_cross_normal = map_array_2(&force_offset, |offset| {
                        offset[0] * contact.normal[1] - contact.normal[0] * offset[1]
                    });

                    let vel = map_array_2(&objs, |o_| o_.body.velocity_or_zero());
                    let normal_vel = [
                        vel[0].linear.dot(&contact.normal)
                            + (offset_cross_normal[0] * vel[0].angular),
                        // normal is towards obj2 -> this one will be negative
                        // (if objects moving into each other)
                        vel[1].linear.dot(&contact.normal)
                            + (offset_cross_normal[1] * vel[1].angular),
                    ];

                    let relative_normal_vel = normal_vel[0] - normal_vel[1];
                    if relative_normal_vel < 0.0 {
                        continue;
                    }

                    let inv_mass_sum = inv_mass[0]
                        + (inv_mom_inertia[0] * offset_cross_normal[0] * offset_cross_normal[0])
                        + inv_mass[1]
                        + (inv_mom_inertia[1] * offset_cross_normal[1] * offset_cross_normal[1]);

                    let impulse_magnitude = relative_normal_vel / inv_mass_sum; // TODO: restitution -> bounce

                    // apply the impulse

                    objs[0].body.velocity_mut().map(|vel| {
                        vel.linear -= inv_mass[0] * impulse_magnitude * *contact.normal;
                        vel.angular -=
                            inv_mom_inertia[0] * impulse_magnitude * offset_cross_normal[0];
                    });
                    objs[1].body.velocity_mut().map(|vel| {
                        vel.linear += inv_mass[1] * impulse_magnitude * *contact.normal;
                        vel.angular +=
                            inv_mom_inertia[1] * impulse_magnitude * offset_cross_normal[1];
                    });
                }
            }

            // events
            // TODO: only generate these if listeners are present?
            for (o1, o2, contact) in &contacts {
                let evt1 = CollisionEvent {
                    source: *o1,
                    other: *o2,
                    normal: -contact.normal,
                    depth: contact.depth,
                    point: contact.point,
                };
                let evt2 = CollisionEvent {
                    source: *o2,
                    other: *o1,
                    normal: contact.normal,
                    depth: contact.depth,
                    point: contact.point - contact.depth * *contact.normal,
                };

                events.push(evt1);
                events.push(evt2);

                queue.push(Box::new(evt1));
                queue.push(Box::new(evt2));
            }

            // for visualization, TODO: remove when all collider types are done and shown to work
            space.write_global_state(|colls| {
                std::mem::replace(colls, events);
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
}

fn map_array_2<T, R>(arr: &[T; 2], mut f: impl FnMut(&T) -> R) -> [R; 2] {
    [f(&arr[0]), f(&arr[1])]
}
