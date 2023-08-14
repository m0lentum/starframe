use crate::{
    math as m,
    physics::{BodyKey, ColliderKey, PhysicsWorld},
};

use thunderdome as td;

#[derive(Clone, Copy, Debug)]
pub struct HecsSyncOptions {
    pub hecs_to_physics: bool,
    pub physics_to_hecs: bool,
    pub autodelete: bool,
}

impl HecsSyncOptions {
    #[inline]
    pub fn both_ways() -> Self {
        Self {
            hecs_to_physics: true,
            physics_to_hecs: true,
            autodelete: true,
        }
    }

    #[inline]
    pub fn hecs_to_physics_only() -> Self {
        Self {
            hecs_to_physics: true,
            physics_to_hecs: false,
            autodelete: true,
        }
    }

    #[inline]
    pub fn physics_to_hecs_only() -> Self {
        Self {
            hecs_to_physics: false,
            physics_to_hecs: true,
            autodelete: false,
        }
    }

    #[inline]
    pub fn do_not_sync() -> Self {
        Self {
            hecs_to_physics: false,
            physics_to_hecs: false,
            autodelete: false,
        }
    }
}

/// Automatically syncs information between a [`hecs`][hecs] world
/// and a [`PhysicsWorld`][super::PhysicsWorld].
///
/// TODOC: which components are synced
#[derive(Default, Debug)]
pub struct HecsSyncManager {
    /// If set, automatically uses these options to register all hecs entities
    /// with [`BodyKey`][BodyKey] or [`ColliderKey`][ColliderKey] components
    /// that haven't been registered manually. None by default.
    pub default_opts: Option<HecsSyncOptions>,
    body_entity_map: td::Arena<(hecs::Entity, HecsSyncOptions)>,
    collider_entity_map: td::Arena<(hecs::Entity, HecsSyncOptions)>,
}

impl HecsSyncManager {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn new_autosync(opts: HecsSyncOptions) -> Self {
        Self {
            default_opts: Some(opts),
            ..Self::default()
        }
    }

    #[inline]
    pub fn register_body(&mut self, body: BodyKey, entity: hecs::Entity, opts: HecsSyncOptions) {
        self.body_entity_map.insert_at(body.0, (entity, opts));
    }

    #[inline]
    pub fn register_collider(
        &mut self,
        coll: ColliderKey,
        entity: hecs::Entity,
        opts: HecsSyncOptions,
    ) {
        self.collider_entity_map.insert_at(coll.0, (entity, opts));
    }

    /// Sync data from a hecs world to the physics world.
    /// Call before [`PhysicsWorld::tick`][PhysicsWorld::tick].
    pub fn sync_hecs_to_physics(
        &mut self,
        physics: &mut PhysicsWorld,
        hecs_world: &mut hecs::World,
    ) {
        // auto-register new entities
        if let Some(opts) = self.default_opts {
            for (entity, body_key) in hecs_world.query_mut::<&BodyKey>() {
                if !self.body_entity_map.contains(body_key.0) {
                    self.body_entity_map.insert_at(body_key.0, (entity, opts));
                }
            }
            for (entity, coll_key) in hecs_world.query_mut::<&ColliderKey>() {
                if !self.collider_entity_map.contains(coll_key.0) {
                    self.collider_entity_map
                        .insert_at(coll_key.0, (entity, opts));
                }
            }
        }
        self.body_entity_map.retain(|body_key, (entity, opts)| {
            // auto-delete bodies for entities that don't exist anymore,
            // using the surrounding `retain` to also delete them from this map
            if opts.autodelete && !hecs_world.contains(*entity) {
                physics.entity_set.remove_body(BodyKey(body_key));
                return false;
            }
            if opts.hecs_to_physics {
                // sync poses for bodies that do still exist
                let Ok(pose) = hecs_world.query_one_mut::<&m::Pose>(*entity) else { return true };
                let Some(body) = physics.entity_set.get_body_mut(BodyKey(body_key)) else { return true };
                body.pose = *pose;
            }
            true
        });
        // same as above for colliders,
        // except if colliders have a body, actually sync the body
        self.collider_entity_map.retain(|coll_key, (entity, opts)| {
            let coll_key = ColliderKey(coll_key);
            if opts.autodelete && !hecs_world.contains(*entity) {
                physics.entity_set.remove_collider(coll_key);
                return false;
            }
            if opts.hecs_to_physics {
                let Ok(pose) = hecs_world
                    .query_one_mut::<&m::Pose>(*entity) else { return true };

                if let Some(body) = physics.entity_set.get_collider_body_mut(coll_key) {
                    body.pose = *pose;
                } else {
                    let Some(coll) = physics
                        .entity_set.get_collider_mut(coll_key) else { return true };
                    coll.pose = *pose;
                }
            }
            true
        });
    }

    /// Sync data from a physics world to a hecs world.
    /// Call after [`PhysicsWorld::tick`][PhysicsWorld::tick].
    pub fn sync_physics_to_hecs(&mut self, physics: &PhysicsWorld, hecs_world: &mut hecs::World) {
        for (body_key, (entity, opts)) in self.body_entity_map.iter() {
            if !opts.physics_to_hecs {
                continue;
            }
            let Some(body) = physics.entity_set.get_body(BodyKey(body_key)) else { continue };
            let Ok(pose) = hecs_world.query_one_mut::<&mut m::Pose>(*entity) else { continue };
            *pose = body.pose;
        }
        for (coll_key, (entity, opts)) in self.collider_entity_map.iter() {
            if !opts.physics_to_hecs {
                continue;
            }
            let coll_key = ColliderKey(coll_key);

            let Ok(pose) = hecs_world
                .query_one_mut::<&mut m::Pose>(*entity) else { continue };
            // sync the global pose of the collider even if it's attached to a body
            let Some(coll) = physics
                .entity_set.get_collider(coll_key) else { continue };
            if let Some(body) = physics.entity_set.get_collider_body(coll_key) {
                *pose = body.pose * coll.pose;
            } else {
                *pose = coll.pose;
            }
        }
    }
}
