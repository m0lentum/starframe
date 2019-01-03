pub use self::macro_deps::*;
use crate::space::Space;

pub mod macro_deps {
    pub use crate::IdType;
    pub use hibitset::BitSetLike;
    pub use moleengine_ecs_codegen::*;
}

pub trait System: Sized {
    type Runner: SystemRunner;
    fn operate(items: &mut [Self]);
}

pub trait SystemRunner {
    fn run(space: &Space);
}

// test
#[derive(Debug)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}
pub struct Velocity {
    pub x: f32,
    pub y: f32,
}

#[system_item]
pub struct Mover<'a> {
    position: &'a mut Position,
    velocity: &'a Velocity,
}

#[system_logic]
fn do_move(items: &mut [Mover<'_>]) {
    for item in items {
        item.position.x += item.velocity.x;
        item.position.y += item.velocity.y;
        println!("position is {:?}", item.position);
    }
}
