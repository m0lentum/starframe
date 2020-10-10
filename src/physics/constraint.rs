use super::{collision::narrowphase::Contact, RigidBody, Velocity};
use crate::{graph, math as m};

use itertools::izip;

#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub(crate) nodes: [graph::Node<RigidBody>; 2],
    pub(crate) impulse_bounds: (Option<f32>, Option<f32>),
    pub(crate) bias_coef: f32,
    pub(crate) ty: ConstraintType,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ConstraintType {
    Nonpenetration { contact: Contact },
}

impl ConstraintType {
    pub(crate) fn value(&self, trs: [&m::Transform; 2]) -> f32 {
        use ConstraintType::*;
        match self {
            Nonpenetration { contact } => contact.depth,
        }
    }

    pub(crate) fn jacobian(&self) -> Vec6 {
        use ConstraintType::*;
        match self {
            Nonpenetration { contact } => Vec6 {
                v1: *contact.normal,
                w1: m::left_normal(&contact.offsets[0]).dot(&contact.normal),
                v2: -*contact.normal,
                w2: m::right_normal(&contact.offsets[1]).dot(&contact.normal),
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
    fn derivative(&self, vels: [Velocity; 2]) -> f32 {
        self.v1.dot(&vels[0].linear)
            + self.w1 * vels[0].angular
            + self.v2.dot(&vels[1].linear)
            + self.w2 * vels[1].angular
    }

    fn dot(&self, other: &Vec6) -> f32 {
        self.v1.dot(&other.v1) + self.w1 * other.w1 + self.v2.dot(&other.v2) + self.w2 * other.w2
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
    pub(crate) body_indices: [usize; 2],
    pub(crate) jacobian_row: Vec6,
    pub(crate) bias: f32,
    pub(crate) bounds: (Option<f32>, Option<f32>),
    pub(crate) first_guess: f32,
}

pub(crate) fn solve_pgs(
    user_params: SolverParams,
    dt: f32,
    constraints: &[WorkingConstraint],
    velocities: &[Velocity],
    inv_masses: &[m::Vec2],
) -> Vec<f32> {
    let inv_dt = 1.0 / dt;
    let body_map: Vec<[usize; 2]> = constraints.iter().map(|c| c.body_indices).collect();

    let jacobian: Vec<Vec6> = constraints.iter().map(|c| c.jacobian_row).collect();
    let bounds: Vec<(Option<f32>, Option<f32>)> = constraints.iter().map(|c| c.bounds).collect();
    // `eta` in Cat05
    // length of constraints
    let rhs: Vec<f32> = izip!(constraints, &body_map)
        .map(|(c, bodies)| {
            let vels = map_array_2(bodies, |&b| velocities[b]);
            inv_dt * (c.bias - c.jacobian_row.derivative(vels))
        })
        .collect();
    // `B` in Cat05
    // length of constraints
    let inv_mass_x_jacobian: Vec<Vec6> = izip!(&jacobian, &body_map)
        .map(|(j, bodies)| {
            let inv_masses = map_array_2(bodies, |&b| inv_masses[b]);
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
    let mut answer: Vec<f32> = constraints.iter().map(|c| c.first_guess).collect();
    // change between iterations, separated to check for convergence
    let mut delta_answer: Vec<f32> = vec![0.0; answer.len()];

    // `a` in Cat05
    // length of velocities
    let mut imxj_x_answer: Vec<Velocity> = {
        let mut w = vec![Velocity::default(); velocities.len()];
        for (imxj, &ans, bodies) in izip!(&inv_mass_x_jacobian, &answer, &body_map) {
            // TODO: allow constraint with ground
            w[bodies[0]] += Velocity {
                linear: ans * imxj.v1,
                angular: ans * imxj.w1,
            };
            w[bodies[1]] += Velocity {
                linear: ans * imxj.v2,
                angular: ans * imxj.w2,
            };
        }
        w
    };

    for _i in 0..user_params.max_iterations {
        // PGS step
        for (bodies, jac, im_x_j, bounds, diag, rhs_elem, delta_ans, ans) in izip!(
            &body_map,
            &jacobian,
            &inv_mass_x_jacobian,
            &bounds,
            &j_x_imxj_diag,
            &rhs,
            &mut delta_answer,
            &mut answer
        ) {
            let a_1 = imxj_x_answer[bodies[0]];
            let a_2 = imxj_x_answer[bodies[1]];
            // normal Gauss-Seidel step
            let unprojected_delta_ans = (rhs_elem
                - jac.v1.dot(&a_1.linear)
                - jac.w1 * a_1.angular
                - jac.v2.dot(&a_2.linear)
                - jac.w2 * a_2.angular)
                / diag;
            // clamping total impulse (projection)
            let prev_ans = *ans;
            let unprojected_a = *ans + unprojected_delta_ans;
            match bounds {
                (Some(lower), _) if *lower > unprojected_a => *ans = *lower,
                (_, Some(upper)) if *upper < unprojected_a => *ans = *upper,
                _ => *ans = unprojected_a,
            }
            *delta_ans = *ans - prev_ans;

            imxj_x_answer[bodies[0]] += Velocity {
                linear: *delta_ans * im_x_j.v1,
                angular: *delta_ans * im_x_j.w1,
            };
            imxj_x_answer[bodies[1]] += Velocity {
                linear: *delta_ans * im_x_j.v2,
                angular: *delta_ans * im_x_j.w2,
            };
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

    answer
}

fn map_array_2<T, R>(arr: &[T; 2], mut f: impl FnMut(&T) -> R) -> [R; 2] {
    [f(&arr[0]), f(&arr[1])]
}
