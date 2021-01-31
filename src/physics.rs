use crate::{
    graph::{self, NodeRef, UnsafeNode},
    math::{self, uv, Angle, Unit},
};

use itertools::izip;
use slotmap as sm;
use std::collections::HashMap;

//

pub mod collision;
pub use collision::{Collider, ColliderShape, Contact};

pub mod constraint;
use constraint::{
    cache::{ConstraintId, DynamicConstraintId, DynamicConstraintType, ImpulseCache},
    func::ConstraintFunction,
    ConstraintSoftnessType, ImpulseBounds, WorkingConstraint,
};
pub use constraint::{
    Constraint, ConstraintBuilder, OscillatorParams, SolverConvergence, SolverParams,
};

pub mod forcefield;
pub use forcefield::ForceField;

pub mod rigidbody;
pub use rigidbody::RigidBody;

//

#[derive(Copy, Clone, Debug)]
pub struct Velocity {
    /// Linear velocity in metres per second.
    pub linear: uv::Vec2,
    /// Angular velocity in radians per second.
    pub angular: f32,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: uv::Vec2::zero(),
            angular: 0.0,
        }
    }
}

impl Velocity {
    /// Get the linear velocity of a point offset from the center of mass.
    pub fn point_velocity(&self, offset: uv::Vec2) -> uv::Vec2 {
        let tangent = uv::Vec2::new(-offset[1], offset[0]) * self.angular;
        self.linear + tangent
    }
}

impl std::ops::Add for Velocity {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            linear: self.linear + other.linear,
            angular: self.angular + other.angular,
        }
    }
}
impl std::ops::AddAssign for Velocity {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

/// Events produced by the physics system when two physics objects collide.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    pub other_body: graph::Node<RigidBody>,
    pub info: ContactInfo,
}

/// Detailed information about a contact event.
#[derive(Clone, Copy, Debug)]
pub struct ContactInfo {
    /// The point in world space where the collision occurred.
    pub point: uv::Vec2,
    /// The normal of the colliding plane, facing towards this object.
    pub normal: Unit<uv::Vec2>,
    /// The strength of the impulse caused by the contact.
    pub impulse: f32,
}

sm::new_key_type! {
    pub struct ConstraintHandle;
}

/// Allows a small amount of position error on constraints so that objects don't
/// e.g. shake on the ground due to being pushed out and back in constantly
const SLOP_LIMIT: f32 = 0.01;

pub struct Physics {
    stabilisation_coef: f32,
    solver_params: SolverParams,
    impulse_cache: ImpulseCache,
    user_constraints: sm::DenseSlotMap<ConstraintHandle, Constraint>,
}

