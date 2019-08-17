use super::{
    super::{
        integrator::{Integrator, IntegratorState},
        rigidbody::RigidBody,
        CollisionEvent,
    },
    broadphase::{BroadPhase, Collidable},
    narrowphase::{intersection_check, Contact},
    Transform,
};
use crate::ecs::{event::EventQueue, system::*, IdType, Space};
use std::{collections::HashMap, marker::PhantomData};

const PROJECTION_AMOUNT: f32 = 0.4;

/// An intermediate structure that caches some information
/// during impulse resolution and allows undoing negative impulses at the end.
#[derive(Debug)]
struct ContactAccumulator {
    ids: [IdType; 2],
    indices: [usize; 2],
    contact: Contact,
    inv_masses: [f32; 2],
    inv_mom_inertias: [f32; 2],
    inv_masses_sum: f32,
    offsets_cross_normals: [f32; 2],
    total_impulse: f32,
}

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
        // apply environment forces before solving collisions
        for vel in items.iter_mut().filter_map(|item| item.body.velocity_mut()) {
            vel.linear[1] -= 250.0 * self.timestep; // TODO: unhack this
        }

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
            let iter = items.iter().map(|rbf| rbf.as_collidable());

            let mut events = Vec::new();

            let pairs = B::pairs(iter);
            let mut contacts = Vec::new();
            for ids in pairs {
                // every id is in the map so this can't fail
                let i = [
                    *id_index_map.get(&ids[0]).unwrap(),
                    *id_index_map.get(&ids[1]).unwrap(),
                ];
                // ids guaranteed unequal -> we can do this trick to get mutable ref to both
                let objs = if i[0] < i[1] {
                    let (l, r) = items.split_at_mut(i[1]);
                    [&mut l[i[0]], &mut r[0]]
                } else {
                    let (l, r) = items.split_at_mut(i[0]);
                    [&mut r[0], &mut l[i[1]]]
                };

                if !objs[0].body.responds_to_collisions() && !objs[1].body.responds_to_collisions()
                {
                    continue;
                }

                // initialize accumulators, cache some calculations we don't need to repeat per iteration
                for contact in intersection_check(objs[0].as_collidable(), objs[1].as_collidable())
                {
                    let force_offsets =
                        map_array_2(&objs, |o_| contact.point - o_.tr.get_translation());
                    let offsets_cross_normals = map_array_2(&force_offsets, |offset| {
                        offset[0] * contact.normal[1] - contact.normal[0] * offset[1]
                    });
                    let inv_masses = map_array_2(&objs, |o_| o_.body.inverse_mass());
                    let inv_mom_inertias =
                        map_array_2(&objs, |o_| o_.body.inverse_moment_of_inertia());
                    let inv_masses_sum = inv_masses[0]
                        + (inv_mom_inertias[0]
                            * offsets_cross_normals[0]
                            * offsets_cross_normals[0])
                        + inv_masses[1]
                        + (inv_mom_inertias[1]
                            * offsets_cross_normals[1]
                            * offsets_cross_normals[1]);
                    contacts.push(ContactAccumulator {
                        ids,
                        indices: i,
                        contact,
                        inv_masses,
                        inv_mom_inertias,
                        inv_masses_sum,
                        offsets_cross_normals,
                        total_impulse: 0.0,
                    });
                }
            }

            // iterative impulse accumulation (projected Gauss-Seidel)
            for _ in 0..self.iterations {
                for acc in contacts.iter_mut() {
                    let i = acc.indices;
                    let objs = if i[0] < i[1] {
                        let (l, r) = items.split_at_mut(i[1]);
                        [&mut l[i[0]], &mut r[0]]
                    } else {
                        let (l, r) = items.split_at_mut(i[0]);
                        [&mut r[0], &mut l[i[1]]]
                    };

                    let vels = map_array_2(&objs, |o_| o_.body.velocity_or_zero());
                    let normal_vels = [
                        vels[0].linear.dot(&acc.contact.normal)
                            + (acc.offsets_cross_normals[0] * vels[0].angular),
                        // normal is towards obj2 -> this one will be negative
                        // (if objects moving into each other)
                        vels[1].linear.dot(&acc.contact.normal)
                            + (acc.offsets_cross_normals[1] * vels[1].angular),
                    ];

                    let relative_normal_vel = normal_vels[0] - normal_vels[1];

                    let impulse_magnitude = relative_normal_vel / acc.inv_masses_sum;

                    // apply the impulse

                    acc.total_impulse += impulse_magnitude;

                    if let Some(vel) = objs[0].body.velocity_mut() {
                        vel.linear -= acc.inv_masses[0] * impulse_magnitude * *acc.contact.normal;
                        vel.angular -= acc.inv_mom_inertias[0]
                            * impulse_magnitude
                            * acc.offsets_cross_normals[0];
                    }
                    if let Some(vel) = objs[1].body.velocity_mut() {
                        vel.linear += acc.inv_masses[1] * impulse_magnitude * *acc.contact.normal;
                        vel.angular += acc.inv_mom_inertias[1]
                            * impulse_magnitude
                            * acc.offsets_cross_normals[1];
                    }
                }
            }

            // final cleaning up
            for acc in contacts.iter() {
                let i = acc.indices;
                let objs = if i[0] < i[1] {
                    let (l, r) = items.split_at_mut(i[1]);
                    [&mut l[i[0]], &mut r[0]]
                } else {
                    let (l, r) = items.split_at_mut(i[0]);
                    [&mut r[0], &mut l[i[1]]]
                };

                // position projection
                let proj = acc.contact.depth * PROJECTION_AMOUNT * *acc.contact.normal;
                match map_array_2(&objs, |o_| o_.body.responds_to_collisions()) {
                    [true, true] => {
                        objs[0].tr.translate(-0.5 * proj);
                        objs[1].tr.translate(0.5 * proj);
                    }
                    [true, false] => objs[0].tr.translate(-proj),
                    [false, true] => objs[1].tr.translate(proj),
                    [false, false] => (),
                }

                // revert negative impulses
                if acc.total_impulse >= 0.0 {
                    continue;
                }
                if let Some(vel) = objs[0].body.velocity_mut() {
                    vel.linear -= acc.inv_masses[0] * -acc.total_impulse * *acc.contact.normal;
                    vel.angular -=
                        acc.inv_mom_inertias[0] * -acc.total_impulse * acc.offsets_cross_normals[0];
                }
                if let Some(vel) = objs[1].body.velocity_mut() {
                    vel.linear += acc.inv_masses[1] * -acc.total_impulse * *acc.contact.normal;
                    vel.angular +=
                        acc.inv_mom_inertias[1] * -acc.total_impulse * acc.offsets_cross_normals[1];
                }
            }

            // events
            // TODO: only generate these if listeners are present?
            for ContactAccumulator { ids, contact, .. } in &contacts {
                let evt1 = CollisionEvent {
                    source: ids[0],
                    other: ids[1],
                    normal: -contact.normal,
                    depth: contact.depth,
                    point: contact.point,
                };
                let evt2 = CollisionEvent {
                    source: ids[1],
                    other: ids[0],
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

impl<'a> RigidBodyFilter<'a> {
    pub(self) fn as_collidable(&'a self) -> Collidable<'a> {
        Collidable {
            id: self.id,
            tr: self.tr,
            coll: &self.body.collider,
        }
    }
}

fn map_array_2<T, R>(arr: &[T; 2], mut f: impl FnMut(&T) -> R) -> [R; 2] {
    [f(&arr[0]), f(&arr[1])]
}
