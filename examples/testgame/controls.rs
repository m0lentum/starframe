use moleengine::{ecs::system::*, physics2d::RigidBody, util::inputcache::*};

use glium::glutin::VirtualKeyCode as Key;
use ultraviolet as uv;

#[derive(Copy, Clone)]
pub struct KeyboardControls;

impl moleengine::ecs::DefaultStorage for KeyboardControls {
    // TODO: change this to NullStorage once implemented
    type DefaultStorage = crate::ecs::storage::VecStorage<Self>;
}

#[derive(ComponentQuery)]
pub struct PosVel<'a> {
    _marker: &'a KeyboardControls,
    body: &'a mut RigidBody,
}

pub struct KeyboardMover<'a> {
    input: &'a InputCache,
}
impl<'a> KeyboardMover<'a> {
    pub fn new(input: &'a InputCache) -> Self {
        KeyboardMover { input }
    }
}
impl<'a> SimpleSystem<'a> for KeyboardMover<'a> {
    type Query = PosVel<'a>;
    fn run_system(self, items: &mut [Self::Query]) {
        if self.input.is_key_pressed(Key::LShift, None) {
            for item in items {
                item.body.velocity_mut().map(|vel| {
                    vel.linear = uv::Vec2::zero();
                    vel.angular = 0.0;
                });
            }
        } else {
            let mut t = uv::Vec2::zero();
            let mut r = 0.0;
            if self.input.is_key_pressed(Key::Left, Some(0)) {
                t.x = -3.0
            } else if self.input.is_key_pressed(Key::Right, Some(0)) {
                t.x = 3.0
            }
            if self.input.is_key_pressed(Key::Up, Some(0)) {
                t.y = 3.0
            } else if self.input.is_key_pressed(Key::Down, Some(0)) {
                t.y = -3.0
            }
            if self.input.is_key_pressed(Key::PageDown, Some(0)) {
                r = -6.0;
            } else if self.input.is_key_pressed(Key::PageUp, Some(0)) {
                r = 6.0;
            }

            for item in items {
                item.body.velocity_mut().map(|vel| {
                    vel.linear += t;
                    vel.angular += r;
                });
            }
        }
    }
}
