use super::{Body, Collider};

use thunderdome as td;

/// Key type to look up a collider stored in the physics world.
///
/// When using a [`hecs`][crate::hecs] World, this type should be stored
/// in the world instead of [`Collider`][super::Collider].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Internal representation of objects in the physics world,
/// comprised of bodies and colliders.
///
/// Represented as a graph where dynamic bodies can have multiple colliders
/// and colliders can be attached to dynamic bodies or be static.
#[derive(Default)]
pub struct EntitySet {
    // pub fields instead of immutable accessors because I'm lazy,
    // there are some invariants that can be violated with these when inserting/removing
    // but I only use them in the physics solver where I don't do those things
    pub(super) bodies: td::Arena<Body>,
    // keeping track of highest slot indices
    // because slots are used for addressing during physics solve
    pub(super) body_slot_count: usize,
    pub(super) colliders: td::Arena<Collider>,
    pub(super) coll_slot_count: usize,
    pub(super) coll_bodies: td::Arena<BodyKey>,
}

impl EntitySet {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Access a [`Body`][super::Body] in the physics world, if it still exists.
    #[inline]
    pub fn get_body(&self, body: BodyKey) -> Option<&Body> {
        self.bodies.get(body.0)
    }

    /// Mutably access a [`Body`][super::Body] in the physics world, if it still exists.
    #[inline]
    pub fn get_body_mut(&mut self, body: BodyKey) -> Option<&mut Body> {
        self.bodies.get_mut(body.0)
    }

    /// Access a [`Collider`][super::Collider] in the physics world, if it still exists.
    #[inline]
    pub fn get_collider(&self, coll: ColliderKey) -> Option<&Collider> {
        self.colliders.get(coll.0)
    }

    /// Mutably access a [`Collider`][super::Collider] in the physics world, if it still exists.
    #[inline]
    pub fn get_collider_mut(&mut self, coll: ColliderKey) -> Option<&mut Collider> {
        self.colliders.get_mut(coll.0)
    }

    /// Access the Body connected to the given Collider, if both still exist.
    #[inline]
    pub fn get_collider_body(&self, coll: ColliderKey) -> Option<&Body> {
        self.coll_bodies
            .get(coll.0)
            .and_then(|b| self.bodies.get(b.0))
    }

    /// Mutably access the Body connected to the given Collider, if both still exist.
    #[inline]
    pub fn get_collider_body_mut(&mut self, coll: ColliderKey) -> Option<&mut Body> {
        self.coll_bodies
            .get(coll.0)
            .and_then(|b| self.bodies.get_mut(b.0))
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

    /// Attach an already existing collider to a dynamic body.
    /// If it was attached to another body before, that connection is removed.
    #[inline]
    pub fn attach_existing_collider(&mut self, body: BodyKey, coll: ColliderKey) {
        self.coll_bodies.insert_at(coll.0, body);
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

    /// Remove a [`Body`][super::Body] from the physics world,
    /// returning it if it still existed.
    ///
    /// Colliders associated with this body will be automatically removed
    /// at the next physics tick.
    #[inline]
    pub fn remove_body(&mut self, body: BodyKey) -> Option<Body> {
        self.bodies.remove(body.0)
    }

    /// Remove a [`Collider`][super::Collider] from the physics world,
    /// returning it if it still existed.
    ///
    /// If a body is associated with this collider, it will not be automatically removed.
    /// In such cases, prefer to remove the body instead.
    #[inline]
    pub fn remove_collider(&mut self, coll: ColliderKey) -> Option<Collider> {
        self.coll_bodies.remove(coll.0);
        self.colliders.remove(coll.0)
    }

    /// Remove colliders that have had their corresponding bodies removed.
    pub(super) fn remove_orphan_colliders(&mut self) {
        self.colliders.retain(|k, _| {
            if let Some(&b) = self.coll_bodies.get(k) {
                self.bodies.contains(b.0)
            } else {
                true
            }
        });
    }

    // not exposed to users, must use through PhysicsWorld::clear
    pub(super) fn clear(&mut self) {
        self.bodies.clear();
        self.body_slot_count = 0;
        self.colliders.clear();
        self.coll_slot_count = 0;
        self.coll_bodies.clear();
    }
}
