use crate::core::{graph, Transform};
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
    pub source_body: graph::NodePosition,
    pub other_body: graph::NodePosition,
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

pub struct PhysicsSolver {
    impulse_cache: ImpulseCache,
    stabilisation_coef: f32,
    loop_condition: SolverLoopCondition,
}

const BAD_NODE_ERR: &'static str = "Broad phase returned a graph node that does not exist";

impl PhysicsSolver {
    pub fn new() -> Self {
        PhysicsSolver {
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

    /// Detect collisions, solve constraint forces and move bodies.
    /// Call this once in your `FeatureSet`'s `tick` function.
    pub fn tick(
        &mut self,
        graph: &graph::Graph,
        l_transform: &mut graph::Layer<Transform>,
        l_body: &mut graph::Layer<RigidBody>,
        l_collider: &graph::Layer<Collider>,
        dt: f32,
        forcefield: Option<&impl ForceField>,
    ) -> Vec<ContactEvent> {
        //
        // Detect collisions
        //

        let body_ref_iter = l_body.iter().filter_map(|rb| {
            let coll = graph.get_neighbor(&rb, &l_collider)?;
            let tr = graph.get_neighbor(&rb, &l_transform)?;
            Some(BodyRef {
                tr,
                coll,
                rb_pos: graph::NodePosition::from(&rb),
            })
        });

        use collision::BroadPhase;
        let pairs = collision::broadphase::BruteForce::pairs(body_ref_iter);
        let mut contact_constraints = Vec::new();
        for pair in pairs {
            if (l_body.get(pair[0].rb).expect(BAD_NODE_ERR)).responds_to_collisions()
                || (l_body.get(pair[1].rb).expect(BAD_NODE_ERR)).responds_to_collisions()
            {
                let contacts = collision::narrowphase::intersection_check(
                    pair[0].upgrade(l_transform, l_collider),
                    pair[1].upgrade(l_transform, l_collider),
                );
                for contact in contacts {
                    contact_constraints.push(WorkingConstraint {
                        nodes: pair,
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
            for mut rb in l_body.iter_mut() {
                if let (Some(tr), Some(vel)) =
                    (graph.get_neighbor(&rb, &l_transform), rb.velocity_mut())
                {
                    vel.linear += ff.value_at(tr.isometry.translation.vector.into()) * dt;
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
                constraint.nodes[0].rb != constraint.nodes[1].rb,
                "bug: paired a body with itself"
            );

            let bodies = map_array_2(&constraint.nodes, |n| l_body.get(n.rb).expect(BAD_NODE_ERR));
            // begin accumulator construction
            let offsets_cross_normals = map_array_2(&constraint.offsets, |offset| {
                offset[0] * constraint.normal[1] - constraint.normal[0] * offset[1]
            });
            let inv_masses = map_array_2(&bodies, |rb| rb.inverse_mass());
            let inv_mom_inertias = map_array_2(&bodies, |rb| rb.inverse_moment_of_inertia());
            let inv_masses_sum = inv_masses[0]
                + (inv_mom_inertias[0] * offsets_cross_normals[0] * offsets_cross_normals[0])
                + inv_masses[1]
                + (inv_mom_inertias[1] * offsets_cross_normals[1] * offsets_cross_normals[1]);

            // warm start
            let initial_impulse = if let Some(prev_impulse) = self
                .impulse_cache
                .get(constraint.nodes[0].cache_id(constraint.nodes[1]))
            {
                if let Some(vel) = l_body
                    .get_mut(constraint.nodes[0].rb)
                    .expect(BAD_NODE_ERR)
                    .velocity_mut()
                {
                    vel.linear -= inv_masses[0] * prev_impulse * (*constraint.normal);
                    vel.angular -= inv_mom_inertias[0] * prev_impulse * offsets_cross_normals[0];
                }
                if let Some(vel) = l_body
                    .get_mut(constraint.nodes[1].rb)
                    .expect(BAD_NODE_ERR)
                    .velocity_mut()
                {
                    vel.linear += inv_masses[1] * prev_impulse * (*constraint.normal);
                    vel.angular += inv_mom_inertias[1] * prev_impulse * offsets_cross_normals[1];
                }
                *prev_impulse
            } else {
                0.0
            };

            accumulators.push(ConstraintAccumulator {
                constraint,
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
                let bodies = map_array_2(&acc.constraint.nodes, |n| {
                    l_body.get(n.rb).expect(BAD_NODE_ERR)
                });
                let vels = map_array_2(&bodies, |rb| rb.velocity_or_zero());
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
                if let Some(vel) = l_body
                    .get_mut(acc.constraint.nodes[0].rb)
                    .expect(BAD_NODE_ERR)
                    .velocity_mut()
                {
                    vel.linear -= acc.inv_masses[0] * clamped_impulse * (*acc.constraint.normal);
                    vel.angular -=
                        acc.inv_mom_inertias[0] * clamped_impulse * acc.offsets_cross_normals[0];
                }
                if let Some(vel) = l_body
                    .get_mut(acc.constraint.nodes[1].rb)
                    .expect(BAD_NODE_ERR)
                    .velocity_mut()
                {
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
                source_body: acc.constraint.nodes[0].rb,
                other_body: acc.constraint.nodes[1].rb,
                info: ContactInfo {
                    point: acc.constraint.point,
                    normal: -acc.constraint.normal,
                    impulse: acc.total_impulse,
                },
            });
            events.push(ContactEvent {
                source_body: acc.constraint.nodes[1].rb,
                other_body: acc.constraint.nodes[0].rb,
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
        for rb in l_body.iter() {
            if let (Some(mut tr), Some(vel)) =
                (graph.get_neighbor_mut(&rb, l_transform), rb.velocity())
            {
                tr.append_translation_mut(&(dt * vel.linear).into());
                tr.append_rotation_wrt_center_mut(&m::Angle::Rad(dt * vel.angular).into());
            }
        }

        events
    }
}

// TODO: WorkingConstraint and ConstraintAccumulator should be one and the same
// and we should have another Constraint type that's a general constraint that can be added manually.
#[derive(Clone, Copy, Debug)]
struct WorkingConstraint {
    nodes: [BodyNodes; 2],
    normal: na::Unit<m::Vec2>,
    point: m::Point2,
    offsets: [m::Vec2; 2],
    impulse_bounds: (Option<f32>, Option<f32>),
    bias: f32,
}

#[derive(Debug)]
struct ConstraintAccumulator {
    constraint: WorkingConstraint,
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
pub struct ImpulseCache(HashMap<[usize; 2], f32>);

impl ImpulseCache {
    pub fn new() -> Self {
        ImpulseCache(HashMap::new())
    }

    pub(self) fn get(&self, ids: [usize; 2]) -> Option<&f32> {
        if ids[0] < ids[1] {
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
                let ids = acc.constraint.nodes[0].cache_id(acc.constraint.nodes[1]);
                let ids = if ids[0] < ids[1] {
                    ids
                } else {
                    [ids[1], ids[0]]
                };
                (ids, acc.total_impulse)
            })
            .collect();
    }
}

/// References to the parts of a body that we need to find out if it collides with anything.
/// Used internally in collisiion detection, exposed to allow custom broad phase algorithms.
pub struct BodyRef<'a> {
    pub tr: graph::NodeRef<'a, Transform>,
    pub coll: graph::NodeRef<'a, Collider>,
    pub(crate) rb_pos: graph::NodePosition,
}

/// A non-reference version of `BodyRef`, to allow for multiple mutable references
/// from the same graph layer during one iteration.
#[derive(Clone, Copy, Debug)]
pub struct BodyNodes {
    pub(crate) tr: graph::NodePosition,
    pub(crate) coll: graph::NodePosition,
    pub(crate) rb: graph::NodePosition,
}
impl From<&BodyRef<'_>> for BodyNodes {
    fn from(br: &BodyRef<'_>) -> Self {
        BodyNodes {
            tr: graph::NodePosition::from(&br.tr),
            coll: graph::NodePosition::from(&br.coll),
            rb: br.rb_pos,
        }
    }
}
impl BodyNodes {
    fn upgrade<'a>(
        self,
        l_tr: &'a graph::Layer<Transform>,
        l_coll: &'a graph::Layer<Collider>,
    ) -> BodyRef<'a> {
        BodyRef {
            tr: l_tr.get(self.tr).expect("A BodyNodes was malformed"),
            coll: l_coll.get(self.coll).expect("A BodyNodes was malformed"),
            rb_pos: self.rb,
        }
    }

    fn cache_id(&self, other: BodyNodes) -> [usize; 2] {
        [self.rb.item_idx, other.rb.item_idx]
    }
}
