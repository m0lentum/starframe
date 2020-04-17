use moleengine::{
    core::{self, container::Container, inputcache::Key, math as m, space, storage},
    graphics as gx, physics2d as phys,
};

use crate::MainSpaceFeatures;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PlayerRecipe {
    pub transform: m::TransformBuilder,
}

impl core::Recipe<crate::MainSpaceFeatures> for PlayerRecipe {
    fn spawn_vars(&self, key: space::MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform.into());
    }

    fn spawn_consts(key: space::MasterKey, feat: &mut MainSpaceFeatures) {
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
}
impl PlayerController {
    pub fn new(init: space::FeatureSetInit) -> Self {
        PlayerController {
            tags: Container::new(init),
        }
    }

    pub fn add(&mut self, key: space::MasterKey) {
        self.tags.insert(key, ())
    }

    pub fn tick(
        &mut self,
        space: space::SpaceWriteAccess<'_>,
        input: &core::InputCache,
        trs: &m::TransformFeature,
        phys_f: &phys::PhysicsFeature,
    ) {
        if input.is_key_pressed(Key::Right, Some(0)) {
            println!("Yo sorry we're not really done here yet");
        }
        let iter = space.iter().overlay(self.tags.iter()).overlay(trs.iter());
        // TODO next time: get the body iterator from `phys_f`.
        // See if a helper type to make returning iterator fragments less verbose is feasible
    }
}
