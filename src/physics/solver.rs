use super::{
    constraint::ConstraintTargets, constraint_graph::ConstraintGraphEdge, Body, BodyKey, Collider,
    ColliderKey, Constraint, ConstraintGraph, ContactResult, EntitySet, ForceField, PhysicsPose,
    TuningConstants,
};
use crate::math::{symmetric_product_3, uv};

use itertools::izip;

/// View into the working buffers created in physics::tick
/// and some other data for a single island.
pub struct DataView<'a> {
    // global / per island
    pub dt: f64,
    pub consts: &'a TuningConstants,
    pub constraint_graph: &'a ConstraintGraph,
    /// global index of the first body in the island
    pub body_index_offset: usize,
    /// global index of the first constraint in the island
    pub constraint_index_offset: usize,
    /// map from the entity_set body storage to the sorted order
    pub global_body_order: &'a [usize],

    // per body
    pub bodies: &'a mut [Body],
    pub old_poses: &'a mut [PhysicsPose],
    pub inertial_poses: &'a mut [PhysicsPose],

    // per constraint
    pub constraints: &'a [Constraint],
    pub constraint_stiffnesses: &'a mut [f64],
    pub constraint_lagrange_mults: &'a mut [f64],

    // per contacting pair
    pub coll_pairs: &'a [[ColliderKey; 2]],
    pub contacts: &'a mut [ContactResult],
    pub contact_stiffnesses: &'a mut [f64],
    pub contact_lagrange_mults: &'a mut [f64],
    pub last_contacts: &'a mut [ContactResult],
}

// SAFETY: we only use these inside of the solver
// where we make sure we don't drop anything before every view is solved
unsafe impl Sync for DataView<'_> {}
unsafe impl Send for DataView<'_> {}

/// Get the index of the body connected to a collider within this island's slice.
fn get_collider_body(
    global_body_order: &[usize],
    island_offset: usize,
    coll_key: ColliderKey,
    entity_set: &EntitySet,
) -> Option<usize> {
    let body_key = entity_set.coll_bodies.get(coll_key.0)?;
    let slot = body_key.0.slot() as usize;
    Some(global_body_order[slot] - island_offset)
}

