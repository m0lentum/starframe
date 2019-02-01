use crate::{collision::intersection_check, Collider, RigidBody};
use moleengine::ecs::{system::*, IdType};
use moleengine::Transform;

pub struct RigidBodySolver;

#[derive(ComponentFilter)]
pub struct ColliderFilter<'a> {
    #[id]
    id: IdType,
    tr: &'a Transform,
    rb: &'a mut RigidBody,
    coll: &'a Collider,
}

impl<'a> StatefulSystem<'a> for RigidBodySolver {
    type Filter = ColliderFilter<'a>;

    fn run_system(&mut self, items: &mut [Self::Filter], _space: &Space, queue: &mut EventQueue) {
        // ugly brute force for now
        let mut iter = items.iter();
        while let Some(o1) = iter.next() {
            for o2 in iter.clone() {
                if let Some(colls) =
                    intersection_check(o1.id, o1.tr, o1.coll, o2.id, o2.tr, o2.coll)
                {
                    // testing
                    dbg!(colls);
                    panic!();
                    // TODO: push events to queue
                }
            }
        }
    }
}
