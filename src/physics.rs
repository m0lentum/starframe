use crate::{
    event::{Event, EventSink},
    graph::{self, UnsafeNode},
    math::{self as m, Angle},
};

use itertools::izip;
use slotmap as sm;

//

pub mod collision;
use collision::{shape_shape::intersection_check, SpatialIndex};
pub use collision::{Collider, ColliderType, Contact, ContactResult, Material};

mod constraint;
pub use constraint::*;

pub mod forcefield;
pub use forcefield::ForceField;

mod body;
pub use body::*;

mod rope;
pub use rope::*;

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
    /// The collider that this body was in contact with.
    pub other_collider: graph::Node<Collider>,
}

sm::new_key_type! {
    pub struct ConstraintHandle;
}

pub struct Physics {
    pub substeps: usize,
    pub mask_matrix: collision::MaskMatrix,
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
            mask_matrix: Default::default(),
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
        l_body: &mut graph::Layer<Body>,
        l_collider: &graph::Layer<Collider>,
        l_rope: &graph::Layer<Rope>,
        l_evt_sink: &mut graph::Layer<EventSink>,
        dt: f64,
        forcefield: &impl ForceField,
    ) {
        let dt = dt / self.substeps as f64;
        let inv_dt = 1.0 / dt;
        let inv_dt_sq = inv_dt * inv_dt;

        //
        // Setting up buffers
        //

        let body_refs: Vec<graph::NodeRef<Body>> = l_body.iter(graph).collect();
        // map from the position of a node in the graph layer to the position of a node in body_refs
        let node_ref_map: Vec<usize> = {
            let mut map = vec![0; l_body.content.len()];
            for (ref_pos, node) in body_refs.iter().enumerate() {
                map[node.pos().item_idx] = ref_pos;
            }
            map
        };
        // store indices into neighboring particles for rope nodes
        let mut rope_next_particles: Vec<Option<usize>> = vec![None; body_refs.len()];
        let mut rope_prev_particles: Vec<Option<usize>> = vec![None; body_refs.len()];
        for rope_node in l_rope.iter(graph) {
            let mut iter = RopeIter::new(rope_node, l_body, graph)
                .map(|node| node_ref_map[node.pos().item_idx])
                .peekable();
            while let Some(particle) = iter.next() {
                if let Some(next_particle) = iter.peek() {
                    rope_next_particles[particle] = Some(*next_particle);
                    rope_prev_particles[*next_particle] = Some(particle);
                }
            }
        }
        // store lateral position corrections (bending resistance) for velocity correction
        let mut rope_lateral_corrections: Vec<Option<m::Vec2>> = vec![None; body_refs.len()];

        // buffers for working variables, outside of body_refs
        // to make it simpler to mutate things without breaking borrowing rules.
        // indexed using the same index as for body_refs
        let mut old_poses: Vec<m::Pose> = body_refs
            .iter()
            .map(|b| *(graph.get_neighbor(b, l_pose)).expect("A Body didn't have a Pose"))
            .collect();
        // poses after velocity and constraints are applied, used for rope normal correction
        let mut pre_contact_poses: Vec<m::Pose> = old_poses.clone();
        // actual poses used in most calculations
        let mut poses: Vec<m::Pose> = old_poses.clone();
        // old velocities used for restitution
        let mut old_velocities: Vec<Velocity> =
            body_refs.iter().map(|body| body.velocity).collect();
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

        // assuming here that slotmap's iteration order doesn't change if the
        // contents don't change. it doesn't guarantee this in the docs so if
        // constraints start jumping from one object to another this is why
        let constraint_body_pairs: Vec<(usize, Option<usize>)> = self
            .user_constraints
            .values()
            .map(|c| {
                (
                    node_ref_map[c.owner.pos().item_idx],
                    c.target.map(|t| node_ref_map[t.pos().item_idx]),
                )
            })
            .collect();

        //
        // Set up collision detection
        //

        /// poses are in our temporary buffer for colliders attached to bodies,
        /// but for static colliders they're in the graph.
        /// because we don't modify non-body poses, we can get the poses for static colliders just once
        enum ColliderContext {
            Body(usize),
            Static(m::Pose),
        }
        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep.
        // we can map them to NodeRefs here because we won't borrow colliders mutably
        let coll_pairs: Vec<[graph::NodeRef<Collider>; 2]> = SpatialIndex::pairs(l_collider, graph)
            .iter()
            .map(|colls| map_pair(colls, |c| l_collider.get_unchecked(c.pos())))
            .filter(|[c1, c2]| self.mask_matrix.get(c1.layer, c2.layer))
            .collect();
        // indices to bodies in `body_refs` corresponding to `coll_pairs`
        let ctx_pairs: Vec<[ColliderContext; 2]> = coll_pairs
            .iter()
            .map(|colls| {
                map_pair(colls, |c| match graph.get_neighbor_unchecked(c, l_body) {
                    Some(b) => ColliderContext::Body(node_ref_map[b.pos().item_idx]),
                    None => {
                        ColliderContext::Static(match graph.get_neighbor_unchecked(c, l_pose) {
                            Some(pose) => *pose,
                            None => m::Pose::default(),
                        })
                    }
                })
            })
            .collect();

        // store latest contacts for use in the velocity step
        let mut contacts: Vec<ContactResult> = vec![ContactResult::Zero; coll_pairs.len()];
        // store contact forces for friction purposes
        let mut contact_lambdas: Vec<f64> = vec![0.0; coll_pairs.len()];

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
                if let Mass::Finite { .. } = body.mass {
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
            // Rope constraints
            //

            rope_lateral_corrections.iter_mut().for_each(|c| {
                *c = None;
            });
            for rope in l_rope.iter(graph) {
                let first_particle = graph.get_neighbor(&rope, l_body).unwrap().pos().item_idx;
                // solve constraints
                let mut curr_particle = first_particle;
                let mut next_particle = rope_next_particles[curr_particle].unwrap();
                loop {
                    let dist = poses[next_particle].translation - poses[curr_particle].translation;
                    let dist_mag = dist.mag();
                    let dir = dist / dist_mag;
                    let error = rope.spacing - dist_mag;

                    let lambda = -error
                        / (body_refs[curr_particle].mass.inv()
                            + body_refs[next_particle].mass.inv()
                            + rope.compliance * inv_dt_sq);

                    poses[curr_particle]
                        .append_translation(body_refs[curr_particle].mass.inv() * lambda * dir);
                    poses[next_particle]
                        .append_translation(-body_refs[next_particle].mass.inv() * lambda * dir);

                    let particle_after_next = match rope_next_particles[next_particle] {
                        Some(next) => next,
                        None => break,
                    };

                    // curvature constraint between last three particles

                    let curr_to_next =
                        poses[next_particle].translation - poses[curr_particle].translation;
                    let next_to_after =
                        poses[particle_after_next].translation - poses[next_particle].translation;
                    let angle = next_to_after
                        .normalized()
                        .dot(curr_to_next.normalized())
                        .acos();
                    let error = angle - rope.bending_max_angle;
                    if error > 0.0 {
                        let lambda = -error
                            / (body_refs[particle_after_next].mass.inv()
                                + rope.bending_compliance * inv_dt_sq);

                        let lambda_oriented =
                            if m::left_normal(curr_to_next).dot(next_to_after) > 0.0 {
                                lambda
                            } else {
                                -lambda
                            };
                        let correction = m::Rotor2::from_angle(
                            lambda_oriented * body_refs[particle_after_next].mass.inv(),
                        );
                        let old_pos = poses[particle_after_next].translation;
                        poses[particle_after_next].translation =
                            poses[next_particle].translation + correction * next_to_after;

                        rope_lateral_corrections[particle_after_next] =
                            Some(poses[particle_after_next].translation - old_pos);
                    }

                    curr_particle = next_particle;
                    next_particle = particle_after_next;
                }
            }

            //
            // User-defined constraints
            //

            for (constraint, pair) in izip!(self.user_constraints.values(), &constraint_body_pairs)
            {
                let inv_masses = map_semi_pair(*pair, |b| body_refs[*b].mass.inv(), 0.0);
                let inv_mom_inertias =
                    map_semi_pair(*pair, |b| body_refs[*b].moment_of_inertia.inv(), 0.0);

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
                                    let pair = [pair.0, p1];
                                    let offsets_rotated = map_pair(&[0, 1], |i| {
                                        poses[pair[*i]].rotation * constraint.offsets[*i]
                                    });
                                    let offsets_wedge_dir =
                                        map_pair(&[0, 1], |i| offsets_rotated[*i].wedge(dir).xy);
                                    let eff_inv_masses = map_pair(&[0, 1], |i| {
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

            for (pose, pre_cont_pose) in izip!(&poses, &mut pre_contact_poses) {
                *pre_cont_pose = *pose;
            }

            for (colls, ctxs, contact, lambda_n) in
                izip!(&coll_pairs, &ctx_pairs, &mut contacts, &mut contact_lambdas)
            {
                if !match ctxs[0] {
                    ColliderContext::Body(bi) => body_refs[bi].sees_forces(),
                    ColliderContext::Static(_) => false,
                } && !match ctxs[1] {
                    ColliderContext::Body(bi) => body_refs[bi].sees_forces(),
                    ColliderContext::Static(_) => false,
                } {
                    // both bodies are kinematic or static, skip this pair
                    *contact = ContactResult::Zero;
                    continue;
                }

                // check for collision
                *contact = {
                    let poses = map_pair(ctxs, |ctx| match ctx {
                        ColliderContext::Body(b) => poses[*b],
                        ColliderContext::Static(pose) => *pose,
                    });
                    intersection_check(&poses[0], &*colls[0], &poses[1], &*colls[1])
                };

                // if one of the bodies is from a rope, adjust normal
                // to perpendicular to the rope *before* any contacts
                //
                // (because rope colliders are circles, only the One case is possible here)
                if let ContactResult::One(contact) = contact {
                    for (ctx, normal_dir) in izip!(ctxs, [1.0, -1.0]) {
                        if let ColliderContext::Body(bi) = ctx {
                            if let (Some(prev), Some(next)) =
                                (rope_prev_particles[*bi], rope_next_particles[*bi])
                            {
                                let normal_oriented = *contact.normal * normal_dir;
                                let to_prev = pre_contact_poses[prev].translation
                                    - pre_contact_poses[*bi].translation;
                                let to_next = pre_contact_poses[next].translation
                                    - pre_contact_poses[*bi].translation;
                                let new_normal = if normal_oriented.dot(to_prev)
                                    > normal_oriented.dot(to_next)
                                {
                                    m::Unit::new_normalize(m::left_normal(to_prev))
                                } else {
                                    m::Unit::new_normalize(m::left_normal(to_next))
                                };
                                contact.normal = if contact.normal.dot(*new_normal) > 0.0 {
                                    new_normal
                                } else {
                                    -new_normal
                                };
                            }
                        }
                    }
                }

                let materials = match (colls[0].ty, colls[1].ty) {
                    (ColliderType::Solid(m0), ColliderType::Solid(m1)) => [m0, m1],
                    // one of the colliders was a trigger, no physics response
                    _ => {
                        continue;
                    }
                };

                for contact in contact.iter() {
                    // tangent for static friction
                    let tangent = m::left_normal(*contact.normal);

                    // gather variables into a struct because they're different
                    // for static and dynamic bodies and this lets us get them in one match
                    struct WorkingVars {
                        // we can't return depth directly from collision detection because
                        // earlier position corrections can change it,
                        // thus we compute depth here from the points on each object's surface
                        offset_worldspace: m::Vec2,
                        offset_wedge_normal: f64,
                        eff_inv_mass_n: f64,
                        // for friction
                        offset_worldspace_old: m::Vec2,
                        offset_wedge_tan: f64,
                        eff_inv_mass_tan: f64,
                    }
                    let vars = map_pair(&[0, 1], |i| {
                        match ctxs[*i] {
                            // no body attached -> static body, infinite mass
                            ColliderContext::Static(pose) => {
                                let offset_worldspace = pose * contact.offsets[*i];
                                WorkingVars {
                                    offset_worldspace,
                                    offset_wedge_normal: 0.0,
                                    eff_inv_mass_n: 0.0,
                                    offset_worldspace_old: offset_worldspace,
                                    offset_wedge_tan: 0.0,
                                    eff_inv_mass_tan: 0.0,
                                }
                            }
                            ColliderContext::Body(bi) => {
                                let im = body_refs[bi].mass.inv();
                                let imi = body_refs[bi].moment_of_inertia.inv();
                                let offset_rotated = poses[bi].rotation * contact.offsets[*i];
                                let offset_wedge_normal = offset_rotated.wedge(*contact.normal).xy;
                                let offset_wedge_tan = offset_rotated.wedge(tangent).xy;

                                WorkingVars {
                                    offset_worldspace: poses[bi] * contact.offsets[*i],
                                    offset_wedge_normal,
                                    eff_inv_mass_n: im + (offset_wedge_normal.powi(2) * imi),
                                    offset_worldspace_old: old_poses[bi] * contact.offsets[*i],
                                    offset_wedge_tan,
                                    eff_inv_mass_tan: im + (offset_wedge_tan.powi(2) * imi),
                                }
                            }
                        }
                    });

                    let depth = (vars[0].offset_worldspace - vars[1].offset_worldspace)
                        .dot(*contact.normal);

                    if depth <= 0.0 {
                        *lambda_n = 0.0;
                        continue;
                    }

                    *lambda_n = -depth / (vars[0].eff_inv_mass_n + vars[1].eff_inv_mass_n);

                    if let ColliderContext::Body(bi) = ctxs[0] {
                        let im = body_refs[bi].mass.inv();
                        let imi = body_refs[bi].moment_of_inertia.inv();
                        poses[bi].append_translation(im * *lambda_n * *contact.normal);
                        poses[bi].prepend_rotation(
                            Angle::Rad(imi * *lambda_n * vars[0].offset_wedge_normal).into(),
                        );
                    }
                    if let ColliderContext::Body(bi) = ctxs[1] {
                        let im = body_refs[bi].mass.inv();
                        let imi = body_refs[bi].moment_of_inertia.inv();
                        poses[bi].append_translation(-im * *lambda_n * *contact.normal);
                        poses[bi].prepend_rotation(
                            Angle::Rad(-imi * *lambda_n * vars[1].offset_wedge_normal).into(),
                        );
                    }

                    // static friction

                    let offset_diff_motion = (vars[0].offset_worldspace
                        - vars[0].offset_worldspace_old)
                        - (vars[1].offset_worldspace - vars[1].offset_worldspace_old);
                    let motion_along_tan = offset_diff_motion.dot(tangent);

                    let friction_coef = materials[0].static_friction_with(&materials[1]);
                    let max_coulomb_dx = *lambda_n * friction_coef;

                    let lambda_t =
                        -motion_along_tan / (vars[0].eff_inv_mass_tan + vars[1].eff_inv_mass_tan);

                    if lambda_t < max_coulomb_dx {
                        if let ColliderContext::Body(bi) = ctxs[0] {
                            let im = body_refs[bi].mass.inv();
                            let imi = body_refs[bi].moment_of_inertia.inv();
                            poses[bi].append_translation(im * lambda_t * tangent);
                            poses[bi].prepend_rotation(
                                Angle::Rad(imi * lambda_t * vars[0].offset_wedge_tan).into(),
                            );
                        }
                        if let ColliderContext::Body(bi) = ctxs[1] {
                            let im = body_refs[bi].mass.inv();
                            let imi = body_refs[bi].moment_of_inertia.inv();
                            poses[bi].append_translation(-im * lambda_t * tangent);
                            poses[bi].prepend_rotation(
                                Angle::Rad(-imi * lambda_t * vars[1].offset_wedge_tan).into(),
                            );
                        }
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

            for (colls, ctxs, contact, lambda_n) in
                izip!(&coll_pairs, &ctx_pairs, &contacts, &contact_lambdas)
            {
                let materials = match (colls[0].ty, colls[1].ty) {
                    (ColliderType::Solid(m0), ColliderType::Solid(m1)) => [m0, m1],
                    // one of the colliders was a trigger, no physics response
                    _ => {
                        continue;
                    }
                };

                for contact in contact.iter() {
                    struct WorkingVars {
                        inv_mass: f64,
                        inv_mom_inertia: f64,
                        offset_rotated: m::Vec2,
                        point_vel: m::Vec2,
                        old_point_vel: m::Vec2,
                        ext_f_accel: m::Vec2,
                    }
                    let vars = map_pair(&[0, 1], |i| match ctxs[*i] {
                        // no body => infinite mass
                        ColliderContext::Static(pose) => WorkingVars {
                            inv_mass: 0.0,
                            inv_mom_inertia: 0.0,
                            offset_rotated: pose.rotation * contact.offsets[*i],
                            point_vel: m::Vec2::zero(),
                            old_point_vel: m::Vec2::zero(),
                            ext_f_accel: m::Vec2::zero(),
                        },
                        ColliderContext::Body(bi) => {
                            let offset_rotated = poses[bi].rotation * contact.offsets[*i];
                            WorkingVars {
                                inv_mass: body_refs[bi].mass.inv(),
                                inv_mom_inertia: body_refs[bi].moment_of_inertia.inv(),
                                offset_rotated,
                                point_vel: velocities[bi].point_velocity(offset_rotated),
                                old_point_vel: old_velocities[bi].point_velocity(offset_rotated),
                                ext_f_accel: ext_f_accelerations[bi],
                            }
                        }
                    });

                    let relative_vel_at_p = vars[0].point_vel - vars[1].point_vel;

                    // restitution

                    let normal_vel = relative_vel_at_p.dot(*contact.normal);
                    let old_rel_vel = vars[0].old_point_vel - vars[1].old_point_vel;
                    let old_normal_vel = old_rel_vel.dot(*contact.normal);
                    let restitution_coef = if old_normal_vel * old_normal_vel
                        < dt * dt * (vars[0].ext_f_accel + vars[1].ext_f_accel).mag_sq()
                    {
                        // don't bounce if the normal velocity is very small to avoid jitter
                        0.0
                    } else {
                        materials[0].restitution_with(&materials[1])
                    };
                    let delta_normal_vel = -normal_vel - restitution_coef * old_normal_vel.max(0.0);

                    // dynamic friction

                    let tangent = m::left_normal(*contact.normal);
                    let tangent_vel = relative_vel_at_p.dot(tangent);
                    let friction_coef = materials[0].dynamic_friction_with(&materials[1]);
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
                    let offsets_wedge_dv = map_pair(&[0, 1], |i| {
                        vars[*i].offset_rotated.wedge(vel_update_dir).xy
                    });
                    let eff_inv_masses = map_pair(&[0, 1], |i| {
                        vars[*i].inv_mass
                            + (offsets_wedge_dv[*i].powi(2) * vars[*i].inv_mom_inertia)
                    });
                    let impulse_mag = vel_update_mag / (eff_inv_masses[0] + eff_inv_masses[1]);

                    if let ColliderContext::Body(bi) = ctxs[0] {
                        velocities[bi].linear += vars[0].inv_mass * impulse_mag * vel_update_dir;
                        velocities[bi].angular +=
                            vars[0].inv_mom_inertia * impulse_mag * offsets_wedge_dv[0];
                    }
                    if let ColliderContext::Body(bi) = ctxs[1] {
                        velocities[bi].linear -= vars[1].inv_mass * impulse_mag * vel_update_dir;
                        velocities[bi].angular -=
                            vars[1].inv_mom_inertia * impulse_mag * offsets_wedge_dv[1];
                    }
                }
            }

            // damping

            for (constraint, pair) in izip!(self.user_constraints.values(), &constraint_body_pairs)
            {
                let inv_masses = map_semi_pair(*pair, |b| body_refs[*b].mass.inv(), 0.0);
                let inv_mom_inertias =
                    map_semi_pair(*pair, |b| body_refs[*b].moment_of_inertia.inv(), 0.0);

                match pair.1 {
                    Some(p1) => {
                        let pair = [pair.0, p1];
                        let offsets_rotated = map_pair(&[0, 1], |i| {
                            poses[pair[*i]].rotation * constraint.offsets[*i]
                        });

                        let relative_vel = velocities[pair[0]].point_velocity(offsets_rotated[0])
                            - velocities[pair[1]].point_velocity(offsets_rotated[1]);
                        let relative_vel_mag = relative_vel.mag();
                        if relative_vel_mag == 0.0 {
                            continue;
                        }
                        let dir = relative_vel / relative_vel_mag;

                        let offsets_wedge_dir =
                            map_pair(&[0, 1], |i| offsets_rotated[*i].wedge(dir).xy);
                        let eff_inv_masses = map_pair(&[0, 1], |i| {
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
                        if point_vel_mag == 0.0 {
                            continue;
                        }
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
            // Damping and velocity correction for ropes
            //

            for rope in l_rope.iter(graph) {
                let first_particle = graph.get_neighbor(&rope, l_body).unwrap().pos().item_idx;
                let mut curr_particle = first_particle;
                let mut next_particle = rope_next_particles[curr_particle].unwrap();
                loop {
                    let relative_vel =
                        velocities[curr_particle].linear - velocities[next_particle].linear;
                    let relative_vel_mag = relative_vel.mag();
                    if relative_vel_mag != 0.0 {
                        let dir = relative_vel / relative_vel_mag;
                        let vel_update_mag = -relative_vel_mag * (rope.damping * dt).min(1.0);

                        let linear_impulse_mag = vel_update_mag
                            / (body_refs[curr_particle].mass.inv()
                                + body_refs[next_particle].mass.inv());

                        velocities[curr_particle].linear +=
                            body_refs[curr_particle].mass.inv() * linear_impulse_mag * dir;
                        velocities[next_particle].linear -=
                            body_refs[next_particle].mass.inv() * linear_impulse_mag * dir;
                    }

                    curr_particle = next_particle;
                    next_particle = match rope_next_particles[next_particle] {
                        Some(next) => next,
                        None => break,
                    };
                }

                // velocity correction to prevent bouncing if there was a lateral position correction
                let mut particle = first_particle;
                loop {
                    if let Some(corr) = rope_lateral_corrections[particle] {
                        let corr_mag = corr.mag();
                        // velocity "created" by the correction, used as a maximum bound on
                        // velocity correction to keep velocity from e.g. gravity
                        let vel_from_corr = corr_mag * inv_dt;

                        let dir = corr / corr_mag;
                        let vel_in_dir = velocities[particle].linear.dot(dir);
                        let vel_clamped = vel_in_dir.min(vel_from_corr).max(-vel_from_corr);

                        let impulse_mag = -vel_clamped
                            / (body_refs[particle].mass.inv() + rope.bending_compliance * inv_dt);
                        velocities[particle].linear +=
                            body_refs[particle].mass.inv() * impulse_mag * dir;
                    }

                    particle = match rope_next_particles[particle] {
                        Some(next) => next,
                        None => break,
                    }
                }
            }

            //
            // Event gathering
            //

            for (colls, contact) in izip!(&coll_pairs, &contacts) {
                match contact {
                    ContactResult::Zero => (),
                    _ => {
                        if let Some(mut sink) =
                            graph.get_neighbor_mut_unchecked(&colls[0], l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_collider: graph::NodeRef::as_node(&colls[1], graph),
                            }));
                        }
                        if let Some(mut sink) =
                            graph.get_neighbor_mut_unchecked(&colls[1], l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_collider: graph::NodeRef::as_node(&colls[0], graph),
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
        let body_nodes: Vec<graph::NodePosition> =
            body_refs.into_iter().map(|br| br.pos()).collect();
        for (body, pose_result, vel_result) in izip!(body_nodes, poses, velocities) {
            let mut body = l_body.get_mut_unchecked(body);
            let mut pose = graph.get_neighbor_mut_unchecked(&body, l_pose).unwrap();
            body.velocity = vel_result;
            *pose = pose_result;
        }
    }

    /// Find the first rigid body that intersects with the given point.
    pub fn query_point_body<'g>(
        &self,
        graph: &'g graph::Graph,
        l_pose: &'g graph::Layer<m::Pose>,
        l_collider: &'g graph::Layer<Collider>,
        l_body: &'g graph::Layer<Body>,
        point: m::Vec2,
    ) -> Option<(
        graph::NodeRef<'g, m::Pose>,
        graph::NodeRef<'g, Collider>,
        graph::NodeRef<'g, Body>,
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
//
// helpers to reduce duplication when fetching info for pairs of objects
fn map_pair<T, R>(pair: &[T; 2], f: impl Fn(&T) -> R) -> [R; 2] {
    [f(&pair[0]), f(&pair[1])]
}

fn map_semi_pair<T, R>(pair: (T, Option<T>), f: impl Fn(&T) -> R, snd_default: R) -> [R; 2] {
    [f(&pair.0), pair.1.map(|x| f(&x)).unwrap_or(snd_default)]
}
