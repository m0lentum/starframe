use super::{Body, Collider};

use thunderdome as td;

/// Key type to look up a collider stored in the physics world.
///
/// When using a [`hecs`][crate::hecs] World, this type should be stored
/// in the world instead of [`Collider`][super::Collider].
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
    pub bodies: td::Arena<Body>,
    pub colliders: td::Arena<Collider>,
    pub coll_bodies: td::Arena<BodyKey>,
}

impl ComponentGraph {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a dynamic body into the world.
    #[inline]
    pub fn insert_body(&mut self, body: Body) -> BodyKey {
        BodyKey(self.bodies.insert(body))
    }

    /// Attach a collider to a dynamic body.
    #[inline]
    pub fn attach_collider(&mut self, body: BodyKey, coll: Collider) -> ColliderKey {
        // TODO: moment of inertia (and maybe mass from density)
        // could totally be computed here instead of in the Body constructors.
        // think about it
        let coll_key = self.colliders.insert(coll);
        self.coll_bodies.insert_at(coll_key, body);
        ColliderKey(coll_key)
    }

    /// Insert a collider that isn't attached to a dynamic body
    /// (typically a static collider or a sensor).
    #[inline]
    pub fn insert_collider(&mut self, coll: Collider) -> ColliderKey {
        ColliderKey(self.colliders.insert(coll))
    }
}
