#[macro_use]
mod tracy_helpers {
    macro_rules! tracy_span {
        ($name: literal, $func_name: literal) => {{
            #[cfg(feature = "tracy-client")]
            {
                tracy_client::Span::new($name, $func_name, file!(), line!(), 100)
            }
            #[cfg(not(feature = "tracy-client"))]
            {
                ()
            }
        }};
    }
}

pub mod graph;

pub mod event;
pub use event::Event;

pub mod game;
pub use game::Game;

pub mod input;
pub use input::InputCache;

pub mod math;

pub mod graphics;

pub mod physics;
pub use physics::Physics;
