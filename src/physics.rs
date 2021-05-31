use crate::{
    event::{Event, EventSink},
    graph::{self, UnsafeNode},
    math::{self as m, Angle},
};

use itertools::izip;
use slotmap as sm;
use std::collections::HashMap;

//

pub mod collision;
use collision::narrowphase::intersection_check;
pub use collision::{Collider, ColliderShape, Contact, ContactResult};

pub mod constraint;
pub use constraint::{Constraint, ConstraintBuilder, ConstraintLimit, ConstraintType};

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
    pub linear: m::Vec2,
    /// Angular velocity in radians per second.
    pub angular: f64,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: m::Vec2::zero(),
            angular: 0.0,
        }
    }
}

impl Velocity {
    /// Get the linear velocity of a point offset from the center of mass.
    pub fn point_velocity(&self, offset: m::Vec2) -> m::Vec2 {
        let tangent = m::left_normal(offset) * self.angular;
        self.linear + tangent
    }

    pub fn apply_to_pose(&self, dt: f64, mut pose: m::Pose) -> m::Pose {
        let scaled = *self * dt;
        pose.append_translation(scaled.linear);
        pose.prepend_rotation(m::Angle::Rad(scaled.angular).into());
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
impl std::ops::Mul<f64> for Velocity {
    type Output = Velocity;

    fn mul(self, rhs: f64) -> Self::Output {
        Velocity {
            linear: self.linear * rhs,
            angular: self.angular * rhs,
        }
    }
}

/// Events produced by the physics system when two physics objects collide.
///
/// Note: for now this doesn't tell you anything except which body the contact was with.
/// This isn't great, but that's the only thing I've needed so far.
/// More info will be added when needed in a project.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    /// The body that this body was in contact with.
    pub other_body: graph::Node<RigidBody>,
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

    /// Access a constraint if it still exists.
    pub fn get_constraint(&self, handle: ConstraintHandle) -> Option<&Constraint> {
        self.user_constraints.get(handle)
    }

    /// Mutably access a constraint if it still exists.
    pub fn get_constraint_mut(&mut self, handle: ConstraintHandle) -> Option<&mut Constraint> {
        self.user_constraints.get_mut(handle)
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
    pub fn tick(
        &mut self,
        graph: &graph::Graph,
        l_pose: &mut graph::Layer<m::Pose>,
        l_body: &mut graph::Layer<RigidBody>,
        l_collider: &graph::Layer<Collider>,
        l_evt_sink: &mut graph::Layer<EventSink>,
        dt: f64,
        forcefield: &impl ForceField,
    ) {
        let dt = dt / self.substeps as f64;
        let inv_dt = 1.0 / dt;
        let inv_dt_sq = inv_dt * inv_dt;

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
        let mut old_poses: Vec<m::Pose> = body_refs.iter().map(|body| *body.pose).collect();
        let mut poses: Vec<m::Pose> = old_poses.clone();
        // old velocities used for restitution
        let mut old_velocities: Vec<Velocity> = body_refs
            .iter()
            .map(|body| body.rb.velocity_or_zero())
            .collect();
        let mut velocities: Vec<Velocity> = old_velocities.clone();
        // accelerations from external forces used as a speed limit for restitution
        let mut ext_f_accelerations: Vec<m::Vec2> = vec![m::Vec2::default(); velocities.len()];

        //
        // set up user-defined constraints
        //

        // remove constraints where one or both participating bodies have been destroyed
        self.user_constraints.retain(|_, c| {
            c.owner.check(&graph).is_some()
                && c.target.map(|t| t.check(&graph).is_some()).unwrap_or(true)
        });

        // map from the position of a node in the graph layer to the position of a node in body_refs
        let node_ref_map: HashMap<usize, usize> = body_refs
            .iter()
            .enumerate()
            .map(|(idx, br)| (br.rb.pos().item_idx, idx))
            .collect();

        // assuming here that slotmap's iteration order doesn't change if the
        // contents don't change. it doesn't guarantee this in the docs so if
        // constraints start jumping from one object to another this is why
        let constraint_body_pairs: Vec<(usize, Option<usize>)> = self
            .user_constraints
            .values()
            .map(|c| {
                (
                    node_ref_map[&c.owner.pos().item_idx],
                    c.target.map(|t| node_ref_map[&t.pos().item_idx]),
                )
            })
            .collect();

        //
        // Set up collision detection
        //

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
        // store contact forces for friction purposes
        let mut contact_lambdas: Vec<f64> = vec![0.0; pairs.len()];

        //
        // Actual physics step
        //

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

            // helpers to reduce duplication when fetching info for pairs of objects
            fn map_pair<T, R>(pair: [T; 2], f: impl Fn(&T) -> R) -> [R; 2] {
                [f(&pair[0]), f(&pair[1])]
            }

            fn map_semi_pair<T, R>(
                pair: (T, Option<T>),
                f: impl Fn(&T) -> R,
                snd_default: R,
            ) -> [R; 2] {
                [f(&pair.0), pair.1.map(|x| f(&x)).unwrap_or(snd_default)]
            }

            //
            // User-defined constraints
            //

            for (constraint, pair) in izip!(self.user_constraints.values(), &constraint_body_pairs)
            {
                let inv_masses = map_semi_pair(*pair, |b| body_refs[*b].rb.inverse_mass(), 0.0);
                let inv_mom_inertias =
                    map_semi_pair(*pair, |b| body_refs[*b].rb.inverse_moment_of_inertia(), 0.0);

                match constraint.ty {
                    ConstraintType::Distance { distance } => {
                        let offsets_worldspace = [
                            poses[pair.0] * constraint.offsets[0],
                            pair.1
                                .map(|p1| poses[p1] * constraint.offsets[1])
                                .unwrap_or(constraint.offsets[1]),
                        ];
                        let actual_dist = offsets_worldspace[1] - offsets_worldspace[0];
                        let actual_dist_mag = actual_dist.mag();
                        let error = distance - actual_dist_mag;

                        if match constraint.limit {
                            ConstraintLimit::Eq => true,
                            ConstraintLimit::Lt if error < 0.0 => true,
                            ConstraintLimit::Gt if error > 0.0 => true,
                            _ => false,
                        } {
                            let dir = if actual_dist_mag != 0.0 {
                                actual_dist / actual_dist_mag
                            } else {
                                m::Vec2::unit_y()
                            };

                            match pair.1 {
                                Some(p1) => {
                                    // TODO: this will need some abstraction when more constraint types
                                    // involving position corrections are introduced
                                    let pair = [pair.0, p1];
                                    let offsets_rotated = map_pair([0, 1], |i| {
                                        poses[pair[*i]].rotation * constraint.offsets[*i]
                                    });
                                    let offsets_wedge_dir =
                                        map_pair([0, 1], |i| offsets_rotated[*i].wedge(dir).xy);
                                    let eff_inv_masses = map_pair([0, 1], |i| {
                                        inv_masses[*i]
                                            + (offsets_wedge_dir[*i].powi(2) * inv_mom_inertias[*i])
                                    });

                                    let lambda = -error
                                        / (eff_inv_masses[0]
                                            + eff_inv_masses[1]
                                            + constraint.compliance * inv_dt_sq);

                                    poses[pair[0]].append_translation(inv_masses[0] * lambda * dir);
                                    poses[pair[0]].prepend_rotation(
                                        Angle::Rad(
                                            inv_mom_inertias[0] * lambda * offsets_wedge_dir[0],
                                        )
                                        .into(),
                                    );
                                    poses[pair[1]]
                                        .append_translation(-inv_masses[1] * lambda * dir);
                                    poses[pair[1]].prepend_rotation(
                                        Angle::Rad(
                                            -inv_mom_inertias[1] * lambda * offsets_wedge_dir[1],
                                        )
                                        .into(),
                                    );
                                }
                                None => {
                                    // this is repetitive but kind of hard to abstract :thinking:
                                    let offset_rotated =
                                        poses[pair.0].rotation * constraint.offsets[0];
                                    let offset_wedge_dir = offset_rotated.wedge(dir).xy;
                                    let eff_inv_mass = inv_masses[0]
                                        + offset_wedge_dir.powi(2) * inv_mom_inertias[0];

                                    let lambda =
                                        -error / (eff_inv_mass + constraint.compliance * inv_dt_sq);

                                    poses[pair.0].append_translation(inv_masses[0] * lambda * dir);
                                    poses[pair.0].prepend_rotation(
                                        Angle::Rad(inv_mom_inertias[0] * lambda * offset_wedge_dir)
                                            .into(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            //
            // Contacts
            //

            for (pair, contact, lambda_n) in izip!(&pairs, &contacts, &mut contact_lambdas) {
                // TODO: match here and use a block solver for the Two case
                for contact in contact.iter() {
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

                    // we can't return depth directly from collision detection because
                    // earlier position corrections can change it,
                    // thus we compute depth here from the points on each object's surface
                    let offsets_worldspace =
                        map_pair([0, 1], |i| poses[pair[*i]] * contact.offsets[*i]);
                    let depth =
                        (offsets_worldspace[0] - offsets_worldspace[1]).dot(*contact.normal);

                    if depth <= 0.0 {
                        *lambda_n = 0.0;
                        continue;
                    }

                    *lambda_n = -depth / (eff_inv_masses[0] + eff_inv_masses[1]);

                    poses[pair[0]].append_translation(inv_masses[0] * *lambda_n * *contact.normal);
                    poses[pair[0]].prepend_rotation(
                        Angle::Rad(inv_mom_inertias[0] * *lambda_n * offsets_wedge_normal[0])
                            .into(),
                    );
                    poses[pair[1]].append_translation(-inv_masses[1] * *lambda_n * *contact.normal);
                    poses[pair[1]].prepend_rotation(
                        Angle::Rad(-inv_mom_inertias[1] * *lambda_n * offsets_wedge_normal[1])
                            .into(),
                    );

                    // static friction

                    let offsets_worldspace_old =
                        map_pair([0, 1], |i| old_poses[pair[*i]] * contact.offsets[*i]);
                    let offset_diff_motion = (offsets_worldspace[0] - offsets_worldspace_old[0])
                        - (offsets_worldspace[1] - offsets_worldspace_old[1]);
                    let tangent = m::left_normal(*contact.normal);
                    let motion_along_tan = offset_diff_motion.dot(tangent);

                    let friction_coef = (body_refs[pair[0]].rb.material)
                        .static_friction_with(&body_refs[pair[1]].rb.material);
                    let max_coulomb_dx = *lambda_n * friction_coef;

                    let offsets_wedge_tan =
                        map_pair([0, 1], |i| offsets_rotated[*i].wedge(tangent).xy);
                    let eff_inv_masses_tan = map_pair([0, 1], |i| {
                        inv_masses[*i] + (offsets_wedge_tan[*i].powi(2) * inv_mom_inertias[*i])
                    });

                    let lambda_t =
                        -motion_along_tan / (eff_inv_masses_tan[0] + eff_inv_masses_tan[1]);

                    if lambda_t < max_coulomb_dx {
                        poses[pair[0]].append_translation(inv_masses[0] * lambda_t * tangent);
                        poses[pair[0]].prepend_rotation(
                            Angle::Rad(inv_mom_inertias[0] * lambda_t * offsets_wedge_tan[0])
                                .into(),
                        );
                        poses[pair[1]].append_translation(-inv_masses[1] * lambda_t * tangent);
                        poses[pair[1]].prepend_rotation(
                            Angle::Rad(-inv_mom_inertias[1] * lambda_t * offsets_wedge_tan[1])
                                .into(),
                        );
                    }
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
            // velocity step for dynamic friction and restitution on contacts + damping on other constraints
            //

            for (pair, contact, lambda_n) in izip!(&pairs, &contacts, &contact_lambdas) {
                for contact in contact.iter() {
                    let inv_masses = map_pair(*pair, |b| body_refs[*b].rb.inverse_mass());
                    let inv_mom_inertias =
                        map_pair(*pair, |b| body_refs[*b].rb.inverse_moment_of_inertia());
                    let offsets_rotated =
                        map_pair([0, 1], |i| poses[pair[*i]].rotation * contact.offsets[*i]);

                    let relative_vel_at_p = velocities[pair[0]].point_velocity(offsets_rotated[0])
                        - velocities[pair[1]].point_velocity(offsets_rotated[1]);

                    // restitution

                    let normal_vel = relative_vel_at_p.dot(*contact.normal);
                    let old_rel_vel = old_velocities[pair[0]].point_velocity(offsets_rotated[0])
                        - old_velocities[pair[1]].point_velocity(offsets_rotated[1]);
                    let old_normal_vel = old_rel_vel.dot(*contact.normal);
                    let restitution_coef = if old_normal_vel * old_normal_vel
                        < dt * dt
                            * (ext_f_accelerations[pair[0]] + ext_f_accelerations[pair[1]]).mag_sq()
                    {
                        // don't bounce if the normal velocity is very small to avoid jitter
                        0.0
                    } else {
                        (body_refs[pair[0]].rb.material)
                            .restitution_with(&body_refs[pair[1]].rb.material)
                    };
                    let delta_normal_vel = -normal_vel - restitution_coef * old_normal_vel.max(0.0);

                    // dynamic friction

                    let tangent = m::left_normal(*contact.normal);
                    let tangent_vel = relative_vel_at_p.dot(tangent);
                    let friction_coef = (body_refs[pair[0]].rb.material)
                        .dynamic_friction_with(&body_refs[pair[1]].rb.material);
                    let max_coulomb_dv = inv_dt * lambda_n * friction_coef;
                    let delta_tan_vel =
                        tangent_vel.abs().min(max_coulomb_dv.abs()) * -tangent_vel.signum();

                    // apply impulse

                    let total_vel_update =
                        delta_normal_vel * *contact.normal + delta_tan_vel * tangent;
                    let vel_update_mag = total_vel_update.mag();
                    if vel_update_mag < 0.0001 {
                        continue;
                    }
                    let vel_update_dir = total_vel_update / vel_update_mag;
                    let offsets_wedge_dv =
                        map_pair([0, 1], |i| offsets_rotated[*i].wedge(vel_update_dir).xy);
                    let eff_inv_masses = map_pair([0, 1], |i| {
                        inv_masses[*i] + (offsets_wedge_dv[*i].powi(2) * inv_mom_inertias[*i])
                    });
                    let impulse_mag = vel_update_mag / (eff_inv_masses[0] + eff_inv_masses[1]);

                    velocities[pair[0]].linear += inv_masses[0] * impulse_mag * vel_update_dir;
                    velocities[pair[0]].angular +=
                        inv_mom_inertias[0] * impulse_mag * offsets_wedge_dv[0];
                    velocities[pair[1]].linear -= inv_masses[1] * impulse_mag * vel_update_dir;
                    velocities[pair[1]].angular -=
                        inv_mom_inertias[1] * impulse_mag * offsets_wedge_dv[1];
                }
            }

            // damping

            for (constraint, pair) in izip!(self.user_constraints.values(), &constraint_body_pairs)
            {
                let inv_masses = map_semi_pair(*pair, |b| body_refs[*b].rb.inverse_mass(), 0.0);
                let inv_mom_inertias =
                    map_semi_pair(*pair, |b| body_refs[*b].rb.inverse_moment_of_inertia(), 0.0);

                match pair.1 {
                    Some(p1) => {
                        let pair = [pair.0, p1];
                        let offsets_rotated = map_pair([0, 1], |i| {
                            poses[pair[*i]].rotation * constraint.offsets[*i]
                        });

                        let relative_vel = velocities[pair[0]].point_velocity(offsets_rotated[0])
                            - velocities[pair[1]].point_velocity(offsets_rotated[1]);
                        let relative_vel_mag = relative_vel.mag();
                        let dir = relative_vel / relative_vel_mag;

                        let offsets_wedge_dir =
                            map_pair([0, 1], |i| offsets_rotated[*i].wedge(dir).xy);
                        let eff_inv_masses = map_pair([0, 1], |i| {
                            inv_masses[*i] + (offsets_wedge_dir[*i].powi(2) * inv_mom_inertias[*i])
                        });

                        let vel_update_mag =
                            -relative_vel_mag * (constraint.linear_damping * dt).min(1.0);
                        let linear_impulse_mag =
                            vel_update_mag / (eff_inv_masses[0] + eff_inv_masses[1]);

                        velocities[pair[0]].linear += inv_masses[0] * linear_impulse_mag * dir;
                        velocities[pair[0]].angular +=
                            inv_mom_inertias[0] * linear_impulse_mag * offsets_wedge_dir[0];
                        velocities[pair[1]].linear -= inv_masses[1] * linear_impulse_mag * dir;
                        velocities[pair[1]].angular -=
                            inv_mom_inertias[1] * linear_impulse_mag * offsets_wedge_dir[1];

                        if constraint.angular_damping > 0.0 {
                            let rel_angular_vel =
                                velocities[pair[0]].angular - velocities[pair[1]].angular;
                            let ang_vel_update_mag =
                                -rel_angular_vel * (constraint.angular_damping * dt).min(1.0);
                            let angular_impulse =
                                ang_vel_update_mag / (eff_inv_masses[0] + eff_inv_masses[1]);

                            velocities[pair[1]].angular -= inv_mom_inertias[1] * angular_impulse;
                            velocities[pair[0]].angular += inv_mom_inertias[0] * angular_impulse;
                        };
                    }
                    None => {
                        let offset_rotated = poses[pair.0].rotation * constraint.offsets[0];

                        let point_vel = velocities[pair.0].point_velocity(offset_rotated);
                        let point_vel_mag = point_vel.mag();
                        let dir = point_vel / point_vel_mag;

                        let offset_wedge_dir = offset_rotated.wedge(dir).xy;
                        let eff_inv_mass =
                            inv_masses[0] + offset_wedge_dir.powi(2) * inv_mom_inertias[0];

                        let vel_update_mag =
                            -point_vel_mag * (constraint.linear_damping * dt).min(1.0);
                        let linear_impulse_mag = vel_update_mag / eff_inv_mass;

                        velocities[pair.0].linear += inv_masses[0] * linear_impulse_mag * dir;
                        velocities[pair.0].angular +=
                            inv_mom_inertias[0] * linear_impulse_mag * offset_wedge_dir;

                        if constraint.angular_damping > 0.0 {
                            let ang_vel_update_mag = velocities[pair.0].angular
                                * (constraint.angular_damping * dt).min(1.0);
                            let angular_impulse = -ang_vel_update_mag / eff_inv_mass;
                            velocities[pair.0].angular += inv_mom_inertias[0] * angular_impulse;
                        };
                    }
                }
            }

            //
            // Event gathering
            //

            for (pair, contact) in izip!(&pairs, &contacts) {
                match contact {
                    ContactResult::Zero => (),
                    _ => {
                        if let Some(mut sink) =
                            graph.get_neighbor_mut(&body_refs[pair[0]].rb, l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_body: graph::NodeRef::as_node(&body_refs[pair[1]].rb, graph),
                            }));
                        }
                        if let Some(mut sink) =
                            graph.get_neighbor_mut(&body_refs[pair[1]].rb, l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_body: graph::NodeRef::as_node(&body_refs[pair[0]].rb, graph),
                            }));
                        }
                    }
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
            if let Some(v) = rb.velocity_mut() {
                *v = vel_result;
            }
            *pose = pose_result;
        }
    }

    /// Find the first rigid body that intersects with the given point.
    pub fn query_point_body<'g>(
        &self,
        graph: &'g graph::Graph,
        l_pose: &'g graph::Layer<m::Pose>,
        l_collider: &'g graph::Layer<Collider>,
        l_body: &'g graph::Layer<RigidBody>,
        point: m::Vec2,
    ) -> Option<(
        graph::NodeRef<'g, m::Pose>,
        graph::NodeRef<'g, Collider>,
        graph::NodeRef<'g, RigidBody>,
    )> {
        l_body.iter(graph).find_map(|rb| {
            let coll = graph.get_neighbor(&rb, &l_collider)?;
            let pose = graph.get_neighbor(&rb, &l_pose)?;
            if collision::query::point_collider_bool(point, &*pose, &*coll) {
                Some((pose, coll, rb))
            } else {
                None
            }
        })
    }
}

/// References to the parts of a body that we need to find out if it collides with anything.
/// Used internally in collision detection, exposed to allow custom broad phase algorithms.
pub struct BodyRef<'a> {
    pub pose: graph::NodeRef<'a, m::Pose>,
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
