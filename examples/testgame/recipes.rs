use moleengine::{
    core::{self, math as m, space::MasterKey},
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

pub type Player = crate::player::PlayerRecipe;

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct StaticBlock {
    pub width: f32,
    pub height: f32,
    pub transform: m::TransformBuilder,
}

impl core::Recipe<MainSpaceFeatures> for StaticBlock {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform.into());
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
    pub transform: m::TransformBuilder,
}

impl core::Recipe<MainSpaceFeatures> for DynamicBlock {
    fn spawn_vars(&self, key: MasterKey, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(key, self.transform.into());
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
            .insert(key, m::TransformBuilder::from(self.position).into());
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
