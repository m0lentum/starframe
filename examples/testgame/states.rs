use crate::{controls::*, recipes, Resources};

use glium::{glutin, Surface};
use glutin::VirtualKeyCode as Key;
use moleengine::{
    ecs,
    physics2d::{
        self as phys,
        collision::{broadphase, CollisionSolver},
        integrator,
    },
    util::{
        gameloop::{GameLoop, LockstepLoop},
        statemachine::{GameState, StateMachine, StateOp},
        InputCache, Transform,
    },
    visuals_glium as vis,
};

use nalgebra::Vector2;
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
    fn update(&mut self, res: &mut Resources, dt: f32) -> StateOp<Resources> {
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
            reload_space(&mut res.space);
        }

        if res.input_cache.is_key_pressed(Key::S, Some(0)) {
            if let Some(id) = res.space.spawn_from_pool("box") {
                res.space
                    .write_component(id, |tr: &mut Transform| {
                        let mut rng = rand::thread_rng();
                        tr.set_translation(nalgebra::Vector2::new(
                            distr::Uniform::from(-300.0..300.0).sample(&mut rng),
                            distr::Uniform::from(0.0..200.0).sample(&mut rng),
                        ));
                        tr.set_rotation_deg(distr::Uniform::from(0.0..360.0).sample(&mut rng));
                    })
                    .expect("No transform on the thing");
            }
        }

        update_space(res, dt);

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
    fn update(&mut self, res: &mut Resources, _dt: f32) -> StateOp<Resources> {
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

    let ctx = vis::Context::get();

    let mut target = ctx.display.draw();

    target.clear_color(BG_COLOR[0], BG_COLOR[1], BG_COLOR[2], BG_COLOR[3]);

    res.space.run_system(&mut vis::ShapeRenderer {
        target: &mut target,
        shaders: &ctx.shaders,
    });
    //res.intersection_vis
    //    .draw_space(&mut target, &res.space, [0.8, 0.1, 0.2, 1.0], &res.shaders);

    target.finish().unwrap();
}

fn update_space(res: &mut Resources, dt: f32) {
    microprofile::flip();
    microprofile::scope!("update", "all");
    res.space
        .run_system(&mut KeyboardMover::new(&res.input_cache));
    {
        microprofile::scope!("update", "rigid body solver");

        use broadphase::BruteForce;
        use integrator::SemiImplicitEuler;
        let fields = vec![
            phys::ForceField::gravity(Vector2::new(0.0, -250.0)),
            phys::ForceField::from_fn(|p| Vector2::new(-p[0] / 2.0, 0.0)),
        ];
        res.space
            .run_system(&mut CollisionSolver::<SemiImplicitEuler, BruteForce>::new(
                dt,
                4,
                Some(fields),
            ));
    }
}

pub fn reload_space(space: &mut ecs::Space) {
    space.destroy_all();

    let dir = "./examples/testgame/scenes";

    let file = std::fs::File::open(format!("{}/test.ron", dir)).expect("Failed to open file");
    space
        .read_ron_file::<recipes::Recipes>(file)
        .expect("Failed to load scene");

    //space.create_pool("box", 20, {
    //    let mut rec = recipes.get("box").unwrap().clone();
    //    rec.modify_variable(|sh: &mut Shape| sh.set_color([0.4, 0.8, 0.5, 1.0]));
    //    rec
    //});
}
