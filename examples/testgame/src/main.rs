#[macro_use]
extern crate microprofile;

mod controls;
mod states;
mod test_events;

//

use self::{controls::KeyboardControls, test_events::*};
use glium::glutin;
use moleengine::{
    ecs::{
        recipe::{ObjectRecipe, RecipeBook},
        space::Space,
        storage::VecStorage,
    },
    physics2d::{Collider, Collision, RigidBody},
    util::{inputcache::InputCache, Transform},
    visuals_glium::{shaders::Shaders, shape::Shape},
};

//

pub struct Resources {
    pub display: glium::Display,
    pub events: glutin::EventsLoop,
    pub shaders: Shaders,
    pub input_cache: InputCache,
    pub recipes: RecipeBook,
    pub space: Space,
}

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let res = init_resources();
    states::begin(res);

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}

pub fn init_resources() -> Resources {
    let events = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new()
        .with_title("MoleEngine project template")
        .with_dimensions(glutin::dpi::LogicalSize::new(800.0, 600.0));
    let context = glutin::ContextBuilder::new();
    let display = glium::Display::new(window, context, &events).expect("Failed to create display");

    let shaders = Shaders::compile(&display).expect("Failed to compile shader");

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[Left, Right, Down, Up, PageDown, PageUp]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Shape, VecStorage<_>>()
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<Collider, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<KeyboardControls, VecStorage<_>>()
        .init_global_state::<Vec<Collision>>(Vec::new());

    let mut recipes = RecipeBook::new();

    let coll = Collider::new_rect(180.0, 100.0);
    let thingy = ObjectRecipe::new()
        // TODO
        //.add(Shape::new_outlined(
        //    coll.as_points(),
        //    [0.0; 4],
        //    1.0,
        //    LINE_COLOR,
        //))
        .add(coll)
        .add(RigidBody::new())
        .add_named_variable("T", None::<Transform>)
        //.add_listener(TestCollisionListener)
        .add_listener(ChainEventListener);
    recipes.add("thingy", thingy.clone());

    let other_thingy = ObjectRecipe::new()
        .add_listener(ChainEventListener)
        .add(Transform::new([120.0, 180.0], 0.0, 1.0))
        //.add(Shape::new_outlined(
        //    coll.as_points(),
        //    [0.0; 4],
        //    1.0,
        //    LINE_COLOR,
        //))
        .add(coll)
        .add(RigidBody::new())
        .add(KeyboardControls)
        .add_listener(LifecycleListener);
    recipes.add("other", other_thingy);

    Resources {
        display,
        events,
        shaders,
        input_cache,
        recipes,
        space,
    }
}
