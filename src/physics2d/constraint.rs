use crate::physics2d as phys;

use std::{collections::HashMap, marker::PhantomData};
use ultraviolet as uv;

#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub(crate) ids: [usize; 2],
    pub(crate) normal: uv::Vec2,
    pub(crate) offsets: [uv::Vec2; 2],
    pub(crate) impulse_bounds: (Option<f32>, Option<f32>),
    pub(crate) bias: f32,
}

#[derive(Debug)]
struct ConstraintAccumulator<'a> {
    ids: [usize; 2],
    indices: [usize; 2],
    constraint: &'a Constraint,
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
        items: impl IntoIterator<Item = &'a ConstraintAccumulator<'a>>,
    ) {
        self.0 = items
            .into_iter()
            .map(|acc| {
                let ids = if acc.ids[0] < acc.ids[1] {
                    acc.ids
                } else {
                    [acc.ids[1], acc.ids[0]]
                };
                (ids, acc.total_impulse)
            })
            .collect();
    }
}

/// A System that tries to enforce any constraints present in the physics world.
pub struct ConstraintSolver<'a, I>
where
    I: phys::Integrator,
{
    timestep: f32,
    constraints: &'a [Constraint],
    impulse_cache: &'a mut ImpulseCache,
    loop_condition: SolverLoopCondition,
    forcefield: phys::ForceField,
    _integrator_marker: PhantomData<I>,
}

