use crate::controls::KeyboardControls;
use moleengine::{ecs, physics2d as phys, util::Transform, visuals_glium as vis};

pub fn test_ser() {
    let blok = StaticBlock {
        width: 100.0,
        height: 50.0,
        transform: Transform::from_coords(-50.0, -250.0),
    };

    println!(
        "{}",
        ron::ser::to_string_pretty(&blok, Default::default()).unwrap()
    );
}

pub fn test_deser() {
    let blok: StaticBlock = ron::de::from_str(
        "StaticBlock(
            width: 100.0,
            height: 50.0,
            transform: (
                position: (-50, -250),
                rotation: 45,
            ),
        )",
    )
    .unwrap();

    println!("{:?}", blok);
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct StaticBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl ecs::ObjectRecipe for StaticBlock {
    fn spawn(&self, mut obj: ecs::ObjectHandle) {
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

pub struct Player {
    pub transform: Transform,
}

impl ecs::ObjectRecipe for Player {
    fn spawn(&self, mut obj: ecs::ObjectHandle) {
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
