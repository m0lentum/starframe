use crate::event::EventQueue;
use crate::space::Space;
pub use crate::IdType;
pub use moleengine_ecs_codegen::*;

/// A System can perform arbitrary operations on game objects with desired associated Components,
/// which are defined as a ComponentFilter.
/// They are executed within a Space with Space::run_system::<Type>() or Space::run_optional_system::<Type>().
pub trait System<'a> {
    type Filter: ComponentFilter<'a>;
    fn operate(items: &mut [Self::Filter], space: &Space, queue: &mut EventQueue);
}

/// A simpler System interface useful to reduce boilerplate
/// when implementing Systems which only use one filter and don't produce events.
pub trait SimpleSystem<'a> {
    type Filter: ComponentFilter<'a>;
    fn operate(items: &mut [Self::Filter]);
}

impl<'a, S: SimpleSystem<'a>> System<'a> for S {
    type Filter = S::Filter;
    fn operate(items: &mut [Self::Filter], _s: &Space, _q: &mut EventQueue) {
        <Self as SimpleSystem>::operate(items);
    }
}

/// A set of Components that knows how to extract itself from a Space.
/// These do not need to be defined by hand - there is a derive macro available
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
    fn run(space: &Space, f: impl FnMut(&mut [Self])) -> Option<()>;
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

//#[derive(ComponentFilter)]
pub struct PosVel<'a> {
    /*#[id]*/ id: IdType,
    position: &'a mut Position,
    velocity: &'a Velocity,
}

impl<'a> ComponentFilter<'a> for PosVel<'a> {
    fn run(space: &Space, mut f: impl FnMut(&mut [Self])) -> Option<()> {
        let position = space.try_open_container::<Position>()?;
        let velocity = space.try_open_container::<Velocity>()?;
        let mut position_access = position.write();
        let velocity_access = velocity.read();

        let alive = space.get_alive();
        let position_users = position.get_users();
        let velocity_users = velocity.get_users();

        let and_set = hibitset::BitSetAll;
        let and_set = hibitset::BitSetAnd(position_users, and_set);
        let and_set = hibitset::BitSetAnd(velocity_users, and_set);
        let and_set = hibitset::BitSetAnd(alive, and_set);

        use hibitset::BitSetLike;
        let iter = and_set.iter();
        let mut items: Vec<_> = iter
            .map(|id| unsafe {
                PosVel {
                    position: position_access.get_mut_raw(id as IdType).as_mut().unwrap(),
                    velocity: velocity_access.get_raw(id as IdType).as_ref().unwrap(),
                    id: id as IdType,
                }
            })
            .collect();

        f(items.as_mut_slice());

        Some(())
    }
}

pub struct Mover;
impl<'a> SimpleSystem<'a> for Mover {
    type Filter = PosVel<'a>;
    fn operate(items: &mut [Self::Filter]) {
        for item in items {
            item.position.x += item.velocity.x;
            item.position.y += item.velocity.y;
        }
    }
}