impl<'a, I> ConstraintSolver<'a, I>
where
    I: phys::Integrator,
{
    pub fn new(
        timestep: f32,
        constraints: &'a [Constraint],
        impulse_cache: &'a mut ImpulseCache,
        loop_condition: SolverLoopCondition,
        ff: impl Into<phys::ForceField>,
    ) -> Self {
        ConstraintSolver {
            timestep,
            constraints,
            impulse_cache,
            loop_condition,
            forcefield: ff.into(),
            _integrator_marker: PhantomData,
        }
    }

    fn tick(&mut self) {
        // TODO: figure out how to bring stuff into this with the new model

        // apply environment forces (gravity, usually)
        // for item in items.iter_mut() {
        //     if let Some(vel) = item.body.velocity_mut() {
        //         vel.linear += self.forcefield.value_at(item.tr.0.translation) * self.timestep;
        //     }
        // }

        // let id_index_map: HashMap<usize, usize> = items
        //     .iter()
        //     .enumerate()
        //     .map(|(index, item)| (item.id, index))
        //     .collect();

        // let mut integrator = I::begin_step(self.timestep);

        // while let phys::integrator::IntegratorState::NeedsDerivatives = integrator.substep(
        //     items
        //         .iter_mut()
        //         .filter_map(|rbf| match rbf.body.velocity_mut() {
        //             Some(vel) => Some((&mut *rbf.tr, vel)),
        //             None => None,
        //         }),
        // ) {
        //     fn map_array_2<T, R>(arr: &[T; 2], mut f: impl FnMut(&T) -> R) -> [R; 2] {
        //         [f(&arr[0]), f(&arr[1])]
        //     }

        //     let mut accumulators = Vec::new();
        //     // Initialize accumulators
        //     for constraint in self.constraints.iter() {
        //         assert!(
        //             constraint.ids[0] != constraint.ids[1],
        //             "bug: paired an object with itself"
        //         );
        //         // every id is in the map so this can't fail
        //         let indices = [
        //             *id_index_map.get(&constraint.ids[0]).unwrap(),
        //             *id_index_map.get(&constraint.ids[1]).unwrap(),
        //         ];

        //         // sort by index so the following doesn't need comparison
        //         let (ids, indices) = if indices[0] < indices[1] {
        //             (constraint.ids, indices)
        //         } else {
        //             (
        //                 [constraint.ids[1], constraint.ids[0]],
        //                 [indices[1], indices[0]],
        //             )
        //         };

        //         // ids guaranteed unequal -> we can do this trick to get mutable ref to both
        //         let objs = {
        //             let (l, r) = items.split_at_mut(indices[1]);
        //             [&mut l[indices[0]], &mut r[0]]
        //         };

        //         // begin accumulator construction
        //         let offsets_cross_normals = map_array_2(&constraint.offsets, |offset| {
        //             offset[0] * constraint.normal[1] - constraint.normal[0] * offset[1]
        //         });
        //         let inv_masses = map_array_2(&objs, |o_| o_.body.inverse_mass());
        //         let inv_mom_inertias = map_array_2(&objs, |o_| o_.body.inverse_moment_of_inertia());
        //         let inv_masses_sum = inv_masses[0]
        //             + (inv_mom_inertias[0] * offsets_cross_normals[0] * offsets_cross_normals[0])
        //             + inv_masses[1]
        //             + (inv_mom_inertias[1] * offsets_cross_normals[1] * offsets_cross_normals[1]);

        //         // warm start
        //         let initial_impulse = if let Some(prev_impulse) = self.impulse_cache.get(ids) {
        //             if let Some(vel) = objs[0].body.velocity_mut() {
        //                 vel.linear -= inv_masses[0] * prev_impulse * constraint.normal;
        //                 vel.angular -=
        //                     inv_mom_inertias[0] * prev_impulse * offsets_cross_normals[0];
        //             }
        //             if let Some(vel) = objs[1].body.velocity_mut() {
        //                 vel.linear += inv_masses[1] * prev_impulse * constraint.normal;
        //                 vel.angular +=
        //                     inv_mom_inertias[1] * prev_impulse * offsets_cross_normals[1];
        //             }
        //             *prev_impulse
        //         } else {
        //             0.0
        //         };

        //         accumulators.push(ConstraintAccumulator {
        //             ids,
        //             indices,
        //             constraint,
        //             inv_masses,
        //             inv_mom_inertias,
        //             inv_masses_sum,
        //             offsets_cross_normals,
        //             total_impulse: initial_impulse,
        //         });
        //     }

        //     // iterative impulse accumulation
        //     let mut biggest_change = std::f32::MAX;
        //     let mut loop_count = 0;
        //     while biggest_change > self.loop_condition.convergence_threshold
        //         && loop_count < self.loop_condition.max_loops
        //     {
        //         loop_count += 1;
        //         biggest_change = 0.0;

        //         for acc in accumulators.iter_mut() {
        //             let objs = {
        //                 let (l, r) = items.split_at_mut(acc.indices[1]);
        //                 [&mut l[acc.indices[0]], &mut r[0]]
        //             };

        //             let vels = map_array_2(&objs, |o_| o_.body.velocity_or_zero());
        //             // TODO: this part is the actual constraint function and should be generalized
        //             let normal_vels = [
        //                 vels[0].linear.dot(acc.constraint.normal)
        //                     + (acc.offsets_cross_normals[0] * vels[0].angular),
        //                 vels[1].linear.dot(acc.constraint.normal)
        //                     + (acc.offsets_cross_normals[1] * vels[1].angular),
        //             ];

        //             let relative_normal_vel = normal_vels[0] - normal_vels[1] + acc.constraint.bias;

        //             let impulse_magnitude = relative_normal_vel / acc.inv_masses_sum;
        //             biggest_change = biggest_change.max(impulse_magnitude.abs());

        //             // clamp total accumulated to the constraint's bounds
        //             let new_total = acc.total_impulse + impulse_magnitude;
        //             let clamped_impulse = match acc.constraint.impulse_bounds {
        //                 (Some(lo), _) if new_total < lo => {
        //                     acc.total_impulse = lo;
        //                     impulse_magnitude - new_total
        //                 }
        //                 (_, Some(hi)) if new_total > hi => {
        //                     acc.total_impulse = hi;
        //                     impulse_magnitude - new_total
        //                 }
        //                 _ => {
        //                     acc.total_impulse = new_total;
        //                     impulse_magnitude
        //                 }
        //             };

        //             // apply the impulse
        //             if let Some(vel) = objs[0].body.velocity_mut() {
        //                 vel.linear -= acc.inv_masses[0] * clamped_impulse * acc.constraint.normal;
        //                 vel.angular -= acc.inv_mom_inertias[0]
        //                     * clamped_impulse
        //                     * acc.offsets_cross_normals[0];
        //             }
        //             if let Some(vel) = objs[1].body.velocity_mut() {
        //                 vel.linear += acc.inv_masses[1] * clamped_impulse * acc.constraint.normal;
        //                 vel.angular += acc.inv_mom_inertias[1]
        //                     * clamped_impulse
        //                     * acc.offsets_cross_normals[1];
        //             }
        //         }
        //     }

        //     // store impulses for next frame's warm start
        //     self.impulse_cache.replace(&accumulators);
        // }
    }
}
