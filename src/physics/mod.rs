use crate::core::{
    container::{ContainerInit, IterSeed},
    space::{CreationId, Id},
    storage, Container, Transform, TransformFeature,
};
use std::collections::HashMap;

//

pub mod collision;
pub use collision::{Collider, ColliderShape};

pub mod forcefield;
pub use forcefield::ForceField;

pub mod rigidbody;
pub use rigidbody::RigidBody;

//

use crate::core::math as m;
use nalgebra as na;

#[derive(Copy, Clone, Debug)]
pub struct Velocity {
    /// Linear velocity in metres per second.
    pub linear: m::Vec2,
    /// Angular velocity in radians per second.
    pub angular: f32,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: m::Vec2::zeros(),
            angular: 0.0,
        }
    }
}

impl Velocity {
    /// Get the linear velocity of a point offset from the center of mass.
    pub fn point_velocity(&self, offset: m::Vec2) -> m::Vec2 {
        let tangent = m::Vec2::new(-offset[1], offset[0]) * self.angular;
        self.linear + tangent
    }
}

/// Events produced by the physics system when two physics objects collide.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    pub source: Id,
    pub other: Id,
    pub info: ContactInfo,
}

/// Detailed information about a contact event.
#[derive(Clone, Copy, Debug)]
pub struct ContactInfo {
    /// The point in world space where the collision occurred.
    pub point: m::Point2,
    /// The normal of the colliding plane, facing towards this object.
    pub normal: na::Unit<m::Vec2>,
    /// The strength of the impulse caused by the contact.
    pub impulse: f32,
}

/// Everything you need to have rigid body physics in your Space.
/// Put one of these inside your FeatureSet.
///
/// Note: the containers for `bodies` and `colliders` are `pub` for easy access in game logic.
/// You should probably not insert directly to them; use this feature's methods instead to get
/// objects with a guaranteed well-defined meaning.
///
/// TODOC: usage example
pub struct PhysicsFeature {
    pub bodies: Container<storage::DenseVecStorage<RigidBody>>,
    pub colliders: Container<storage::DenseVecStorage<Collider>>,
    impulse_cache: ImpulseCache,
    stabilisation_coef: f32,
    loop_condition: SolverLoopCondition,
}

impl PhysicsFeature {
    pub fn new(init: ContainerInit) -> Self {
        PhysicsFeature {
            bodies: Container::new(init),
            colliders: Container::new(init),
            impulse_cache: ImpulseCache::new(),
            stabilisation_coef: 0.1,
            loop_condition: SolverLoopCondition {
                max_loops: 10,
                convergence_threshold: 0.05,
            },
        }
    }

    /// Replace the default constraint bias coefficient used in Baumgarte stabilisation.
    /// The default value is good for most purposes.
    ///
    /// Lower values resolve penetration more slowly and vice versa.
    pub fn with_stabilisation_coef(mut self, sc: f32) -> Self {
        self.stabilisation_coef = sc;
        self
    }

    /// Replace the default condition to stop the constraint solver loop.
    pub fn with_loop_condition(mut self, cond: SolverLoopCondition) -> Self {
        self.loop_condition = cond;
        self
    }

    /// Add a rigid body component to an object.
    pub fn add_body(&mut self, id: CreationId, body: RigidBody, collider: Collider) {
        self.bodies.insert(id, body);
        self.colliders.insert(id, collider);
    }

    /// Detect collisions, solve constraint forces and move bodies.
    /// Call this once in your `FeatureSet`'s `tick` function.
    pub fn tick(
        &mut self,
        iter_seed: IterSeed,
        trs: &mut TransformFeature,
        dt: f32,
        forcefield: Option<&impl ForceField>,
    ) -> Vec<ContactEvent> {
        struct Item<'a> {
            body: &'a mut RigidBody,
            coll: &'a Collider,
            tr: &'a mut Transform,
            id: Id,
        }
        let mut items: Vec<Item<'_>> = iter_seed
            .overlay(self.bodies.iter_mut())
            .and(self.colliders.iter())
            .and(trs.iter_mut())
            .with_ids()
            .into_iter()
            .map(|(((body, coll), tr), id)| Item { body, coll, tr, id })
            .collect();

        //
        // Detect collisions
        //