pub fn solve(forcefield: &impl ForceField, data: &mut DataView<'_>, entity_set: &EntitySet) {
    // initialize inertial poses and solution guesses
    // TODO: adaptive initialization

    let dt = data.dt;
    let dt_sq = dt.powi(2);
    let inv_dt = 1. / dt;
    let inv_dt_sq = 1. / dt_sq;

    for (body, old_pose, inertial_pose) in izip!(
        &mut *data.bodies,
        &mut *data.old_poses,
        &mut *data.inertial_poses,
    ) {
        let ext_accel = if !body.ignores_gravity && body.mass.is_finite() {
            forcefield.value_at(body.pose.translation)
        } else {
            uv::DVec2::zero()
        };

        *inertial_pose = PhysicsPose {
            translation: body.pose.translation + data.dt * body.velocity.linear + dt_sq * ext_accel,
            rotation: uv::DRotor2::from_angle(data.dt * body.velocity.angular) * body.pose.rotation,
        };

        *old_pose = body.pose;
        body.pose = *inertial_pose;
    }

    // main part of the solver

    // reusable buffer to collect poses for constraints,
    // since there can be a variable number of bodies involved
    let mut pose_buf: Vec<PhysicsPose> = Vec::with_capacity(2);

    for _ in 0..data.consts.iterations {
        for (local_body_idx, inertial_pose) in data.inertial_poses.iter_mut().enumerate() {
            let body = data.bodies[local_body_idx];
            if !body.sees_forces() {
                continue;
            };

            let pos_diff = body.pose.translation - inertial_pose.translation;
            let rot_diff = body.pose.rotation * inertial_pose.rotation.reversed();

            let inertial_force = -inv_dt_sq
                * uv::DVec3::new(
                    body.mass * pos_diff.x,
                    body.mass * pos_diff.y,
                    body.moment_of_inertia * 2. * rot_diff.bv.xy,
                );
            let mass_hessian = uv::DMat3::from([
                [inv_dt_sq * body.mass, 0., 0.],
                [0., inv_dt_sq * body.mass, 0.],
                [0., 0., inv_dt_sq * body.moment_of_inertia],
            ]);

            let mut total_force = inertial_force;
            let mut total_hessian = mass_hessian;

            for constraint in data
                .constraint_graph
                .body_iter(local_body_idx + data.body_index_offset)
            {
                match constraint {
                    ConstraintGraphEdge::Contact {
                        other_body_idx,
                        pair_idx,
                        instance_idx,
                    } => {
                        // TODO
                        continue;
                    }
                    // TODO for multibody constraints
                    // this will repeat the same constraint multiple times,
                    // need to deduplicate somehow
                    ConstraintGraphEdge::Constraint {
                        other_body_idx,
                        constr_idx,
                        instance_idx,
                    } => {
                        let ci = constr_idx - data.constraint_index_offset;
                        let constr = &data.constraints[ci];
                        let stiffness = data.constraint_stiffnesses[ci];
                        let lambda = data.constraint_lagrange_mults[ci];

                        pose_buf.clear();
                        pose_buf.extend(constr.target.iter().map(|bk| {
                            let body_idx = data.global_body_order[bk.0.slot() as usize]
                                - data.body_index_offset;
                            data.bodies[body_idx].pose
                        }));
                        let c_vals = constr.compute_derivatives(&pose_buf, instance_idx);

                        let force = if stiffness.is_infinite() {
                            -(stiffness * c_vals.value + lambda)
                                .clamp(constr.limits.0, constr.limits.1)
                                * c_vals.gradient
                        } else {
                            -stiffness * c_vals.value * c_vals.gradient
                        };
                        total_force += force;

                        // TODO: test the diagonalized version of c_vals.hessian
                        let hess =
                            stiffness * symmetric_product_3(c_vals.gradient) + c_vals.hessian;
                        total_hessian += hess;
                    }
                };

                let body = &mut data.bodies[local_body_idx];
                let correction = total_hessian.inversed() * total_force;
                body.pose.translation += correction.xy();
                body.pose.rotation = (body.pose.rotation
                    + uv::DRotor2::new(0., uv::DBivec2::new(0.5 * correction.z))
                        * body.pose.rotation)
                    .normalized();
            }
        }

        // update stiffnesses and Lagrange multipliers

        for (constr, stiffness, lambda) in izip!(
            data.constraints,
            &mut *data.constraint_stiffnesses,
            &mut *data.constraint_lagrange_mults
        ) {
            pose_buf.clear();
            pose_buf.extend(constr.target.iter().map(|bk| {
                let body_idx =
                    data.global_body_order[bk.0.slot() as usize] - data.body_index_offset;
                data.bodies[body_idx].pose
            }));

            let c_val = constr.compute_value(&pose_buf);
            // TODO what if there are limits on a compliant constraint?
            if constr.stiffness.is_infinite() {
                let next_lambda = *stiffness * c_val + *lambda;
                if next_lambda < constr.limits.0 {
                    *lambda = constr.limits.0;
                } else if next_lambda > constr.limits.1 {
                    *lambda = constr.limits.1;
                } else {
                    // stiffness only updates if lambda wasn't clamped
                    *stiffness += data.consts.stiffness_growth_coef * c_val.abs();
                }
            } else {
                *stiffness = f64::min(
                    constr.stiffness,
                    *stiffness + data.consts.stiffness_growth_coef * c_val.abs(),
                );
            }
        }

        // TODO: same for contacts
    }

    // update velocities from pose differences

    for (old_pose, body) in izip!(&mut *data.old_poses, &mut *data.bodies) {
        body.velocity.linear = (body.pose.translation - old_pose.translation) * inv_dt;
        let rot_diff = body.pose.rotation * old_pose.rotation.reversed();
        let angle_diff = -rot_diff.bv.xy.atan2(rot_diff.s) * 2.0;
        body.velocity.angular = angle_diff * inv_dt;
    }

    // TODO: how to do damping?
}
