use crate::{
    core,
    physics2d::{self as phys, collision as coll},
};

use std::marker::PhantomData;

pub struct ContactSolver<B: coll::BroadPhase> {
    stabilisation_coef: f32,
    _broad_phase_marker: PhantomData<B>,
}

impl<'a, B: coll::BroadPhase> ContactSolver<B> {
    pub fn new(stabilisation_coef: f32) -> Self {
        ContactSolver {
            stabilisation_coef,
            _broad_phase_marker: PhantomData,
        }
    }

    /// Check for collisions in a Space and output inequality constraints for use in the constraint solver.
    pub fn gather_contact_constraints(&mut self) {
        // space.run_query(|items: &mut [RigidBodyQuery]| {
        //     let contacts = B::run(items.iter().map(|rbq| rbq.as_collidable()));

        //     contact_constraints = contacts
        //         .iter()
        //         .map(|cont| phys::Constraint {
        //             ids: cont.ids,
        //             normal: cont.normal,
        //             offsets: cont.offsets,
        //             impulse_bounds: (Some(0.0), None),
        //             bias: cont.depth * self.stabilisation_coef,
        //         })
        //         .collect();

        //     if let Some(ref mut out) = self.contact_out {
        //         out.0 = contacts;
        //     }
        // });

        // if contact_constraints.len() > 0 {
        //     dbg!(&contact_constraints);
        // }
    }
}
