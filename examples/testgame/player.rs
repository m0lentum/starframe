use crate::MainSpaceFeatures;
use moleengine::{
    core::{
        self,
        container::Container,
        inputcache::{Key, KeyAxisState},
        math as m, space, storage,
    },
    graphics as gx, physics as phys,
};

use nalgebra as na;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PlayerRecipe {
    pub transform: m::TransformBuilder,
}

impl core::Recipe<crate::MainSpaceFeatures> for PlayerRecipe {
    fn spawn_vars(&self, id: space::CreationId, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(id, self.transform.into());
    }

    fn spawn_consts(key: space::CreationId, feat: &mut MainSpaceFeatures) {
        const WIDTH: f32 = 0.2;
        const HEIGHT: f32 = 0.4;
        feat.shape.add(
            key,
            gx::Shape::Rect {
                w: WIDTH,
                h: HEIGHT,
                color: [0.2, 0.8, 0.6, 1.0],
            },
        );
        let collider = phys::Collider::new_rect(WIDTH, HEIGHT);
        feat.physics
            .add_body(key, phys::RigidBody::new_dynamic(&collider, 3.0), collider);
        feat.player.add(key);
    }
}

pub struct PlayerController {
    tags: Container<storage::NullStorage>,
    base_move_speed: f32,
    max_acceleration: f32,
}
impl PlayerController {
    pub fn new(init: space::FeatureSetInit) -> Self {
        PlayerController {
            tags: Container::new(init),
            base_move_speed: 4.0,
            max_acceleration: 8.0,
        }
    }

    pub fn add(&mut self, key: space::CreationId) {
        self.tags.insert(key, ())
    }

    pub fn tick(
        &mut self,
        space: space::SpaceWriteAccess<'_>,
        input: &core::InputCache,
        trs: &mut m::TransformFeature,
        phys_f: &mut phys::PhysicsFeature,
    ) {
        let target_hdir = match input.get_key_axis_state(Key::Right, Key::Left) {
            KeyAxisState::Zero => 0.0,
            KeyAxisState::Pos => 1.0,
            KeyAxisState::Neg => -1.0,
        };

        for (player_body, player_tr) in space
            .iter()
            .overlay(self.tags.iter())
            .overlay(phys_f.bodies.iter_mut())
            .and(trs.iter_mut())
        {
            // move

            let move_speed = self.base_move_speed;

            let target_hvel = target_hdir * move_speed;
            let player_vel = match player_body.velocity_mut() {
                Some(vel) => vel,
                None => continue,
            };
            let accel_needed = target_hvel - player_vel.linear.x;
            let accel = accel_needed.min(self.max_acceleration);
            player_vel.linear.x += accel;

            // hacked up rotation locking

            player_vel.angular = 0.0;
            player_tr.isometry.rotation = na::UnitComplex::new(0.0);

            // jump

            if input.is_key_pressed(Key::LShift, Some(0)) {
                // TODO: only on ground, double jump, custom curve
                player_vel.linear.y = 8.0;
            }
        }
    }
}
