use glium::glutin::VirtualKeyCode as Key;
use moleengine::{
    ecs::{space::Space, system::*},
    util::{inputcache::*, Transform},
};
use nalgebra::Vector2;

#[derive(Copy, Clone)]
pub struct KeyboardControls;

#[derive(ComponentFilter)]
pub struct PosVel<'a> {
    _marker: &'a KeyboardControls,
    tr: &'a mut Transform,
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
    fn run_system(self, items: &mut [Self::Filter]) {
        let mut t = Vector2::zeros();
        let mut r = 0.0;
        if self.input.is_key_pressed(Key::Left, None) {
            t[0] = -5.0;
        } else if self.input.is_key_pressed(Key::Right, None) {
            t[0] = 5.0;
        }
        if self.input.is_key_pressed(Key::Up, None) {
            t[1] = 5.0;
        } else if self.input.is_key_pressed(Key::Down, None) {
            t[1] = -5.0;
        }
        if self.input.is_key_pressed(Key::PageDown, None) {
            r = -0.03;
        } else if self.input.is_key_pressed(Key::PageUp, None) {
            r = 0.03;
        }

        for item in items {
            item.tr.translate(t);
            item.tr.rotate_rad(r);
        }
    }
}
