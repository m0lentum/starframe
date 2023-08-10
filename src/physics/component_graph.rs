use super::{Body, Collider};

use thunderdome as td;

/// Key type to look up a collider stored in the physics world.
///
/// When using a [`hecs`][crate::hecs] World, this type should be stored
/// in the world instead of [`Collider`][super::Collider].
#[derive(Clone, Copy, Debug)]
pub struct ColliderKey(pub(super) td::Index);

impl ColliderKey {
    /// Get the underlying [`thunderdome::Index`][thunderdome::Index] of this key.
    /// Useful for creating your own mappings from colliders to other things
    /// such as [`hecs`][hecs] entities.
    #[inline]
    pub fn index(&self) -> td::Index {
        self.0
    }
}

/// Key type to look up a body stored in the physics world.
///
/// When using a [`hecs`][crate::hecs] World, this type should be stored
/// in the world instead of [`Body`][super::Body].
#[derive(Clone, Copy, Debug)]
pub struct BodyKey(pub(super) td::Index);

impl BodyKey {
    /// Get the underlying [`thunderdome::Index`][thunderdome::Index] of this key.
    /// Useful for creating your own mappings from bodies to other things
    /// such as [`hecs`][hecs] entities.
    #[inline]
    pub fn index(&self) -> td::Index {
        self.0
    }
}

/// Internal representation of objects in the physics world.
/// Represented as a graph where dynamic bodies can have multiple colliders
/// and colliders can be attached to dynamic bodies or be static.
#[derive(Default)]
pub(super) struct ComponentGraph {
    // pub fields instead of immutable accessors because I'm lazy,
    // there are some invariants that can be violated with these when inserting/removing
    // but I only use them in the physics solver where I don't do those things
    pub bodies: td::Arena<Body>,
    // keeping track of highest slot indices
    // because slots are used for addressing during physics solve
    pub body_slot_count: usize,
    pub colliders: td::Arena<Collider>,
    pub coll_slot_count: usize,
    pub coll_bodies: td::Arena<BodyKey>,
}

impl ComponentGraph {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a dynamic body into the world.
    pub fn insert_body(&mut self, body: Body) -> BodyKey {
        let key = self.bodies.insert(body);
        let slot = key.slot() as usize;
        if slot >= self.body_slot_count {
            self.body_slot_count = slot + 1;
        }
        BodyKey(key)
    }

    /// Attach a collider to a dynamic body.
    pub fn attach_collider(&mut self, body: BodyKey, coll: Collider) -> ColliderKey {
        // TODO: moment of inertia (and maybe mass from density)
        // could totally be computed here instead of in the Body constructors.
        // think about it
        let coll_key = self.insert_collider(coll);
        self.coll_bodies.insert_at(coll_key.0, body);
        coll_key
    }

    /// Insert a collider that isn't attached to a dynamic body
    /// (typically a static collider or a sensor).
    pub fn insert_collider(&mut self, coll: Collider) -> ColliderKey {
        let key = self.colliders.insert(coll);
        let slot = key.slot() as usize;
        if slot >= self.coll_slot_count {
            self.coll_slot_count = slot + 1;
        }
        ColliderKey(key)
    }

    pub fn clear(&mut self) {
        self.bodies.clear();
        self.body_slot_count = 0;
        self.colliders.clear();
        self.coll_slot_count = 0;
        self.coll_bodies.clear();
    }

    // TODO:
    // - remove bodies
    // - cleanup step at the start of tick to remove colliders that were attached to that body
}
