use crate::{controls::*, recipes, Resources};

use glium::{glutin, Surface};
use glutin::VirtualKeyCode as Key;
use moleengine::{
    ecs,
    physics2d::{
        self as phys,
        collision::{broadphase, CollisionSolver, SolverLoopCondition},
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

        // pool spawning

        fn spawn_random_pos<T>(space: &mut ecs::Space)
        where
            T: ecs::ObjectRecipe + Clone + 'static,
        {
            if let Some(obj) = space.spawn_from_pool::<T>() {
                let mut rng = rand::thread_rng();
                obj.write_component(|tr: &mut Transform| {
                    tr.set_position([
                        distr::Uniform::from(-3.0..3.0).sample(&mut rng),
                        distr::Uniform::from(0.0..2.0).sample(&mut rng),
                    ]);
                    tr.set_rotation(distr::Uniform::from(0.0..360.0).sample(&mut rng));
                });
            }
        }
        if res.input_cache.is_key_pressed(Key::S, Some(0)) {
            spawn_random_pos::<recipes::DynamicBlock>(&mut res.space);
        }
        if res.input_cache.is_key_pressed(Key::T, Some(0)) {
            spawn_random_pos::<recipes::Ball>(&mut res.space);
        }

        //

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

    res.space.run_system(vis::ShapeRenderer {
        camera: &res.camera,
        target: &mut target,
        shaders: &ctx.shaders,
    });

    // res.debug_vis.contact_indicator.draw(
    //     &res.camera,
    //     &mut target,
    //     &res.debug_vis.contact_cache,
    //     [1.0, 0.0, 0.0, 1.0],
    //     &ctx.shaders,
    // );

    target.finish().unwrap();
}

fn update_space(res: &mut Resources, dt: f32) {
    microprofile::flip();
    microprofile::scope!("update", "all");
    res.space.run_system(KeyboardMover::new(&res.input_cache));
    {
        microprofile::scope!("update", "rigid body solver");

        use broadphase::BruteForce;
        use integrator::SemiImplicitEuler;
        res.space.run_system(
            CollisionSolver::<SemiImplicitEuler, BruteForce>::new(
                dt,
                &mut res.impulse_cache,
                SolverLoopCondition {
                    convergence_threshold: 0.2,
                    max_loops: 6,
                },
                phys::ForceField::gravity(Vector2::new(0.0, -9.81)),
            )
            .output_contacts(&mut res.debug_vis.contact_cache),
        );
    }
}

pub fn reload_space(space: &mut ecs::Space) {
    space.clear();

    let dir = "./examples/testgame/scenes";

    let file = std::fs::File::open(format!("{}/test.ron", dir)).expect("Failed to open file");
    space
        .read_ron_file::<recipes::Recipes>(file)
        .expect("Failed to load scene");

    space.create_pool(
        50,
        recipes::DynamicBlock {
            width: 0.8,
            height: 0.6,
            transform: Default::default(),
        },
    );
    space.create_pool(
        50,
        recipes::Ball {
            radius: 0.2,
            position: [0.0; 2],
        },
    );
}
