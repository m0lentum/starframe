use moleengine::ecs::space::Space;
use moleengine::ecs::system::*;
use moleengine::inputstate::KeyState;
use moleengine::InputState;
use moleengine::Transform;
use nalgebra::geometry::Translation2;
use piston::input::keyboard::Key;

// useless crap that needs to be deleted

#[derive(Debug, Clone)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}
impl std::str::FromStr for Position {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let mut halves = s.split(',');
        let x = halves.next().unwrap().parse().unwrap();
        let y = halves.next().unwrap().parse().unwrap();
        Ok(Position { x, y })
    }
}
#[derive(Clone, Copy)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}
impl std::str::FromStr for Velocity {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let pos = s.parse::<Position>()?; // lol
        Ok(Velocity { x: pos.x, y: pos.y })
    }
}

// end useless crap that needs to be deleted

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
        let mut t = Translation2::identity();
        if check_key(self.input, Key::Left) {
            t.vector[0] = -10.0;
        } else if check_key(self.input, Key::Right) {
            t.vector[0] = 10.0;
        }
        if check_key(self.input, Key::Up) {
            t.vector[1] = -10.0;
        } else if check_key(self.input, Key::Down) {
            t.vector[1] = 10.0;
        }

        for item in items {
            item.tr.0.isometry.append_translation_mut(&t);
        }
    }
}

fn check_key(states: &InputState, key: Key) -> bool {
    match states.get_key(key) {
        Some((KeyState::Pressed, _)) => true,
        _ => false,
    }
}
