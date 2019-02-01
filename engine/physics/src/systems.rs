use crate::rigidbody::{BodyType, RigidBody};
use moleengine::ecs::system::*;
use moleengine::Transform;
use nalgebra::Vector2;

/// System that applies velocity to position.
/// Prefer to run this *after* solving collisions for improved accuracy
/// (symplectic Euler integration as opposed to explicit Euler).
pub struct Motion;

#[derive(ComponentFilter)]
pub struct MotionFilter<'a> {
    tr: &'a mut Transform,
    body: &'a RigidBody,
}

impl<'a> SimpleSystem<'a> for Motion {
    type Filter = MotionFilter<'a>;

    fn run_system(self, items: &mut [Self::Filter]) {
        for item in items {
            match item.body.get_body_type() {
                BodyType::Dynamic | BodyType::Kinematic => item.tr.translate(dbg!(item.body.velocity)),
                BodyType::Static => (),
            }
        }
    }
}

/// System that applies a constant acceleration to every RigidBody.
pub struct Gravity {
    force: Vector2<f32>,
}

impl Gravity {
    pub fn down(strength: f32) -> Self {
        Gravity {
            force: Vector2::new(0.0, strength),
        }
    }

    pub fn from_force(force: Vector2<f32>) -> Self {
        Gravity { force }
    }
}

#[derive(ComponentFilter)]
pub struct RigidBodyFilter<'a> {
    body: &'a mut RigidBody,
}

impl<'a> SimpleSystem<'a> for Gravity {
    type Filter = RigidBodyFilter<'a>;

    fn run_system(self, items: &mut [Self::Filter]) {
        for item in items {
            item.body.velocity += self.force;
        }
    }
}
