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
    physics2d::{Collider, CollisionEvent, RigidBody},
    util::{inputcache::InputCache, Transform},
    visuals_glium::{
        debug::IntersectionIndicator,
        shaders::Shaders,
        shape::{Shape, ShapeStyle},
    },
};

//

pub struct Resources {
    pub display: glium::Display,
    pub events: glutin::EventsLoop,
    pub shaders: Shaders,
    pub input_cache: InputCache,
    pub recipes: RecipeBook,
    pub space: Space,
    pub intersection_vis: IntersectionIndicator,
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
        .with_title("MoleEngine test")
        .with_dimensions(glutin::dpi::LogicalSize::new(800.0, 600.0));
    let context = glutin::ContextBuilder::new();
    let display = glium::Display::new(window, context, &events).expect("Failed to create display");

    let shaders = Shaders::compile(&display).expect("Failed to compile shader");

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, LShift,
        ]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Shape, VecStorage<_>>()
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<Collider, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<KeyboardControls, VecStorage<_>>()
        .init_global_state::<Vec<CollisionEvent>>(Vec::new());

    let recipes = make_recipes(&display);

    let intersection_vis = IntersectionIndicator::new(&display, 50);

    Resources {
        display,
        events,
        shaders,
        input_cache,
        recipes,
        space,
        intersection_vis,
    }
}

fn make_recipes(display: &glium::Display) -> RecipeBook {
    let mut recipes = RecipeBook::new();

    let coll_circle = Collider::new_circle(30.0);
    let ball = ObjectRecipe::new()
        .add(Shape::from_collider(
            display,
            &coll_circle,
            ShapeStyle::Outline([1.0; 4]),
        ))
        .add(RigidBody::new_dynamic(coll_circle.clone(), 1.0))
        .add_named_variable("T", None::<Transform>)
        //.add_listener(TestCollisionListener)
        .add_listener(ChainEventListener);
    recipes.add("ball", ball.clone());

    let coll_rect = Collider::new_rect(90.0, 55.0);
    let player = ObjectRecipe::new()
        .add(Transform::identity())
        .add(Shape::from_collider(
            display,
            &coll_rect,
            ShapeStyle::Outline([0.2, 0.8, 0.6, 0.7]),
        ))
        .add(RigidBody::new_dynamic(coll_rect.clone(), 1.5))
        .add(KeyboardControls);
    recipes.add("player", player);

    let coll_rect = Collider::new_rect(80.0, 65.0);
    let obj_box = ObjectRecipe::new()
        .add_named_variable("T", Some(Transform::identity()))
        .add_variable(Some(Shape::from_collider(
            display,
            &coll_rect,
            ShapeStyle::Outline([1.0; 4]),
        )))
        .add(RigidBody::new_dynamic(coll_rect.clone(), 0.8));
    recipes.add("box", obj_box);

    recipes
}
