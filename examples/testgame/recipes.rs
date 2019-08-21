use crate::controls::KeyboardControls;
use moleengine::{ecs, physics2d as phys, util::Transform, visuals_glium as vis};

#[derive(Clone, Copy)]
pub struct StaticBlock<'a> {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
    pub display: &'a glium::Display,
}

impl<'a> ecs::ObjectRecipe for StaticBlock<'a> {
    fn spawn(&self, mut obj: ecs::ObjectHandle) {
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_static(phys::Collider::new_rect(
            self.width,
            self.height,
        )));
        obj.add(vis::Shape::new_rect(
            self.display,
            self.width,
            self.height,
            vis::ShapeStyle::Fill([0.5; 4]),
        ));
    }
}

pub struct Player<'a> {
    pub transform: Transform,
    pub display: &'a glium::Display,
}

impl<'a> ecs::ObjectRecipe for Player<'a> {
    fn spawn(&self, mut obj: ecs::ObjectHandle) {
        let width = 90.0;
        let height = 55.0;
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_dynamic(
            phys::Collider::new_rect(width, height),
            3.0,
        ));
        obj.add(vis::Shape::new_rect(
            self.display,
            width,
            height,
            vis::ShapeStyle::Outline([0.2, 0.8, 0.6, 1.0]),
        ));
        obj.add(KeyboardControls);
    }
}
