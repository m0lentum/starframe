mod states;

use glium::glutin;

use moleengine::{
    ecs::{
        recipe::{ObjectRecipe, RecipeBook},
        space::Space,
        storage::VecStorage,
    },
    physics2d::RigidBody,
    util::{inputcache::InputCache, Transform},
    visuals_glium::{
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
}

fn main() {
    let res = init_resources();
    states::begin(res);
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
        input_cache.track_keys(&[Escape, Space]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<Shape, VecStorage<_>>();

    let mut recipes = RecipeBook::new();
    let mut block = ObjectRecipe::new()
        .add_variable(Some(Transform::from_position([0.0, 0.0])))
        .add({
            let mut rb = RigidBody::new();
            rb.angular_vel = 0.03;
            rb
        })
        .add(Shape::new_square(
            &display,
            80.0,
            ShapeStyle::Fill([1.0; 4]),
        ));
    block.apply(&mut space);
    for pos in [[100.0, 0.0], [-100.0, 0.0], [0.0, 100.0], [0.0, -100.0]].iter() {
        block.set_variable(Transform::new(*pos, 0.0, 0.5));
        block.apply(&mut space);
    }

    block.reset_variables();
    recipes.add("block", block);

    Resources {
        display,
        events,
        shaders,
        input_cache,
        recipes,
        space,
    }
}
