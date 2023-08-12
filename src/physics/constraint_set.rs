use super::Constraint;

use thunderdome as td;

/// Key type to look up a constraint stored in the physics world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstraintKey(pub(super) td::Index);

/// Manager struct holding constraints inside of a physics world.
#[derive(Clone, Debug, Default)]
pub struct ConstraintSet {
    pub(super) constraints: td::Arena<Constraint>,
}

impl ConstraintSet {
    #[inline]
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Add a user-defined constraint to the physics world.
    /// Returns a key that can be used to remove it later.
    #[inline]
    pub fn insert(&mut self, constraint: Constraint) -> ConstraintKey {
        ConstraintKey(self.constraints.insert(constraint))
    }

    /// Access a Constraint in the physics world, if it still exists.
    #[inline]
    pub fn get(&self, key: ConstraintKey) -> Option<&Constraint> {
        self.constraints.get(key.0)
    }

    /// Mutably access a Constraint in the physics world, if it still exists.
    #[inline]
    pub fn get_mut(&mut self, key: ConstraintKey) -> Option<&mut Constraint> {
        self.constraints.get_mut(key.0)
    }

    /// Remove a constraint from the physics world, returning it if it still existed.
    ///
    /// Constraints can also disappear on their own if the objects they're associated with
    /// are destroyed, so it's not guaranteed the constraint will exist
    /// even if it hasn't been explicitly removed before.
    #[inline]
    pub fn remove(&mut self, key: ConstraintKey) -> Option<Constraint> {
        self.constraints.remove(key.0)
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        self.constraints.clear();
    }
}
