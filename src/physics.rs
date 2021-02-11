use crate::{
    graph::{self, UnsafeNode},
    math::{self, uv, Angle, Unit},
};

use itertools::izip;
use slotmap as sm;
use std::collections::HashMap;

//

pub mod collision;
use collision::narrowphase::intersection_check;
pub use collision::{Collider, ColliderShape, Contact, ContactResult};

pub mod constraint;
pub use constraint::{Constraint, ConstraintBuilder, OscillatorParams};

pub mod forcefield;
pub use forcefield::ForceField;

pub mod rigidbody;
pub use rigidbody::RigidBody;

//

/// Velocity of an object.
///
// Equivalent to a Vec3 but with names for the translational and rotational part.
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
        let tangent = math::left_normal(offset) * self.angular;
        self.linear + tangent
    }

    pub fn apply_to_pose(&self, dt: f32, mut pose: math::Pose) -> math::Pose {
        let scaled = *self * dt;
        pose.append_translation(scaled.linear);
        pose.prepend_rotation(math::Angle::Rad(scaled.angular).into());
        pose
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
impl std::ops::Mul<f32> for Velocity {
    type Output = Velocity;

    fn mul(self, rhs: f32) -> Self::Output {
        Velocity {
            linear: self.linear * rhs,
            angular: self.angular * rhs,
        }
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

pub struct Physics {
    pub substeps: usize,
    user_constraints: sm::DenseSlotMap<ConstraintHandle, Constraint>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::with_substeps(10)
    }
}

impl Physics {
    /// Create a physics solver with the specified number of substeps per frame.
    pub fn with_substeps(substeps: usize) -> Self {
        Physics {
            substeps,
            user_constraints: sm::DenseSlotMap::with_key(),
        }
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

    /// Remove all constraints.
    pub fn clear_constraints(&mut self) {
        self.user_constraints.clear();
    }

    /// Detect collisions, solve constraint forces and move bodies.
    pub fn tick<EvtParams>(
        &mut self,
        graph: &graph::Graph,
        l_pose: &mut graph::Layer<math::Pose>,
        l_body: &mut graph::Layer<RigidBody>,
        l_collider: &graph::Layer<Collider>,
        l_evt_sink: &mut crate::EventSinkLayer<EvtParams>,
        dt: f32,
        forcefield: &impl ForceField,
    ) {
        let dt = dt / self.substeps as f32;
        let inv_dt = 1.0 / dt;

        let body_refs: Vec<BodyRef> = l_body
            .iter(graph)
            .filter_map(|rb| {
                let coll = graph.get_neighbor(&rb, l_collider)?;
                let pose = graph.get_neighbor(&rb, l_pose)?;
                Some(BodyRef { pose, coll, rb })
            })
            .collect();
        // buffers for working variables, outside of body_refs
        // to make it simpler to mutate things without breaking borrowing rules.
        // indexed using the same index as for body_refs
        let mut old_poses: Vec<math::Pose> = body_refs.iter().map(|body| *body.pose).collect();
        let mut poses: Vec<math::Pose> = old_poses.clone();
        // old velocities used for restitution
        let mut old_velocities: Vec<Velocity> = body_refs
            .iter()
            .map(|body| body.rb.velocity_or_zero())
            .collect();
        let mut velocities: Vec<Velocity> = old_velocities.clone();
        // accelerations from external forces used as a speed limit for restitution
        let mut ext_f_accelerations: Vec<uv::Vec2> = vec![uv::Vec2::default(); velocities.len()];

        // map from the position of a node in the graph layer to the position of a node in body_refs
        let node_ref_map: HashMap<usize, usize> = body_refs
            .iter()
            .enumerate()
            .map(|(idx, br)| (br.rb.pos().item_idx, idx))
            .collect();

        // TODO: map out user constraints here

        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep
        let pairs: Vec<[usize; 2]> = {
            use collision::BroadPhase;
            let mut p = collision::broadphase::BruteForce::pairs(&body_refs);
            p.retain(|[b1, b2]| {
                body_refs[*b1].rb.responds_to_collisions()
                    || body_refs[*b2].rb.responds_to_collisions()
            });
            p
        };

        // TODO: how to collect contact events from contacts happening over multiple timesteps?
        // probably have an array of accumulator-type things with length equal to `pairs`

        for _substep in 0..self.substeps {
            //
            // apply external forces and estimate post-step pose with explicit Euler step
            //
            for (body, old_pose, pose, old_vel, vel, ext_accel) in izip!(
                &body_refs,
                &mut old_poses,
                &mut poses,
                &mut old_velocities,
                &mut velocities,
                &mut ext_f_accelerations
            ) {
                if let rigidbody::BodyType::Dynamic { .. } = body.rb.body {
                    // TODO: rename forcefield to accelerationfield or allow it to depend on mass
                    let ff_accel = forcefield.value_at(pose.translation);
                    vel.linear += ff_accel * dt;
                    *ext_accel = ff_accel;

                    // old_vel is velocity after external forces but before collisions
                    *old_vel = *vel;
                    *old_pose = *pose;
                    *pose = vel.apply_to_pose(dt, *pose);
                }
            }

            //
            // single Nonlinear Gauss-Seidel position solve step (accuracy is achieved with substepping)
            //

            // re-do collision detection every iteration so we don't miss anything
            let contacts: Vec<ContactResult> = pairs
                .iter()
                .map(|[b1, b2]| {
                    intersection_check(
                        &poses[*b1],
                        &body_refs[*b1].coll,
                        &poses[*b2],
                        &body_refs[*b2].coll,
                    )
                })
                .collect();

            // helper to reduce duplication when fetching info for a pair objects
            fn map_pair<T, R>(pair: [T; 2], f: impl Fn(&T) -> R) -> [R; 2] {
                [f(&pair[0]), f(&pair[1])]
            }

            for (pair, contact) in izip!(&pairs, &contacts) {
                // TODO: match here and use a block solver for the Two case
                for contact in contact.iter() {
                    let inv_masses = map_pair(*pair, |b| body_refs[*b].rb.inverse_mass());
                    let inv_mom_inertias =
                        map_pair(*pair, |b| body_refs[*b].rb.inverse_moment_of_inertia());
                    let offsets_wedge_normal = map_pair([0, 1], |i| {
                        let os_rotated: uv::Vec2 = poses[pair[*i]].rotation * contact.offsets[*i];
                        os_rotated.wedge(*contact.normal).xy
                    });
                    let eff_inv_masses = map_pair([0, 1], |i| {
                        inv_masses[*i] + (offsets_wedge_normal[*i].powi(2) * inv_mom_inertias[*i])
                    });

                    // we can't return depth directly from collision detection because
                    // earlier position corrections can change it,
                    // thus we compute depth here from the points on each object's surface
                    let offsets_worldspace =
                        map_pair([0, 1], |i| poses[pair[*i]] * contact.offsets[*i]);
                    let depth =
                        (offsets_worldspace[0] - offsets_worldspace[1]).dot(*contact.normal);

                    if depth <= 0.0 {
                        continue;
                    }

                    let d_lambda = -depth / (eff_inv_masses[0] + eff_inv_masses[1]);
                    let impulse = d_lambda * *contact.normal;

                    poses[pair[0]].append_translation(impulse * inv_masses[0]);
                    poses[pair[0]].prepend_rotation(
                        Angle::Rad(d_lambda * offsets_wedge_normal[0] * inv_mom_inertias[0]).into(),
                    );
                    poses[pair[1]].append_translation(-impulse * inv_masses[1]);
                    poses[pair[1]].prepend_rotation(
                        Angle::Rad(-d_lambda * offsets_wedge_normal[1] * inv_mom_inertias[1])
                            .into(),
                    );

                    // TODO: static friction here
                }
            }

            //
            // update velocities from pose differences
            //

            for (old_pose, pose, vel) in izip!(&old_poses, &poses, &mut velocities) {
                vel.linear = (pose.translation - old_pose.translation) * inv_dt;
                // I'm sure there are more efficient ways to handle the angle but this'll do
                vel.angular =
                    Angle::from(pose.rotation * old_pose.rotation.reversed()).rad() * inv_dt;
            }

            //
            // velocity step for dynamic friction and restitution on contacts
            //

            for (pair, contact) in izip!(&pairs, &contacts) {
                for contact in contact.iter() {
                    // TODO: cache these earlier
                    let inv_masses = map_pair(*pair, |b| body_refs[*b].rb.inverse_mass());
                    let inv_mom_inertias =
                        map_pair(*pair, |b| body_refs[*b].rb.inverse_moment_of_inertia());
                    let offsets_rotated =
                        map_pair([0, 1], |i| poses[pair[*i]].rotation * contact.offsets[*i]);
                    let offsets_wedge_normal =
                        map_pair([0, 1], |i| offsets_rotated[*i].wedge(*contact.normal).xy);
                    let eff_inv_masses = map_pair([0, 1], |i| {
                        inv_masses[*i] + (offsets_wedge_normal[*i].powi(2) * inv_mom_inertias[*i])
                    });
                    // </TODO>

                    let relative_vel_at_p = velocities[pair[0]].point_velocity(offsets_rotated[0])
                        - velocities[pair[1]].point_velocity(offsets_rotated[1]);
                    let normal_vel = relative_vel_at_p.dot(*contact.normal);
                    let tangent_vel = relative_vel_at_p - (normal_vel * *contact.normal);

                    // TODO: friction using tangent_vel

                    let old_normal_vel = (old_velocities[pair[0]]
                        .point_velocity(offsets_rotated[0])
                        - old_velocities[pair[1]].point_velocity(offsets_rotated[1]))
                    .dot(*contact.normal);
                    let restitution_coef = if old_normal_vel * old_normal_vel
                        < dt * (ext_f_accelerations[pair[0]] + ext_f_accelerations[pair[1]])
                            .mag_sq()
                    {
                        0.0
                    } else {
                        // TODO: add restitution coef to rigid bodies and use that
                        0.0
                    };

                    let delta_v_mag = -normal_vel - (restitution_coef * old_normal_vel).min(0.0);
                    let impulse_mag = delta_v_mag / (eff_inv_masses[0] + eff_inv_masses[1]);
                    velocities[pair[0]].linear += impulse_mag * inv_masses[0] * *contact.normal;
                    velocities[pair[0]].angular +=
                        impulse_mag * inv_mom_inertias[0] * offsets_wedge_normal[0];
                    velocities[pair[1]].linear -= impulse_mag * inv_masses[1] * *contact.normal;
                    velocities[pair[1]].angular -=
                        impulse_mag * inv_mom_inertias[1] * offsets_wedge_normal[1];
                }
            }
        }

        //
        // apply results back to state from temp buffers
        //

        // drop body_refs so we can get mutable references
        let body_nodes: Vec<BodyNodes> = body_refs.into_iter().map(|br| br.downgrade()).collect();
        for (body, pose_result, vel_result) in izip!(body_nodes, poses, velocities) {
            let mut rb = l_body.get_mut_unchecked(body.rb);
            let mut pose = l_pose.get_mut_unchecked(body.pose);
            rb.velocity_mut().map(|v| *v = vel_result);
            *pose = pose_result;
        }
    }
}

/// References to the parts of a body that we need to find out if it collides with anything.
/// Used internally in collision detection, exposed to allow custom broad phase algorithms.
pub struct BodyRef<'a> {
    pub pose: graph::NodeRef<'a, math::Pose>,
    pub coll: graph::NodeRef<'a, Collider>,
    pub rb: graph::NodeRef<'a, RigidBody>,
}

impl<'a> BodyRef<'a> {
    fn downgrade(&self) -> BodyNodes {
        BodyNodes {
            pose: self.pose.pos(),
            coll: self.coll.pos(),
            rb: self.rb.pos(),
        }
    }
}

/// A non-reference version of `BodyRef`, to allow for multiple mutable references
/// from the same graph layer during one iteration.
#[derive(Clone, Copy, Debug)]
struct BodyNodes {
    pose: graph::NodePosition,
    coll: graph::NodePosition,
    rb: graph::NodePosition,
}
