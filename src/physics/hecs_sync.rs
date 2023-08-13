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
    pub fn enable_all() -> Self {
        Self {
            hecs_to_physics: true,
            physics_to_hecs: true,
            autodelete: true,
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
    /// with synced components that haven't been registered manually.
    /// None by default.
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
        self.body_entity_map.insert_at(coll.0, (entity, opts));
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
        // except sync only colliders that don't also have a body
        self.collider_entity_map.retain(|coll_key, (entity, opts)| {
            if opts.autodelete && !hecs_world.contains(*entity) {
                physics.entity_set.remove_collider(ColliderKey(coll_key));
                return false;
            }
            if opts.hecs_to_physics {
                let Ok(pose) = hecs_world
                    .query_one_mut::<hecs::Without<&m::Pose, &BodyKey>>(*entity) else { return true };
                let Some(coll) = physics
                    .entity_set.get_collider_mut(ColliderKey(coll_key)) else { return true };
                coll.pose = *pose;
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
        // colliders without bodies do not move during physics
        // so we don't need to sync them here
    }
}
