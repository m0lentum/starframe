use crate::physics::ConstraintHandle;

use slotmap as sm;
use std::collections::HashMap;

/// Stores the impulses caused by each constraint.
/// These values are used as the solver's initial guesses next frame.
pub(crate) struct ImpulseCache {
    dynamic: HashMap<DynamicConstraintId, f32>,
    user_defined: sm::SecondaryMap<ConstraintHandle, f32>,
}
impl ImpulseCache {
    pub fn new() -> Self {
        ImpulseCache {
            dynamic: HashMap::new(),
            user_defined: sm::SecondaryMap::new(),
        }
    }

    pub fn get(&self, id: ConstraintId) -> Option<f32> {
        match id {
            ConstraintId::Dynamic(dyn_id) => self.dynamic.get(&dyn_id).map(|v| *v),
            ConstraintId::UserDefined(handle) => self.user_defined.get(handle).map(|v| *v),
        }
    }

    pub(crate) fn insert(&mut self, id: ConstraintId, val: f32) {
        match id {
            ConstraintId::Dynamic(dyn_id) => {
                self.dynamic.insert(dyn_id, val);
            }
            ConstraintId::UserDefined(handle) => {
                self.user_defined.insert(handle, val);
            }
        }
    }

    pub fn clear(&mut self) {
        self.dynamic.clear();
        self.user_defined.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConstraintId {
    Dynamic(DynamicConstraintId),
    UserDefined(ConstraintHandle),
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
