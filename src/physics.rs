use crate::{
    graph::{self, UnsafeNode},
    math as m,
};

use itertools::izip;
use nalgebra as na;
use slotmap as sm;
use std::collections::HashMap;

//

pub mod collision;
pub use collision::{Collider, ColliderShape, Contact};

pub mod constraint;
pub use constraint::{Constraint, SolverConvergence, SolverParams};
use constraint::{
    ConstraintId, ConstraintType, DynamicConstraintId, DynamicConstraintType, ImpulseBounds,
    ImpulseCache, WorkingConstraint,
};

pub mod forcefield;
pub use forcefield::ForceField;

pub mod rigidbody;
pub use rigidbody::RigidBody;

//

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
    pub point: m::Point2,
    /// The normal of the colliding plane, facing towards this object.
    pub normal: na::Unit<m::Vec2>,
    /// The strength of the impulse caused by the contact.
    pub impulse: f32,
}

sm::new_key_type! {
    pub struct ConstraintHandle;
}

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
            solver_params: SolverParams {
                max_iterations: 20,
                convergence: SolverConvergence::AllElements(0.02),
            },
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

    /// Detect collisions, solve constraint forces and move bodies.
    pub fn tick<EvtParams>(
        &mut self,
        graph: &graph::Graph,
        l_transform: &mut graph::Layer<m::Transform>,
        l_body: &mut graph::Layer<RigidBody>,
        l_collider: &graph::Layer<Collider>,
        l_evt_sink: &mut crate::EventSinkLayer<EvtParams>,
        dt: f32,
        forcefield: Option<&impl ForceField>,
    ) {
        let inv_dt = 1.0 / dt;
        // apply environment forces (gravity, usually)
        if let Some(ff) = forcefield {
            for rb in l_body.iter_mut(graph) {
                if let (Some(ref tr), rigidbody::BodyType::Dynamic { velocity, .. }) =
                    (graph.get_neighbor(&rb, &l_transform), &mut rb.item.body)
                {
                    velocity.linear += ff.value_at(tr.item.isometry.translation.vector.into()) * dt;
                }
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

        let velocities: Vec<Velocity> = body_refs
            .iter()
            .map(|br| br.rb.item.velocity_or_zero())
            .collect();
        let inv_masses: Vec<m::Vec2> = body_refs
            .iter()
            .map(|br| {
                m::Vec2::new(
                    br.rb.item.inverse_mass(),
                    br.rb.item.inverse_moment_of_inertia(),
                )
            })
            .collect();

        // detect collisions, produce contact constraints
        use collision::BroadPhase;
        let pairs = collision::broadphase::BruteForce::pairs(&body_refs);
        let mut contacts: Vec<Contact> = Vec::new();
        let mut penetration_constraints: Vec<WorkingConstraint> = Vec::new();
        let mut friction_constraints: Vec<WorkingConstraint> = Vec::new();
        for pair in pairs {
            if body_refs[pair[0]].rb.item.responds_to_collisions()
                || body_refs[pair[1]].rb.item.responds_to_collisions()
            {
                let coll_contacts = collision::narrowphase::intersection_check(
                    &body_refs[pair[0]],
                    &body_refs[pair[1]],
                );
                for (contact_idx, contact) in coll_contacts.iter().enumerate() {
                    // store the contacts themselves to generate events later
                    contacts.push(*contact);

                    let body_indices = ordered_positions(&body_refs[pair[0]], &body_refs[pair[1]]);

                    let normal_constr = ConstraintType::Normal {
                        normal: contact.normal,
                        offsets: contact.offsets,
                    };

                    // constraint index for friction to depend on
                    let next_constr_idx = penetration_constraints.len();
                    penetration_constraints.push(WorkingConstraint {
                        body_indices: (pair[0], Some(pair[1])),
                        jacobian_row: normal_constr.gradient(),
                        bias: -contact.depth * self.stabilisation_coef * inv_dt,
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
                    let friction_coef = body_refs[pair[0]].rb.item.material.friction
                        * body_refs[pair[1]].rb.item.material.friction;

                    let tangent_constr = ConstraintType::Normal {
                        normal: na::Unit::new_unchecked(m::left_normal(&*contact.normal)),
                        offsets: contact.offsets,
                    };

                    friction_constraints.push(WorkingConstraint {
                        body_indices: (pair[0], Some(pair[1])),
                        jacobian_row: tangent_constr.gradient(),
                        bias: 0.0,
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
            user_constraints.push(WorkingConstraint {
                body_indices: (ref_idx_1, ref_idx_2),
                jacobian_row: user_ctr.ty.gradient(),
                bias: 0.0, // TODO: bias from position constraint value
                bounds: ImpulseBounds::Constant(
                    user_ctr.impulse_bounds.0,
                    user_ctr.impulse_bounds.1,
                ),
                cache_id: ConstraintId::UserDefined(key),
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
            let impulses = constraint::solve_pgs(
                self.solver_params,
                dt,
                &constraints,
                &velocities,
                &inv_masses,
                &mut self.impulse_cache,
            );

            // push events
            for contact_idx in pen_constraint_range {
                let constraint = &constraints[contact_idx];
                let bodies = &constraint.body_indices;
                let contact = &contacts[contact_idx];
                let impulse = impulses[contact_idx];
                if let Some(sink) =
                    graph.get_neighbor_mut_unchecked(&body_refs[bodies.0].rb, l_evt_sink)
                {
                    sink.item.push(crate::Event::Contact(ContactEvent {
                        // unwrap because all contact constraints have a target body
                        other_body: body_refs[bodies.1.unwrap()].rb.node(graph),
                        info: ContactInfo {
                            point: contact.point,
                            normal: -contact.normal,
                            impulse,
                        },
                    }));
                }
                if let Some(sink) =
                    graph.get_neighbor_mut_unchecked(&body_refs[bodies.1.unwrap()].rb, l_evt_sink)
                {
                    sink.item.push(crate::Event::Contact(ContactEvent {
                        other_body: body_refs[bodies.0].rb.node(graph),
                        info: ContactInfo {
                            point: contact.point,
                            normal: contact.normal,
                            impulse,
                        },
                    }));
                }
            }
            // // store impulses for next frame's warm start
            // self.impulse_cache.replace(&accumulators);

            // integrate

            // drop body_refs so we can get mutable references for applying the physics
            let body_nodes: Vec<BodyNodes> =
                body_refs.into_iter().map(|br| br.downgrade()).collect();

            for (constraint, impulse) in izip!(constraints, impulses) {
                {
                    let body1 = body_nodes[constraint.body_indices.0];
                    let rb1 = l_body.get_mut_unchecked(body1.rb).item;
                    let rb1_inv_mass = rb1.inverse_mass();
                    let rb1_inv_mi = rb1.inverse_moment_of_inertia();
                    if let Some(vel1) = rb1.velocity_mut() {
                        vel1.linear += rb1_inv_mass * constraint.jacobian_row.v1 * impulse * dt;
                        vel1.angular += rb1_inv_mi * constraint.jacobian_row.w1 * impulse * dt;
                    }
                }
                if let Some(body2_idx) = constraint.body_indices.1 {
                    let body2 = body_nodes[body2_idx];
                    let rb2 = l_body.get_mut_unchecked(body2.rb).item;
                    let rb2_inv_mass = rb2.inverse_mass();
                    let rb2_inv_mi = rb2.inverse_moment_of_inertia();
                    if let Some(vel2) = rb2.velocity_mut() {
                        vel2.linear += rb2_inv_mass * constraint.jacobian_row.v2 * impulse * dt;
                        vel2.angular += rb2_inv_mi * constraint.jacobian_row.w2 * impulse * dt;
                    }
                }
            }
        }

        // semi-implicit Euler integration: use velocities at the end of the time step
        for rb in l_body.iter(graph) {
            if let (Some(tr), Some(vel)) =
                (graph.get_neighbor_mut(&rb, l_transform), rb.item.velocity())
            {
                tr.item.append_translation_mut(&(dt * vel.linear).into());
                tr.item
                    .append_rotation_wrt_center_mut(&m::Angle::Rad(dt * vel.angular).into());
            }
        }
    }
}

/// References to the parts of a body that we need to find out if it collides with anything.
/// Used internally in collision detection, exposed to allow custom broad phase algorithms.
pub struct BodyRef<'a> {
    pub tr: graph::NodeRef<'a, m::Transform>,
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
