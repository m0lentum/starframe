mod states;

use glium::glutin;

use moleengine::{
    ecs::{
        recipe::{ObjectRecipe, RecipeBook},
        space::Space,
        storage::VecStorage,
    },
    physics2d::RigidBody,
    util::{inputstate::*, Transform},
    visuals::shape::*,
};

//

pub struct Resources {
    pub display: glium::Display,
    pub events: glutin::EventsLoop,
    pub input_state: InputState,
    pub recipes: RecipeBook,
    pub space: Space,
}

fn main() {
    let res = init_resources();
    states::begin(res);
}

pub fn init_resources() -> Resources {
    let events = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new();
    let context = glutin::ContextBuilder::new();
    let display = glium::Display::new(window, context, &events).expect("Failed to create display");

    let mut input_state = InputState::new();
    {
        use glutin::VirtualKeyCode::*;
        input_state.track_keys(&[Escape, Space]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<Shape, VecStorage<_>>();

    let mut recipes = RecipeBook::new();
    let mut block = ObjectRecipe::new()
        .add_variable(Some(Transform::from_position([150.0, 150.0])))
        .add({
            let mut rb = RigidBody::new();
            rb.angular_vel = 0.03;
            rb
        })
        .add(Shape::new_square(80.0, [1.0; 4]));
    block.apply(&mut space);
    block.set_variable(Transform::from_position([300.0, 300.0]));
    block.apply(&mut space);
    block.reset_variables();
    recipes.add("block", block);

    Resources {
        display,
        events,
        input_state,
        recipes,
        space,
    }
}