        use collision::BroadPhase;
        let pairs = collision::broadphase::BruteForce::pairs(
            items
                .iter()
                .map(|Item { tr, coll, .. }| collision::BodyRef { tr, coll }),
        );
        let mut contact_constraints = Vec::new();
        for [i1, i2] in pairs {
            let objs = [&items[i1], &items[i2]];
            if objs[0].body.responds_to_collisions() || objs[1].body.responds_to_collisions() {
                let contacts = collision::narrowphase::intersection_check(
                    collision::BodyRef {
                        tr: objs[0].tr,
                        coll: objs[0].coll,
                    },
                    collision::BodyRef {
                        tr: objs[1].tr,
                        coll: objs[1].coll,
                    },
                );
                for contact in contacts {
                    contact_constraints.push(WorkingConstraint {
                        indices: [i1, i2],
                        normal: contact.normal,
                        point: contact.point,
                        offsets: contact.offsets,
                        impulse_bounds: (Some(0.0), None),
                        bias: contact.depth * (1.0 / dt) * self.stabilisation_coef,
                    });
                }
            }
        }

        //
        // Solve constraints
        //

        // TODO: also allow other constraints in the world
        let constraints = contact_constraints;

        // apply environment forces (gravity, usually)
        if let Some(ff) = forcefield {
            for obj in items.iter_mut() {
                if let Some(vel) = obj.body.velocity_mut() {
                    vel.linear += ff.value_at(obj.tr.isometry.translation.vector.into()) * dt;
                }
            }
        }

        fn map_array_2<T, R>(arr: &[T; 2], mut f: impl FnMut(&T) -> R) -> [R; 2] {
            [f(&arr[0]), f(&arr[1])]
        }

        let mut accumulators = Vec::new();
        // Initialize accumulators
        for constraint in constraints {
            assert!(
                constraint.indices[0] != constraint.indices[1],
                "bug: paired an object with itself"
            );

            // objects guaranteed not the same -> we can do this trick to get mutable ref to both
            let objs = {
                let (l, r) = items.split_at_mut(constraint.indices[1]);
                [&mut l[constraint.indices[0]], &mut r[0]]
            };
            let ids = [objs[0].id, objs[1].id];

            // begin accumulator construction
            let offsets_cross_normals = map_array_2(&constraint.offsets, |offset| {
                offset[0] * constraint.normal[1] - constraint.normal[0] * offset[1]
            });
            let inv_masses = map_array_2(&objs, |o_| o_.body.inverse_mass());
            let inv_mom_inertias = map_array_2(&objs, |o_| o_.body.inverse_moment_of_inertia());
            let inv_masses_sum = inv_masses[0]
                + (inv_mom_inertias[0] * offsets_cross_normals[0] * offsets_cross_normals[0])
                + inv_masses[1]
                + (inv_mom_inertias[1] * offsets_cross_normals[1] * offsets_cross_normals[1]);

            // warm start
            let initial_impulse = if let Some(prev_impulse) = self.impulse_cache.get(ids) {
                if let Some(vel) = objs[0].body.velocity_mut() {
                    vel.linear -= inv_masses[0] * prev_impulse * (*constraint.normal);
                    vel.angular -= inv_mom_inertias[0] * prev_impulse * offsets_cross_normals[0];
                }
                if let Some(vel) = objs[1].body.velocity_mut() {
                    vel.linear += inv_masses[1] * prev_impulse * (*constraint.normal);
                    vel.angular += inv_mom_inertias[1] * prev_impulse * offsets_cross_normals[1];
                }
                *prev_impulse
            } else {
                0.0
            };

            accumulators.push(ConstraintAccumulator {
                constraint,
                ids,
                inv_masses,
                inv_mom_inertias,
                inv_masses_sum,
                offsets_cross_normals,
                total_impulse: initial_impulse,
            });
        }

