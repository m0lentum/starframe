use crate::event::EventQueue;
use crate::space::Space;
pub use crate::IdType;
pub use moleengine_ecs_codegen::*;

/// A System can perform arbitrary operations on game objects with desired associated Components,
/// which are defined as a ComponentFilter.
/// They are executed within a Space with Space::(try_)run_system::<Type>().
pub trait System<'a> {
    type Filter: ComponentFilter<'a>;
    fn run_system(items: &mut [Self::Filter], space: &Space, queue: &mut EventQueue);
}

/// A simpler System interface useful to reduce boilerplate
/// when implementing Systems which only use one filter and don't produce events.
pub trait SimpleSystem<'a> {
    type Filter: ComponentFilter<'a>;
    fn run_system(items: &mut [Self::Filter]);
}

impl<'a, S: SimpleSystem<'a>> System<'a> for S {
    type Filter = S::Filter;
    fn run_system(items: &mut [Self::Filter], _s: &Space, _q: &mut EventQueue) {
        <Self as SimpleSystem>::run_system(items);
    }
}

/// A system that can store data within itself. This is only necessary in the unusual case that
/// the system needs some information to persist between updates.
/// These need to be initialized in a Space before using them.
pub trait StatefulSystem<'a> {
    type Filter: ComponentFilter<'a>;
    fn run_system(&mut self, items: &mut [Self::Filter], space: &Space, queue: &mut EventQueue);
}

/// A set of Components that knows how to extract itself from a Space.
/// These do not need to be implemented by hand - there is a derive macro available
/// in the moleengine_ecs_codegen crate.
/// # Example
/// ```
/// #[derive(ComponentFilter)]
/// pub struct PosVel<'a> {
///     #[id] id: IdType,
///     position: &'a mut Position,
///     velocity: &'a Velocity,
/// }
/// ```
pub trait ComponentFilter<'a>: Sized {
    fn run_filter(space: &Space, f: impl FnMut(&mut [Self])) -> Option<()>;
}

// test
#[derive(Debug, Clone)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
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
