use super::{
    super::{
        forcefield::ForceField,
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

/// Condition to stop iterating on the collision solver.
/// Ends either when converging close enough to the actual solution (`convergence_threshold`)
/// or after the given maximum number of loops, whichever comes first.
#[derive(Clone, Copy)]
pub struct SolverLoopCondition {
    pub convergence_threshold: f32,
    pub max_loops: usize,
}

impl SolverLoopCondition {
    /// Create a loop condition and set the converge threshold to zero.
    /// Effectively means `max_loops` number of loops every update.
    pub fn from_max_loops(max_loops: usize) -> Self {
        SolverLoopCondition {
            convergence_threshold: 0.0,
            max_loops,
        }
    }
}

/// A container to store impulses across updates,
/// used for warm starting the solver algorithm.
pub struct ImpulseCache(HashMap<[IdType; 2], f32>);

impl ImpulseCache {
    pub fn new() -> Self {
        ImpulseCache(HashMap::new())
    }

    pub(self) fn get(&self, ids: [IdType; 2]) -> Option<&f32> {
        if ids[0] < ids[1] {
            self.0.get(&ids)
        } else {
            self.0.get(&[ids[1], ids[0]])
        }
    }

    pub(self) fn replace<'a>(&mut self, items: impl IntoIterator<Item = &'a ContactAccumulator>) {
        self.0 = items
            .into_iter()
            .map(|acc| {
                let ids = if acc.ids[0] < acc.ids[1] {
                    acc.ids
                } else {
                    [acc.ids[1], acc.ids[0]]
                };
                (ids, acc.total_impulse)
            })
            .collect();
    }
}

impl Default for ImpulseCache {
    fn default() -> Self {
        Self::new()
    }
}

/// A System that calculates movement for rigid bodies
/// while taking collisions into account.
/// Integrators and broad phase algorithms can be selected using type parameters.
pub struct CollisionSolver<'a, I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    timestep: f32,
    cache: &'a mut ImpulseCache,
    loop_condition: SolverLoopCondition,
    forcefield: ForceField,
    _integrator_marker: PhantomData<I>,
    _broad_phase_marker: PhantomData<B>,
}

impl<'a, I, B> CollisionSolver<'a, I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    pub fn new(
        timestep: f32,
        cache: &'a mut ImpulseCache,
        loop_condition: SolverLoopCondition,
        ff: impl Into<ForceField>,
    ) -> Self {
        CollisionSolver {
            timestep,
            cache,
            loop_condition,
            forcefield: ff.into(),
            _integrator_marker: PhantomData,
            _broad_phase_marker: PhantomData,
        }
    }
}

impl<'a, I, B> System<'a> for CollisionSolver<'a, I, B>
where
    I: Integrator,
    B: BroadPhase,
{
    type Query = RigidBodyQuery<'a>;

    fn run_system(self, items: &mut [Self::Query], _space: &Space, queue: &mut EventQueue) {
        // apply environment forces before solving collisions
        for item in items.iter_mut() {
            if let Some(vel) = item.body.velocity_mut() {
                vel.linear += self.forcefield.value_at(item.tr.position()) * self.timestep;
            }
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

            let pairs = B::pairs(iter);
            let mut contacts = Vec::new();
            for ids in pairs {
                assert!(
                    ids[0] != ids[1],
                    "Broad phase bug: paired an object with itself"
                );
                // every id is in the map so this can't fail
                let indices = [
                    *id_index_map.get(&ids[0]).unwrap(),
                    *id_index_map.get(&ids[1]).unwrap(),
                ];

                // sort by index so the following doesn't need comparison
                let (ids, indices) = if indices[0] < indices[1] {
                    (ids, indices)
                } else {
                    ([ids[1], ids[0]], [indices[1], indices[0]])
                };
                // ids guaranteed unequal -> we can do this trick to get mutable ref to both
                let objs = {
                    let (l, r) = items.split_at_mut(indices[1]);
                    [&mut l[indices[0]], &mut r[0]]
                };

                if !objs[0].body.responds_to_collisions() && !objs[1].body.responds_to_collisions()
                {
                    continue;
                }

                // initialize accumulators, cache some calculations we don't need to repeat per iteration
                for contact in intersection_check(objs[0].as_collidable(), objs[1].as_collidable())
                {
                    let force_offsets =
                        map_array_2(&objs, |o_| contact.point - o_.tr.translation());
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

                    // warm start
                    let initial_impulse = if let Some(prev_impulse) = self.cache.get(ids) {
                        if let Some(vel) = objs[0].body.velocity_mut() {
                            vel.linear -= inv_masses[0] * prev_impulse * *contact.normal;
                            vel.angular -=
                                inv_mom_inertias[0] * prev_impulse * offsets_cross_normals[0];
                        }
                        if let Some(vel) = objs[1].body.velocity_mut() {
                            vel.linear += inv_masses[1] * prev_impulse * *contact.normal;
                            vel.angular +=
                                inv_mom_inertias[1] * prev_impulse * offsets_cross_normals[1];
                        }
                        *prev_impulse
                    } else {
                        0.0
                    };

                    contacts.push(ContactAccumulator {
                        ids,
                        indices,
                        contact,
                        inv_masses,
                        inv_mom_inertias,
                        inv_masses_sum,
                        offsets_cross_normals,
                        total_impulse: initial_impulse,
                    });
                }
            }

            // iterative impulse accumulation
            let mut biggest_change = std::f32::MAX;
            let mut loop_count = 0;
            while biggest_change > self.loop_condition.convergence_threshold
                && loop_count < self.loop_condition.max_loops
            {
                loop_count += 1;
                biggest_change = 0.0;

                for acc in contacts.iter_mut() {
                    let objs = {
                        let (l, r) = items.split_at_mut(acc.indices[1]);
                        [&mut l[acc.indices[0]], &mut r[0]]
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
                    biggest_change = biggest_change.max(impulse_magnitude.abs());

                    // clamp total accumulated to 0 (individual impulse can be negative)
                    let new_total = acc.total_impulse + impulse_magnitude;
                    let clamped_impulse = if new_total < 0.0 {
                        acc.total_impulse = 0.0;
                        impulse_magnitude - new_total
                    } else {
                        acc.total_impulse = new_total;
                        impulse_magnitude
                    };

                    // apply the impulse
                    if let Some(vel) = objs[0].body.velocity_mut() {
                        vel.linear -= acc.inv_masses[0] * clamped_impulse * *acc.contact.normal;
                        vel.angular -= acc.inv_mom_inertias[0]
                            * clamped_impulse
                            * acc.offsets_cross_normals[0];
                    }
                    if let Some(vel) = objs[1].body.velocity_mut() {
                        vel.linear += acc.inv_masses[1] * clamped_impulse * *acc.contact.normal;
                        vel.angular += acc.inv_mom_inertias[1]
                            * clamped_impulse
                            * acc.offsets_cross_normals[1];
                    }
                }
            }

            // position projection
            for acc in contacts.iter() {
                let objs = {
                    let (l, r) = items.split_at_mut(acc.indices[1]);
                    [&mut l[acc.indices[0]], &mut r[0]]
                };

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
            }

            // store impulses for next frame's warm start
            self.cache.replace(&contacts);

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

                queue.push(Box::new(evt1));
                queue.push(Box::new(evt2));
            }
        }
    }
}

#[derive(ComponentQuery)]
pub struct RigidBodyQuery<'a> {
    #[id]
    id: IdType,
    tr: &'a mut Transform,
    body: &'a mut RigidBody,
}

impl<'a> RigidBodyQuery<'a> {
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