        // iterative impulse accumulation
        let mut biggest_change = std::f32::MAX;
        let mut loop_count = 0;
        while biggest_change > self.loop_condition.convergence_threshold
            && loop_count < self.loop_condition.max_loops
        {
            loop_count += 1;
            biggest_change = 0.0;

            for acc in accumulators.iter_mut() {
                let objs = {
                    let (l, r) = items.split_at_mut(acc.constraint.indices[1]);
                    [&mut l[acc.constraint.indices[0]], &mut r[0]]
                };

                let vels = map_array_2(&objs, |o_| o_.body.velocity_or_zero());
                // TODO: this part is the actual constraint function and should be generalized
                let normal_vels = [
                    vels[0].linear.dot(&acc.constraint.normal)
                        + (acc.offsets_cross_normals[0] * vels[0].angular),
                    vels[1].linear.dot(&acc.constraint.normal)
                        + (acc.offsets_cross_normals[1] * vels[1].angular),
                ];

                let relative_normal_vel = normal_vels[0] - normal_vels[1] + acc.constraint.bias;

                let impulse_magnitude = relative_normal_vel / acc.inv_masses_sum;
                biggest_change = biggest_change.max(impulse_magnitude.abs());

                // clamp total accumulated to the constraint's bounds
                let new_total = acc.total_impulse + impulse_magnitude;
                let clamped_impulse = match acc.constraint.impulse_bounds {
                    (Some(lo), _) if new_total < lo => {
                        acc.total_impulse = lo;
                        impulse_magnitude - new_total
                    }
                    (_, Some(hi)) if new_total > hi => {
                        acc.total_impulse = hi;
                        impulse_magnitude - new_total
                    }
                    _ => {
                        acc.total_impulse = new_total;
                        impulse_magnitude
                    }
                };

                // apply the impulse
                if let Some(vel) = objs[0].body.velocity_mut() {
                    vel.linear -= acc.inv_masses[0] * clamped_impulse * (*acc.constraint.normal);
                    vel.angular -=
                        acc.inv_mom_inertias[0] * clamped_impulse * acc.offsets_cross_normals[0];
                }
                if let Some(vel) = objs[1].body.velocity_mut() {
                    vel.linear += acc.inv_masses[1] * clamped_impulse * (*acc.constraint.normal);
                    vel.angular +=
                        acc.inv_mom_inertias[1] * clamped_impulse * acc.offsets_cross_normals[1];
                }
            }
        }

        //
        // Post-solve bookkeeping
        //

        // push events
        let mut events = Vec::new();
        for acc in &accumulators {
            events.push(ContactEvent {
                source: acc.ids[0],
                other: acc.ids[1],
                info: ContactInfo {
                    point: acc.constraint.point,
                    normal: -acc.constraint.normal,
                    impulse: acc.total_impulse,
                },
            });
            events.push(ContactEvent {
                source: acc.ids[1],
                other: acc.ids[0],
                info: ContactInfo {
                    point: acc.constraint.point,
                    normal: acc.constraint.normal,
                    impulse: acc.total_impulse,
                },
            });
        }
        // store impulses for next frame's warm start
        self.impulse_cache.replace(&accumulators);

        //
        // Apply movement
        //

        // semi-implicit Euler integration: use velocities at the end of the time step
        for obj in items {
            if let Some(vel) = obj.body.velocity() {
                obj.tr.append_translation_mut(&(dt * vel.linear).into());
                obj.tr
                    .append_rotation_wrt_center_mut(&m::Angle::Rad(dt * vel.angular).into());
            }
        }

        events
    }
}

// TODO: WorkingConstraint and ConstraintAccumulator should be one and the same
// and we should have another Constraint type that's a general constraint that can be added manually.
#[derive(Clone, Copy, Debug)]
struct WorkingConstraint {
    indices: [usize; 2],
    normal: na::Unit<m::Vec2>,
    point: m::Point2,
    offsets: [m::Vec2; 2],
    impulse_bounds: (Option<f32>, Option<f32>),
    bias: f32,
}

#[derive(Debug)]
struct ConstraintAccumulator {
    constraint: WorkingConstraint,
    ids: [Id; 2],
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
pub struct ImpulseCache(HashMap<[Id; 2], f32>);

impl ImpulseCache {
    pub fn new() -> Self {
        ImpulseCache(HashMap::new())
    }

    pub(self) fn get(&self, ids: [Id; 2]) -> Option<&f32> {
        if ids[0].0 < ids[1].0 {
            self.0.get(&ids)
        } else {
            self.0.get(&[ids[1], ids[0]])
        }
    }

    pub(self) fn replace<'a>(
        &mut self,
        items: impl IntoIterator<Item = &'a ConstraintAccumulator>,
    ) {
        self.0 = items
            .into_iter()
            .map(|acc| {
                let ids = if acc.ids[0].0 < acc.ids[1].0 {
                    acc.ids
                } else {
                    [acc.ids[1], acc.ids[0]]
                };
                (ids, acc.total_impulse)
            })
            .collect();
    }
}
