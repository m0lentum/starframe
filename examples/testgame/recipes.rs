use moleengine::{
    core::{self, space::MasterKey, Transform},
    graphics as gx, physics2d as phys,
};

use super::MainSpaceFeatures;

moleengine::core::recipes! {
    MainSpaceFeatures,
    Player,
    StaticBlock,
    DynamicBlock,
    Ball,
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Player {
    pub transform: Transform,
}

impl core::Recipe<MainSpaceFeatures> for Player {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform);
    }

    fn spawn_consts(key: MasterKey, feat: &mut MainSpaceFeatures) {
        const WIDTH: f32 = 0.9;
        const HEIGHT: f32 = 0.55;
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
        // obj.add(KeyboardControls);
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct StaticBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl core::Recipe<MainSpaceFeatures> for StaticBlock {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform);
        feat.physics.add_body(
            key,
            phys::RigidBody::new_static(),
            phys::Collider::new_rect(self.width, self.height),
        );
        feat.shape.add(
            key,
            gx::Shape::Rect {
                w: self.width,
                h: self.height,
                color: [0.5; 4],
            },
        );
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct DynamicBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl core::Recipe<MainSpaceFeatures> for DynamicBlock {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform);
        let collider = phys::Collider::new_rect(self.width, self.height);
        feat.physics
            .add_body(key, phys::RigidBody::new_dynamic(&collider, 1.0), collider);
        feat.shape.add(
            key,
            gx::Shape::Rect {
                w: self.width,
                h: self.height,
                color: [1.0; 4],
            },
        );
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct Ball {
    pub radius: f32,
    pub position: [f32; 2],
}

impl core::Recipe<MainSpaceFeatures> for Ball {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr
            .insert(key, Transform::from_position(self.position.into()));
        let collider = phys::Collider::new_circle(self.radius);
        feat.physics
            .add_body(key, phys::RigidBody::new_dynamic(&collider, 1.0), collider);
        feat.shape.add(
            key,
            gx::Shape::Circle {
                r: self.radius,
                points: 24,
                color: [1.0; 4],
            },
        );
    }
}
