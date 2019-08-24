use crate::controls::KeyboardControls;
use moleengine::{ecs, physics2d as phys, util::Transform, visuals_glium as vis};

// TODO: add a macro to do this automatically
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum Recipes {
    Player(Player),
    StaticBlock(StaticBlock),
    DynamicBlock(DynamicBlock),
}

impl moleengine::ecs::space::DeserializeRecipes for Recipes {
    fn deserialize_into_space<'a, 'de, D>(
        deserializer: D,
        space: &'a mut ecs::Space,
    ) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RecipeVisitor<'a>(&'a mut ecs::Space);

        impl<'a, 'de> serde::de::Visitor<'de> for RecipeVisitor<'a> {
            type Value = ();

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("A list of ObjectRecipes")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<(), S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                while let Some(value) = seq.next_element()? {
                    match value {
                        Recipes::Player(r) => self.0.spawn(r),
                        Recipes::StaticBlock(r) => self.0.spawn(r),
                        Recipes::DynamicBlock(r) => self.0.spawn(r),
                    }
                }

                Ok(())
            }
        }

        deserializer.deserialize_seq(RecipeVisitor(space))
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
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

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub struct DynamicBlock {
    pub width: f32,
    pub height: f32,
    pub transform: Transform,
}

impl ecs::ObjectRecipe for DynamicBlock {
    fn spawn(&self, mut obj: ecs::ObjectHandle) {
        obj.add(self.transform);
        obj.add(phys::RigidBody::new_dynamic(
            phys::Collider::new_rect(self.width, self.height),
            1.0,
        ));
        obj.add(vis::Shape::new_rect(
            &vis::Context::get().display,
            self.width,
            self.height,
            vis::ShapeStyle::Fill([0.5; 4]),
        ));
    }
}
