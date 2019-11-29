use crate::{
    ecs::{self, system::*},
    physics2d::{self as phys, collision as coll},
    util,
};

use std::marker::PhantomData;

pub struct ContactSolver<'a, B: coll::BroadPhase> {
    contact_out: Option<&'a mut ContactOutput>,
    _broad_phase_marker: PhantomData<B>,
}

impl<'a, B: coll::BroadPhase> ContactSolver<'a, B> {
    pub fn new() -> Self {
        ContactSolver {
            contact_out: None,
            _broad_phase_marker: PhantomData,
        }
    }

    /// Give the solver a ContactOutput to write contacts to.
    pub fn output_raw_contacts(mut self, contact_out: &'a mut ContactOutput) -> Self {
        contact_out.0.clear();
        self.contact_out = Some(contact_out);
        self
    }

    /// Check for collisions in a Space and output inequality constraints for use in the constraint solver.
    pub fn gather_contact_constraints(mut self, space: &mut ecs::Space) -> Vec<phys::Constraint> {
        let mut contact_constraints = Vec::new();

        space.run_query(|items: &mut [RigidBodyQuery]| {
            let contacts = B::run(items.iter().map(|rbq| rbq.as_collidable()));

            // TODO allow queries to return stuff
            contact_constraints = contacts
                .iter()
                .map(|cont| phys::Constraint {
                    ids: cont.ids,
                    normal: cont.normal,
                    offsets_objspace: cont.offsets_objspace,
                    impulse_bounds: (Some(0.0), None),
                    bias: 0.0, // TODO implement bias on the solver
                })
                .collect();

            if let Some(ref mut out) = self.contact_out {
                out.0 = contacts;
            }
        });

        contact_constraints
    }
}

/// A CollisionSolver can optionally output detected contacts for e.g. debug visualization.
pub struct ContactOutput(pub Vec<coll::Contact>);

impl ContactOutput {
    pub fn new() -> Self {
        ContactOutput(Vec::new())
    }
}

#[derive(ComponentQuery)]
pub struct RigidBodyQuery<'a> {
    #[id]
    id: ecs::IdType,
    tr: &'a mut util::Transform,
    body: &'a mut phys::RigidBody,
}

impl<'a> RigidBodyQuery<'a> {
    pub(self) fn as_collidable(&'a self) -> coll::broadphase::Collidable<'a> {
        coll::broadphase::Collidable {
            id: self.id,
            tr: self.tr,
            coll: &self.body.collider,
            responds_to_collisions: self.body.responds_to_collisions(),
        }
    }
}
