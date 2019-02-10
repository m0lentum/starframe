use moleengine::ecs::space::Space;
use moleengine::ecs::system::*;
use moleengine::util::inputstate::*;
use moleengine::util::Transform;
use nalgebra::Vector2;
use piston::input::keyboard::Key;

#[derive(Copy, Clone)]
pub struct KeyboardControls;

#[derive(ComponentFilter)]
pub struct PosVel<'a> {
    _marker: &'a KeyboardControls,
    tr: &'a mut Transform,
}

pub struct KeyboardMover<'a> {
    input: &'a InputState,
}
impl<'a> KeyboardMover<'a> {
    pub fn new(input: &'a InputState) -> Self {
        KeyboardMover { input }
    }
}
impl<'a> SimpleSystem<'a> for KeyboardMover<'a> {
    type Filter = PosVel<'a>;
    fn run_system(self, items: &mut [Self::Filter]) {
        let mut t = Vector2::zeros();
        let mut r = 0.0;
        if check_key(self.input, Key::Left) {
            t[0] = -5.0;
        } else if check_key(self.input, Key::Right) {
            t[0] = 5.0;
        }
        if check_key(self.input, Key::Up) {
            t[1] = -5.0;
        } else if check_key(self.input, Key::Down) {
            t[1] = 5.0;
        }
        if check_key(self.input, Key::PageDown) {
            r = 0.03;
        } else if check_key(self.input, Key::PageUp) {
            r = -0.03;
        }

        for item in items {
            item.tr.translate(t);
            item.tr.rotate_rad(r);
        }
    }
}

fn check_key(states: &InputState, key: Key) -> bool {
    match states.get_key(key) {
        Some((KeyState::Pressed, _)) => true,
        _ => false,
    }
}
