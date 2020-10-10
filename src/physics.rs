use crate::{
    graph::{self, UnsafeNode},
    math as m,
};

use itertools::izip;
use nalgebra as na;

//

pub mod collision;
pub use collision::{Collider, ColliderShape};

pub mod constraint;
pub use constraint::{Constraint, SolverConvergence, SolverParams};
use constraint::{ConstraintType, WorkingConstraint};

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

pub struct Physics {
    stabilisation_coef: f32,
    solver_params: SolverParams,
}

impl Physics {
    pub fn new() -> Self {
        Physics {
            stabilisation_coef: 0.1,
            solver_params: SolverParams {
                max_iterations: 10,
                convergence: SolverConvergence::AllElements(0.02),
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

    /// Replace the default parameters for the constraint solver.
    pub fn with_solver_params(mut self, params: SolverParams) -> Self {
        self.solver_params = params;
        self
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
        let mut contact_constraints = Vec::new();
        for pair in pairs {
            if body_refs[pair[0]].rb.item.responds_to_collisions()
                || body_refs[pair[1]].rb.item.responds_to_collisions()
            {
                let contacts = collision::narrowphase::intersection_check(
                    &body_refs[pair[0]],
                    &body_refs[pair[1]],
                );
                for contact in contacts {
                    let ct = ConstraintType::Nonpenetration { contact };

                    contact_constraints.push(WorkingConstraint {
                        body_indices: pair,
                        jacobian_row: ct.jacobian(),
                        bias: -ct.value([body_refs[pair[0]].tr.item, body_refs[pair[1]].tr.item])
                            * self.stabilisation_coef
                            * inv_dt,
                        bounds: (None, Some(0.0)),
                        // TODO: bring caching back
                        first_guess: 0.0,
                    });
                }
            }
        }

        // TODO: also allow other constraints in the world
        let constraints = contact_constraints;

        // solve
        if constraints.len() != 0 {
            let impulses = constraint::solve_pgs(
                self.solver_params,
                dt,
                &constraints,
                &velocities,
                &inv_masses,
            );

            // // push events
            // for acc in &accumulators {
            //     if let Some(sink) =
            //         graph.get_neighbor_mut_unchecked(&acc.constraint.nodes[0].rb, l_evt_sink)
            //     {
            //         sink.item.push(crate::Event::Contact(ContactEvent {
            //             other_body: l_body.get_unchecked(acc.constraint.nodes[1].rb).node(graph),
            //             info: ContactInfo {
            //                 point: acc.constraint.point,
            //                 normal: -acc.constraint.normal,
            //                 impulse: acc.total_impulse,
            //             },
            //         }));
            //     }
            //     if let Some(sink) =
            //         graph.get_neighbor_mut_unchecked(&acc.constraint.nodes[1].rb, l_evt_sink)
            //     {
            //         sink.item.push(crate::Event::Contact(ContactEvent {
            //             other_body: l_body.get_unchecked(acc.constraint.nodes[0].rb).node(graph),
            //             info: ContactInfo {
            //                 point: acc.constraint.point,
            //                 normal: acc.constraint.normal,
            //                 impulse: acc.total_impulse,
            //             },
            //         }));
            //     }
            // }
            // // store impulses for next frame's warm start
            // self.impulse_cache.replace(&accumulators);

            // integrate

            // drop body_refs so we can get mutable references for applying the physics
            let body_nodes: Vec<BodyNodes> =
                body_refs.into_iter().map(|br| br.downgrade()).collect();

            for (constraint, impulse) in izip!(constraints, impulses) {
                let bodies = [
                    body_nodes[constraint.body_indices[0]],
                    body_nodes[constraint.body_indices[1]],
                ];
                {
                    let rb1 = l_body.get_mut_unchecked(bodies[0].rb).item;
                    let rb1_inv_mass = rb1.inverse_mass();
                    let rb1_inv_mi = rb1.inverse_moment_of_inertia();
                    if let Some(vel1) = rb1.velocity_mut() {
                        vel1.linear += rb1_inv_mass * constraint.jacobian_row.v1 * impulse * dt;
                        vel1.angular += rb1_inv_mi * constraint.jacobian_row.w1 * impulse * dt;
                    }
                }
                {
                    let rb2 = l_body.get_mut_unchecked(bodies[1].rb).item;
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

/// A non-reference version of `BodyRef`, to allow for multiple mutable references
/// from the same graph layer during one iteration.
///
/// It's a very important invariant that these don't live longer than a single physics tick!
/// We skip generation checks for efficiency when repeatedly reading the same objects,
/// but things can be deleted between frames, invalidating these node positions.
#[derive(Clone, Copy, Debug)]
struct BodyNodes {
    tr: graph::NodePosition,
    coll: graph::NodePosition,
    rb: graph::NodePosition,
}
impl BodyNodes {
    fn upgrade<'a>(
        self,
        l_tr: &'a graph::Layer<m::Transform>,
        l_coll: &'a graph::Layer<Collider>,
        l_rb: &'a graph::Layer<RigidBody>,
    ) -> BodyRef<'a> {
        BodyRef {
            tr: l_tr.get_unchecked(self.tr),
            coll: l_coll.get_unchecked(self.coll),
            rb: l_rb.get_unchecked(self.rb),
        }
    }

    fn cache_id(&self, other: BodyNodes) -> [usize; 2] {
        [self.rb.item_idx, other.rb.item_idx]
    }
}
