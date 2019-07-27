use glium::glutin::VirtualKeyCode as Key;
use moleengine::{ecs::system::*, physics2d::RigidBody, util::inputcache::*};
use nalgebra::Vector2;

#[derive(Copy, Clone)]
pub struct KeyboardControls;

#[derive(ComponentFilter)]
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
    type Filter = PosVel<'a>;
    fn run_system(&mut self, items: &mut [Self::Filter]) {
        if self.input.is_key_pressed(Key::LShift, None) {
            for item in items {
                item.body.velocity_mut().map(|vel| {
                    vel.linear = Vector2::zeros();
                    vel.angular = 0.0;
                });
            }
        } else {
            let mut t = Vector2::zeros();
            let mut r = 0.0;
            if self.input.is_key_pressed(Key::Left, Some(1)) {
                t[0] = -150.0;
            } else if self.input.is_key_pressed(Key::Right, Some(1)) {
                t[0] = 150.0;
            }
            if self.input.is_key_pressed(Key::Up, Some(1)) {
                t[1] = 150.0;
            } else if self.input.is_key_pressed(Key::Down, Some(1)) {
                t[1] = -150.0;
            }
            if self.input.is_key_pressed(Key::PageDown, Some(1)) {
                r = -3.0;
            } else if self.input.is_key_pressed(Key::PageUp, Some(1)) {
                r = 3.0;
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
