use moleengine_ecs::space::Space;
use moleengine_ecs::system::*;

// quick and dirty test thingy

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

#[derive(ComponentFilter)]
pub struct PosVel<'a> {
    #[enabled]
    is_enabled: bool,
    position: &'a mut Position,
    velocity: &'a Velocity,
}

pub struct Mover {
    counter: u32,
}
impl Mover {
    pub fn new() -> Self {
        Mover { counter: 0 }
    }
}
impl<'a> StatefulSystem<'a> for Mover {
    type Filter = PosVel<'a>;
    fn run_system(&mut self, items: &mut [Self::Filter], _space: &Space, _queue: &mut EventQueue) {
        for item in items {
            if !item.is_enabled {
                println!("Found disabled object!");
                continue;
            }
            item.position.x += item.velocity.x;
            item.position.y += item.velocity.y;
            println!("Position is {}, {}", item.position.x, item.position.y);
        }

        self.counter += 1;
        println!("Counter is {}", self.counter);
    }
}
