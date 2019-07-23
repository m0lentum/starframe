use crate::{controls::*, Resources};

use glium::{glutin, Surface};
use glutin::VirtualKeyCode as Key;
use moleengine::{
    ecs::{
        recipe::{parse_into_space, RecipeBook},
        space::{ObjectBuilder, Space},
    },
    physics2d::{
        collision::{broadphase, Collider, CollisionSolver},
        integrator, RigidBody,
    },
    util::{
        gameloop::{GameLoop, LockstepLoop},
        inputcache::*,
        statemachine::{GameState, StateMachine, StateOp},
        Transform,
    },
    visuals_glium::shape::{Shape, ShapeRenderer, ShapeStyle},
};

use rand::{distributions as distr, distributions::Distribution};

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const _CYAN_COLOR: [f32; 4] = [0.3, 0.7, 0.8, 1.0];
const _LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

// ================ Begin ====================

pub fn begin(res: Resources) {
    let mut sm = StateMachine::new(res, Box::new(StatePlaying));
    let l = LockstepLoop::from_fps(60);
    l.begin(&mut sm);
}

// ================ Playing ==================

pub struct StatePlaying;

impl GameState<Resources> for StatePlaying {
    fn update(&mut self, res: &mut Resources) -> StateOp<Resources> {
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache) {
            return op;
        }
        if res.input_cache.is_key_pressed(Key::Escape, None) {
            return StateOp::Destroy;
        }
        if res.input_cache.is_key_pressed(Key::Space, Some(0)) {
            return StateOp::Push(Box::new(StatePaused));
        }

        if res.input_cache.is_key_pressed(Key::Return, Some(0)) {
            reload_space(&mut res.space, &mut res.recipes, &res.display);
        }

        if res.input_cache.is_key_pressed(Key::S, Some(0)) {
            if let Some(id) = res.space.spawn_from_pool("box") {
                res.space
                    .write_component(id, |tr: &mut Transform| {
                        let mut rng = rand::thread_rng();
                        tr.set_translation(nalgebra::Vector2::new(
                            distr::Uniform::from(-300.0..300.0).sample(&mut rng),
                            distr::Uniform::from(-200.0..200.0).sample(&mut rng),
                        ));
                        tr.set_rotation_deg(distr::Uniform::from(0.0..360.0).sample(&mut rng));
                    })
                    .expect("No transform on the thing");
            }
        }

        update_space(res);

        res.input_cache.update_ages();
        StateOp::Stay
    }

    fn render(&mut self, res: &mut Resources) {
        draw_space(res);
    }
}

// ===================== Paused ========================

pub struct StatePaused;

impl GameState<Resources> for StatePaused {
    fn update(&mut self, res: &mut Resources) -> StateOp<Resources> {
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache) {
            return op;
        }
        if res.input_cache.is_key_pressed(Key::Escape, None) {
            return StateOp::Destroy;
        }
        if res.input_cache.is_key_pressed(Key::Space, Some(0)) {
            return StateOp::Pop;
        }

        res.input_cache.update_ages();
        StateOp::Stay
    }

    fn render(&mut self, res: &mut Resources) {
        draw_space(res);
    }
}

// ==================== Helper functions ======================

fn handle_events(
    events: &mut glutin::EventsLoop,
    input_cache: &mut InputCache,
) -> Option<StateOp<Resources>> {
    let mut should_close = false;
    events.poll_events(|evt| match evt {
        glutin::Event::WindowEvent { event, .. } => match event {
            glutin::WindowEvent::CloseRequested => should_close = true,
            glutin::WindowEvent::KeyboardInput { input, .. } => input_cache.handle_keyboard(input),
            _ => (),
        },
        _ => (),
    });

    if should_close {
        Some(StateOp::Destroy)
    } else {
        None
    }
}

fn draw_space(res: &mut Resources) {
    microprofile::scope!("render", "all");

    let mut target = res.display.draw();

    target.clear_color(BG_COLOR[0], BG_COLOR[1], BG_COLOR[2], BG_COLOR[3]);

    res.space.run_system(&mut ShapeRenderer {
        target: &mut target,
        shaders: &res.shaders,
    });
    res.intersection_vis
        .draw_space(&mut target, &res.space, [0.8, 0.1, 0.2, 1.0], &res.shaders);

    target.finish().unwrap();
}

fn update_space(res: &mut Resources) {
    microprofile::flip();
    microprofile::scope!("update", "all");
    res.space
        .run_system(&mut KeyboardMover::new(&res.input_cache));
    {
        microprofile::scope!("update", "rigid body solver");
        res.space.run_system(
            // TODO: real timestep
            &mut CollisionSolver::<integrator::SemiImplicitEuler, broadphase::BruteForce>::with_timestep(0.017, 4),
        );
    }
}

pub fn reload_space(space: &mut Space, recipes: &mut RecipeBook, display: &glium::Display) {
    let mes =
        std::fs::read_to_string("./examples/testgame/test_space.mes").expect("File read failed");

    space.destroy_all();

    let r = parse_into_space(mes.as_str(), space, recipes);

    space.create_pool("box", 10, {
        let mut rec = recipes.get("box").unwrap().clone();
        rec.modify_variable(|sh: &mut Shape| sh.set_color([0.4, 0.8, 0.5, 1.0]));
        rec
    });

    make_walls(space, display);

    match r {
        Ok(_) => (),
        Err(e) => eprintln!("Error parsing space: {}", e),
    }
}

// TODO: this is inefficient as hell, probably make it work differently
fn make_walls(space: &mut Space, display: &glium::Display) {
    ObjectBuilder::create(space)
        .with(Collider::new_rect(20.0, 600.0))
        .with(Shape::new_rect(
            display,
            20.0,
            600.0,
            ShapeStyle::Fill([0.5; 4]),
        ))
        .with(Transform::from_position([-400.0, 0.0]))
        .with(RigidBody::default().make_static());
    ObjectBuilder::create(space)
        .with(Collider::new_rect(20.0, 600.0))
        .with(Shape::new_rect(
            display,
            20.0,
            600.0,
            ShapeStyle::Fill([0.5; 4]),
        ))
        .with(Transform::from_position([400.0, 0.0]))
        .with(RigidBody::default().make_static());
    ObjectBuilder::create(space)
        .with(Collider::new_rect(800.0, 20.0))
        .with(Shape::new_rect(
            display,
            800.0,
            20.0,
            ShapeStyle::Fill([0.5; 4]),
        ))
        .with(Transform::from_position([0.0, -300.0]))
        .with(RigidBody::default().make_static());
    ObjectBuilder::create(space)
        .with(Collider::new_rect(800.0, 20.0))
        .with(Shape::new_rect(
            display,
            800.0,
            20.0,
            ShapeStyle::Fill([0.5; 4]),
        ))
        .with(Transform::from_position([0.0, 300.0]))
        .with(RigidBody::default().make_static());
}
