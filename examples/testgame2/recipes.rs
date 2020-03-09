use moleengine::{core, core::Transform, graphics as gx, physics2d as phys};

use super::MainSpaceFeatures;

moleengine::core::recipes_new! {
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
    fn spawn(&self, id: core::Id, feat: &mut MainSpaceFeatures) {
        let width = 0.9;
        let height = 0.55;
        feat.tr.insert(id, self.transform);
        // obj.add(phys::RigidBody::new_dynamic(
        //     phys::Collider::new_rect(width, height),
        //     3.0,
        // ));
        feat.shape.insert(
            id,
            gx::Shape::new_rect(
                &gx::Context::get().display,
                width,
                height,
                gx::ShapeStyle::Outline([0.2, 0.8, 0.6, 1.0]),
            ),
        );
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
    fn spawn(&self, id: core::Id, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(id, self.transform);
        // obj.add(phys::RigidBody::new_static(phys::Collider::new_rect(
        //     self.width,
        //     self.height,
        // )));
        feat.shape.insert(
            id,
            gx::Shape::new_rect(
                &gx::Context::get().display,
                self.width,
                self.height,
                gx::ShapeStyle::Fill([0.5; 4]),
            ),
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
    fn spawn(&self, id: core::Id, feat: &mut MainSpaceFeatures) {
        feat.tr.insert(id, self.transform);
        // obj.add(phys::RigidBody::new_dynamic(
        //     phys::Collider::new_rect(self.width, self.height),
        //     1.0,
        // ));
        feat.shape.insert(
            id,
            gx::Shape::new_rect(
                &gx::Context::get().display,
                self.width,
                self.height,
                gx::ShapeStyle::Outline([1.0; 4]),
            ),
        );
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct Ball {
    pub radius: f32,
    pub position: [f32; 2],
}

impl core::Recipe<MainSpaceFeatures> for Ball {
    fn spawn(&self, id: core::Id, feat: &mut MainSpaceFeatures) {
        feat.tr
            .insert(id, Transform::from_position(self.position.into()));
        // obj.add(phys::RigidBody::new_dynamic(
        //     phys::Collider::new_circle(self.radius),
        //     1.0,
        // ));
        feat.shape.insert(
            id,
            gx::Shape::new_circle(
                &gx::Context::get().display,
                self.radius,
                24,
                gx::ShapeStyle::Outline([1.0; 4]),
            ),
        );
    }
}
