use crate::controls::KeyboardControls;
use moleengine::{ecs, physics2d as phys, util::Transform, visuals_glium as vis};

ecs::recipes! {
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

impl ecs::ObjectRecipe for Player {
    fn spawn(&self, obj: &mut ecs::MasterObjectHandle) {
        let width = 90.0;
        let height = 55.0;
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_dynamic(
            phys::Collider::new_rect(width, height),
            3.0,
        ));
        obj.add(vis::Shape::new_rect(
            &vis::Context::get().display,
            width,
            height,
            vis::ShapeStyle::Outline([0.2, 0.8, 0.6, 1.0]),
        ));
        obj.add(KeyboardControls);
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct StaticBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl ecs::ObjectRecipe for StaticBlock {
    fn spawn(&self, obj: &mut ecs::MasterObjectHandle) {
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_static(phys::Collider::new_rect(
            self.width,
            self.height,
        )));
        obj.add(vis::Shape::new_rect(
            &vis::Context::get().display,
            self.width,
            self.height,
            vis::ShapeStyle::Fill([0.5; 4]),
        ));
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct DynamicBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl ecs::ObjectRecipe for DynamicBlock {
    fn spawn(&self, obj: &mut ecs::MasterObjectHandle) {
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_dynamic(
            phys::Collider::new_rect(self.width, self.height),
            1.0,
        ));
        obj.add(vis::Shape::new_rect(
            &vis::Context::get().display,
            self.width,
            self.height,
            vis::ShapeStyle::Outline([1.0; 4]),
        ));
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct Ball {
    pub radius: f32,
    pub position: [f32; 2],
}

impl ecs::ObjectRecipe for Ball {
    fn spawn(&self, obj: &mut ecs::MasterObjectHandle) {
        obj.add(Transform::from_position(self.position));
        obj.add(phys::RigidBody::new_dynamic(
            phys::Collider::new_circle(self.radius),
            1.0,
        ));
        obj.add(vis::Shape::new_circle(
            &vis::Context::get().display,
            self.radius,
            24,
            vis::ShapeStyle::Outline([1.0; 4]),
        ));
    }
}