impl Physics {
    pub fn new() -> Self {
        Physics {
            stabilisation_coef: 0.1,
            solver_params: SolverParams::default(),
            impulse_cache: ImpulseCache::new(),
            user_constraints: sm::DenseSlotMap::with_key(),
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

    /// Replace the default parameters for the constraint solver.
    pub fn with_solver_params(mut self, params: SolverParams) -> Self {
        self.solver_params = params;
        self
    }

    /// Add a user-defined constraint to the system. Returns a handle that can be used to remove it later.
    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintHandle {
        self.user_constraints.insert(constraint)
    }

    /// Remove a constraint from the system. Returns the constraint if it still existed.
    ///
    /// Constraints can also disappear on their own if the objects they're associated with
    /// are destroyed, so it's not guaranteed the constraint will exist
    /// even if it hasn't been explicitly removed before.
    pub fn remove_constraint(&mut self, handle: ConstraintHandle) -> Option<Constraint> {
        self.user_constraints.remove(handle)
    }

    /// Remove all constraints and state.
    pub fn reset(&mut self) {
        self.impulse_cache.clear();
        self.user_constraints.clear();
    }

    /// Detect collisions, solve constraint forces and move bodies.
    pub fn tick<EvtParams>(
        &mut self,
        graph: &graph::Graph,
        l_transform: &mut graph::Layer<uv::Isometry2>,
        l_body: &mut graph::Layer<RigidBody>,
        l_collider: &graph::Layer<Collider>,
        l_evt_sink: &mut crate::EventSinkLayer<EvtParams>,
        dt: f32,
        forcefield: &impl ForceField,
    ) {
        // apply environment forces (gravity, usually)
        for mut rb in l_body.iter_mut(graph) {
            if let (Some(ref tr), rigidbody::BodyType::Dynamic { velocity, .. }) =
                (graph.get_neighbor(&rb, &l_transform), &mut rb.body)
            {
                velocity.linear += forcefield.value_at(tr.translation) * dt;
            }
        }

        let body_refs: Vec<BodyRef> = l_body
            .iter(graph)
            .filter_map(|rb| {
                let coll = graph.get_neighbor(&rb, &l_collider)?;
                let tr = graph.get_neighbor(&rb, &l_transform)?;
                Some(BodyRef { tr, coll, rb })
            })
            .collect();
        // map from the position of a node in the layer to the position of a node in body_refs
        let node_ref_map: HashMap<usize, usize> = body_refs
            .iter()
            .enumerate()
            .map(|(idx, br)| (br.rb.pos().item_idx, idx))
            .collect();

        // this will be modified directly by the solver and we will map the changes back to the originals at the end
        let mut velocities: Vec<Velocity> = body_refs
            .iter()
            .map(|br| br.rb.velocity_or_zero())
            .collect();
        let inv_masses: Vec<uv::Vec2> = body_refs
            .iter()
            .map(|br| uv::Vec2::new(br.rb.inverse_mass(), br.rb.inverse_moment_of_inertia()))
            .collect();

        // detect collisions, produce contact constraints
        use collision::BroadPhase;
        let pairs = collision::broadphase::BruteForce::pairs(&body_refs);
        let mut contacts: Vec<Contact> = Vec::new();
        let mut penetration_constraints: Vec<WorkingConstraint> = Vec::new();
        let mut friction_constraints: Vec<WorkingConstraint> = Vec::new();
        for pair in pairs {
            if body_refs[pair[0]].rb.responds_to_collisions()
                || body_refs[pair[1]].rb.responds_to_collisions()
            {
                let coll_contacts = collision::narrowphase::intersection_check(
                    &body_refs[pair[0]],
                    &body_refs[pair[1]],
                );
                for (contact_idx, contact) in coll_contacts.iter().enumerate() {
                    // store the contacts themselves to generate events later
                    contacts.push(*contact);

                    let body_indices = ordered_positions(&body_refs[pair[0]], &body_refs[pair[1]]);

                    let normal_constr = ConstraintFunction::Normal {
                        normal: contact.normal,
                        offsets: contact.offsets,
                    };

                    // constraint index for friction to depend on
                    let next_constr_idx = penetration_constraints.len();
                    let depth_with_slop = if contact.depth.abs() < SLOP_LIMIT {
                        0.0
                    } else {
                        contact.depth - contact.depth.signum() * SLOP_LIMIT
                    };
                    penetration_constraints.push(WorkingConstraint {
                        body_indices: (pair[0], Some(pair[1])),
                        jacobian_row: normal_constr
                            .jacobian(*body_refs[pair[0]].tr, Some(*body_refs[pair[1]].tr)),
                        softness: ConstraintSoftnessType::Hard {
                            correction_coef: self.stabilisation_coef,
                        },
                        pos_error: depth_with_slop,
                        bounds: ImpulseBounds::Constant(None, Some(0.0)),
                        cache_id: ConstraintId::Dynamic(DynamicConstraintId {
                            body_indices,
                            constr_id: if contact_idx == 0 {
                                DynamicConstraintType::FirstContact
                            } else {
                                DynamicConstraintType::SecondContact
                            },
                        }),
                    });

                    // friction

                    // simplified coefficient model, actually this would depend on the specific pair of materials
                    // and couldn't be stored within a single material
                    let friction_coef = body_refs[pair[0]].rb.material.friction
                        * body_refs[pair[1]].rb.material.friction;

                    let tangent_constr = ConstraintFunction::Normal {
                        normal: Unit::new_unchecked(math::left_normal(*contact.normal)),
                        offsets: contact.offsets,
                    };

                    friction_constraints.push(WorkingConstraint {
                        body_indices: (pair[0], Some(pair[1])),
                        jacobian_row: tangent_constr
                            .jacobian(*body_refs[pair[0]].tr, Some(*body_refs[pair[1]].tr)),
                        softness: ConstraintSoftnessType::Hard {
                            correction_coef: 0.0,
                        },
                        pos_error: 0.0,
                        bounds: ImpulseBounds::Depends {
                            constraint_idx: next_constr_idx,
                            coefficient: friction_coef,
                        },
                        cache_id: ConstraintId::Dynamic(DynamicConstraintId {
                            body_indices,
                            constr_id: if contact_idx == 0 {
                                DynamicConstraintType::FirstFriction
                            } else {
                                DynamicConstraintType::SecondFriction
                            },
                        }),
                    });
                }
            }
        }

        let mut user_constraints: Vec<WorkingConstraint> =
            Vec::with_capacity(self.user_constraints.len());
        // we remove any constraints that point to bodies we haven't seen
        let mut stale_constraints: Vec<ConstraintHandle> = Vec::new();
        for (key, user_ctr) in self.user_constraints.iter() {
            let ref_idx_1 = match node_ref_map.get(&user_ctr.owner.pos().item_idx) {
                Some(node) => *node,
                None => {
                    stale_constraints.push(key);
                    continue;
                }
            };
            let ref_idx_2 = if let Some(target) = user_ctr.target {
                match node_ref_map.get(&target.pos().item_idx) {
                    Some(node) => Some(*node),
                    None => {
                        stale_constraints.push(key);
                        continue;
                    }
                }
            } else {
                None
            };
            let tr1 = *body_refs[ref_idx_1].tr;
            let tr2 = ref_idx_2.map(|b2| *body_refs[b2].tr);
            let cache_id = ConstraintId::UserDefined(key);
            let softness = match user_ctr.softness {
                None => ConstraintSoftnessType::Hard {
                    correction_coef: self.stabilisation_coef,
                },
                Some(osc_params) => ConstraintSoftnessType::SoftOscillator(osc_params),
            };
            let pos_error = user_ctr.func.value(tr1, tr2);
            let error_with_slop = if pos_error.abs() < SLOP_LIMIT {
                0.0
            } else {
                pos_error - pos_error.signum() * SLOP_LIMIT
            };
            user_constraints.push(WorkingConstraint {
                body_indices: (ref_idx_1, ref_idx_2),
                jacobian_row: user_ctr.func.jacobian(tr1, tr2),
                softness,
                pos_error: error_with_slop,
                bounds: ImpulseBounds::Constant(
                    user_ctr.impulse_bounds.0,
                    user_ctr.impulse_bounds.1,
                ),
                cache_id,
            })
        }
        for stale_handle in stale_constraints {
            self.user_constraints.remove(stale_handle);
        }

        // contacts will fire events, but we want to compute impulse first to include it in the event.
        // this lets us find which constraints were collisions later
        let pen_constraint_range = 0..penetration_constraints.len();

        let constraints = itertools::concat(vec![
            penetration_constraints,
            friction_constraints,
            user_constraints,
        ]);

        if constraints.len() != 0 {
            // solve
            let impulses = constraint::solve(
                self.solver_params,
                dt,
                &constraints,
                &mut velocities,
                &inv_masses,
                &mut self.impulse_cache,
            );

            // push events
            for contact_idx in pen_constraint_range {
                let constraint = &constraints[contact_idx];
                let bodies = &constraint.body_indices;
                let contact = &contacts[contact_idx];
                let impulse = impulses[contact_idx];
                if let Some(mut sink) =
                    graph.get_neighbor_mut_unchecked(&body_refs[bodies.0].rb, l_evt_sink)
                {
                    sink.push(crate::Event::Contact(ContactEvent {
                        // unwrap because all contact constraints have a target body
                        other_body: NodeRef::as_node(&body_refs[bodies.1.unwrap()].rb, &graph),
                        info: ContactInfo {
                            point: contact.point,
                            normal: -contact.normal,
                            impulse,
                        },
                    }));
                }
                if let Some(mut sink) =
                    graph.get_neighbor_mut_unchecked(&body_refs[bodies.1.unwrap()].rb, l_evt_sink)
                {
                    sink.push(crate::Event::Contact(ContactEvent {
                        other_body: NodeRef::as_node(&body_refs[bodies.0].rb, &graph),
                        info: ContactInfo {
                            point: contact.point,
                            normal: contact.normal,
                            impulse,
                        },
                    }));
                }
            }

            // integrate

            // drop body_refs so we can get mutable references for applying the physics
            let body_nodes: Vec<BodyNodes> =
                body_refs.into_iter().map(|br| br.downgrade()).collect();

            // apply the velocities updated by the solver back to the actual bodies
            for (body, new_vel) in izip!(body_nodes, velocities) {
                let mut rb = l_body.get_mut_unchecked(body.rb);
                if let Some(body_vel) = rb.velocity_mut() {
                    *body_vel = new_vel;
                }
            }
        }

        // semi-implicit Euler integration: use velocities at the end of the time step
        for rb in l_body.iter(graph) {
            if let (Some(mut tr), Some(vel)) =
                (graph.get_neighbor_mut(&rb, l_transform), rb.velocity())
            {
                tr.append_translation(dt * vel.linear);
                tr.prepend_rotation(Angle::Rad(dt * vel.angular).into());
            }
        }
    }
}

/// References to the parts of a body that we need to find out if it collides with anything.
/// Used internally in collision detection, exposed to allow custom broad phase algorithms.
pub struct BodyRef<'a> {
    pub tr: graph::NodeRef<'a, uv::Isometry2>,
    pub coll: graph::NodeRef<'a, Collider>,
    pub rb: graph::NodeRef<'a, RigidBody>,
}

impl<'a> BodyRef<'a> {
    fn downgrade(&self) -> BodyNodes {
        BodyNodes {
            tr: self.tr.pos(),
            coll: self.coll.pos(),
            rb: self.rb.pos(),
        }
    }
}

fn ordered_positions(b1: &BodyRef<'_>, b2: &BodyRef<'_>) -> [usize; 2] {
    let i1 = b1.rb.pos().item_idx;
    let i2 = b2.rb.pos().item_idx;
    if i1 < i2 {
        [i1, i2]
    } else {
        [i2, i1]
    }
}

/// A non-reference version of `BodyRef`, to allow for multiple mutable references
/// from the same graph layer during one iteration.
#[derive(Clone, Copy, Debug)]
struct BodyNodes {
    tr: graph::NodePosition,
    coll: graph::NodePosition,
    rb: graph::NodePosition,
}
