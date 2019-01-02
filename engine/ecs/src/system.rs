use crate::space::Space;
use crate::IdType;
use hibitset::{BitSetAnd, BitSetLike};

pub trait System {
    type Runner: SystemRunner;
    fn operate(item: Self);
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

//#[system_data]
pub struct PositionIntegrator<'a> {
    position: &'a mut Position,
    velocity: &'a Velocity,
}

impl<'a> System for PositionIntegrator<'a> {
    type Runner = PositionIntegratorRunner;
    //#[system_logic]
    fn operate(item: Self) {
        item.position.x += item.velocity.x;
        item.position.y += item.velocity.y;
        println!("position is {:?}", item.position);
    }
}

pub struct PositionIntegratorRunner;
impl SystemRunner for PositionIntegratorRunner {
    fn run(space: &Space) {
        let position = space.open::<Position>();
        let velocity = space.open::<Velocity>();
        let mut position_access = position.write();
        let velocity_access = velocity.read();

        let alive = space.get_alive();
        let position_users = position.get_users();
        let velocity_users = velocity.get_users();

        let and_set = BitSetAnd(BitSetAnd(alive, position_users), velocity_users);
        let iter = and_set.iter();

        for id in iter {
            let item = unsafe {
                PositionIntegrator {
                    position: position_access.get_mut(id as IdType),
                    velocity: velocity_access.get(id as IdType),
                }
            };
            PositionIntegrator::operate(item);
        }
    }
}
