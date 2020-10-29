use super::{RigidBody, Velocity};
use crate::{graph, math as m};

use itertools::izip;
use nalgebra as na;
use slotmap as sm;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub(crate) owner: graph::Node<RigidBody>,
    pub(crate) target: Option<graph::Node<RigidBody>>,
    pub(crate) impulse_bounds: (Option<f32>, Option<f32>),
    pub(crate) bias_coef: Option<f32>,
    pub(crate) ty: ConstraintType,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ConstraintType {
    Normal {
        normal: na::Unit<m::Vec2>,
        offsets: [m::Vec2; 2],
    },
}

impl ConstraintType {
    pub(crate) fn gradient(&self) -> Vec6 {
        use ConstraintType::*;
        match self {
            Normal { normal, offsets } => Vec6 {
                v1: **normal,
                w1: m::left_normal(&offsets[0]).dot(&normal),
                v2: -**normal,
                w2: m::right_normal(&offsets[1]).dot(&normal),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Vec6 {
    pub(crate) v1: m::Vec2,
    pub(crate) w1: f32,
    pub(crate) v2: m::Vec2,
    pub(crate) w2: f32,
}

impl Vec6 {
    fn dot(&self, other: &Vec6) -> f32 {
        self.v1.dot(&other.v1) + self.w1 * other.w1 + self.v2.dot(&other.v2) + self.w2 * other.w2
    }
}
impl From<[Velocity; 2]> for Vec6 {
    fn from(v: [Velocity; 2]) -> Self {
        Self {
            v1: v[0].linear,
            w1: v[0].angular,
            v2: v[1].linear,
            w2: v[1].angular,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SolverParams {
    pub max_iterations: u32,
    pub convergence: SolverConvergence,
}

impl Default for SolverParams {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            convergence: SolverConvergence::FixedCount,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SolverConvergence {
    FixedCount,
    AllElements(f32),
    VectorNorm(f32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkingConstraint {
    // indices in `velocities` and `inv_masses`
    pub body_indices: (usize, Option<usize>),
    pub jacobian_row: Vec6,
    pub bias: f32,
    pub bounds: ImpulseBounds,
    pub cache_id: ConstraintId,
}

/// Stores the impulses caused by each constraint.
/// These values are used as the solver's initial guesses next frame.
pub(crate) struct ImpulseCache {
    dynamic: HashMap<DynamicConstraintId, f32>,
    user_defined: sm::SecondaryMap<super::ConstraintHandle, f32>,
}
impl ImpulseCache {
    pub fn new() -> Self {
        ImpulseCache {
            dynamic: HashMap::new(),
            user_defined: sm::SecondaryMap::new(),
        }
    }

    fn get(&self, id: ConstraintId) -> Option<f32> {
        match id {
            ConstraintId::Dynamic(dyn_id) => self.dynamic.get(&dyn_id).map(|v| *v),
            ConstraintId::UserDefined(handle) => self.user_defined.get(handle).map(|v| *v),
        }
    }

    fn insert(&mut self, id: ConstraintId, val: f32) {
        match id {
            ConstraintId::Dynamic(dyn_id) => {
                self.dynamic.insert(dyn_id, val);
            }
            ConstraintId::UserDefined(handle) => {
                self.user_defined.insert(handle, val);
            }
        }
    }

    fn clear(&mut self) {
        self.dynamic.clear();
        self.user_defined.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConstraintId {
    Dynamic(DynamicConstraintId),
    UserDefined(super::ConstraintHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DynamicConstraintId {
    /// body indices in the *graph layer*, not the slice processed by the constraint solver
    pub body_indices: [usize; 2],
    pub constr_id: DynamicConstraintType,
}

/// An identifier for which constraint out of possible multiple between one pair.
/// There are max. two contact points, and they come out of the collision detection
/// in a temporally coherent order, so this should work
///
/// NOTE: this can fail in the case where a collistion changes from two contacts to one.
/// It's not a huge deal but should probably be dealt with
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum DynamicConstraintType {
    FirstContact,
    FirstFriction,
    SecondContact,
    SecondFriction,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ImpulseBounds {
    Constant(Option<f32>, Option<f32>),
    /// Friction needs to know about the normal force to set its bounds.
    /// Internal use only, doesn't seem useful to an end user
    ///
    /// NOTE: friction must come after normal for this to get the latest normal force!
    Depends {
        constraint_idx: usize,
        coefficient: f32,
    },
}

pub(crate) fn solve_pgs(
    user_params: SolverParams,
    dt: f32,
    constraints: &[WorkingConstraint],
    velocities: &[Velocity],
    inv_masses: &[m::Vec2],
    impulse_cache: &mut ImpulseCache,
) -> Vec<f32> {
    let inv_dt = 1.0 / dt;
    let body_map: Vec<(usize, Option<usize>)> =
        constraints.iter().map(|c| c.body_indices).collect();

    let jacobian: Vec<Vec6> = constraints.iter().map(|c| c.jacobian_row).collect();
    let bounds: Vec<ImpulseBounds> = constraints.iter().map(|c| c.bounds).collect();
    // `eta` in Cat05
    // length of constraints
    let rhs: Vec<f32> = izip!(constraints, &body_map)
        .map(|(c, bodies)| {
            let vels = [
                velocities[bodies.0],
                bodies.1.map(|b1| velocities[b1]).unwrap_or_default(),
            ];
            inv_dt * (c.bias - c.jacobian_row.dot(&vels.into()))
        })
        .collect();
    // `B` in Cat05
    // length of constraints
    let inv_mass_x_jacobian: Vec<Vec6> = izip!(&jacobian, &body_map)
        .map(|(j, bodies)| {
            let inv_masses = [
                inv_masses[bodies.0],
                bodies
                    .1
                    .map(|b1| inv_masses[b1])
                    .unwrap_or(m::Vec2::zeros()),
            ];
            Vec6 {
                v1: inv_masses[0][0] * j.v1,
                w1: inv_masses[0][1] * j.w1,
                v2: inv_masses[1][0] * j.v2,
                w2: inv_masses[1][1] * j.w2,
            }
        })
        .collect();
    // `d` in Cat05
    // length of constraints
    let j_x_imxj_diag: Vec<f32> = izip!(&jacobian, &inv_mass_x_jacobian)
        .map(|(j, bj)| j.dot(bj))
        .collect();

    // `lambda` in Cat05
    // length of constraints
    let mut answer: Vec<f32> = constraints
        .iter()
        .map(|c| impulse_cache.get(c.cache_id).unwrap_or(0.0))
        .collect();
    // change between iterations, separated to check for convergence
    let mut delta_answer: Vec<f32> = vec![0.0; answer.len()];

    // `a` in Cat05
    // length of velocities
    let mut imxj_x_answer: Vec<Velocity> = {
        let mut w = vec![Velocity::default(); velocities.len()];
        for (imxj, &ans, bodies) in izip!(&inv_mass_x_jacobian, &answer, &body_map) {
            w[bodies.0] += Velocity {
                linear: ans * imxj.v1,
                angular: ans * imxj.w1,
            };
            if let Some(b1) = bodies.1 {
                w[b1] += Velocity {
                    linear: ans * imxj.v2,
                    angular: ans * imxj.w2,
                };
            }
        }
        w
    };

    for _i in 0..user_params.max_iterations {
        // PGS step
        for (curr_idx, (bodies, jac, im_x_j, bounds, diag, rhs_elem, delta_ans)) in izip!(
            &body_map,
            &jacobian,
            &inv_mass_x_jacobian,
            &bounds,
            &j_x_imxj_diag,
            &rhs,
            &mut delta_answer,
        )
        .enumerate()
        {
            let a_1 = imxj_x_answer[bodies.0];
            let a_2_dot_jac = match bodies.1 {
                Some(b1) => {
                    let a_2 = imxj_x_answer[b1];
                    jac.v2.dot(&a_2.linear) + jac.w2 * a_2.angular
                }
                None => 0.0,
            };
            // normal Gauss-Seidel step
            let unprojected_delta_ans =
                (rhs_elem - jac.v1.dot(&a_1.linear) - jac.w1 * a_1.angular - a_2_dot_jac) / diag;

            // clamping total impulse (projection)
            // check if the bounds depend on another constraint (friction).
            // not totally sure how numerically robust this is,
            // but seems to work pretty well in practice
            let actual_bounds = match bounds {
                ImpulseBounds::Constant(l, r) => (*l, *r),
                ImpulseBounds::Depends {
                    constraint_idx,
                    coefficient,
                } => {
                    let b = (answer[*constraint_idx] * coefficient).abs();
                    (Some(-b), Some(b))
                }
            };

            // answer not borrowed by iterator because we need to index into it in the dependency check above
            let ans = &mut answer[curr_idx];

            let prev_ans = *ans;
            let unprojected_a = *ans + unprojected_delta_ans;

            match actual_bounds {
                (Some(lower), _) if lower > unprojected_a => *ans = lower,
                (_, Some(upper)) if upper < unprojected_a => *ans = upper,
                _ => *ans = unprojected_a,
            }
            *delta_ans = *ans - prev_ans;

            imxj_x_answer[bodies.0] += Velocity {
                linear: *delta_ans * im_x_j.v1,
                angular: *delta_ans * im_x_j.w1,
            };
            if let Some(b1) = bodies.1 {
                imxj_x_answer[b1] += Velocity {
                    linear: *delta_ans * im_x_j.v2,
                    angular: *delta_ans * im_x_j.w2,
                };
            }
        }

        // check for convergence

        use SolverConvergence::*;
        if match user_params.convergence {
            FixedCount => false,
            AllElements(limit) => delta_answer.iter().all(|&x| x.abs() <= limit),
            VectorNorm(limit) => {
                delta_answer.iter().fold(0.0, |acc, x| acc + x * x) < limit * limit
            }
        } {
            break;
        }
    }

    // replace cache
    impulse_cache.clear();
    for (ans, c) in izip!(&answer, constraints) {
        impulse_cache.insert(c.cache_id, *ans);
    }

    answer
}
