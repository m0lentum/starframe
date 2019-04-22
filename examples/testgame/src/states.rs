use crate::{controls::*, Resources};

use glium::{glutin, Surface};
use glutin::VirtualKeyCode as Key;
use moleengine::{
    ecs::{
        recipe::{parse_into_space, RecipeBook},
        space::Space,
    },
    physics2d::{collision::RigidBodySolver, systems::Motion},
    util::{
        gameloop::{GameLoop, LockstepLoop},
        inputcache::*,
        statemachine::{GameState, StateMachine, StateOp},
    },
    visuals_glium::shape::ShapeRenderer,
};

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
            reload_space(&mut res.space, &mut res.recipes);
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

    res.space.run_system(ShapeRenderer {
        target: &mut target,
        shaders: &res.shaders,
    });
    // TODO: remake collision visualizer with glutin

    target.finish().unwrap();
}

fn update_space(res: &mut Resources) {
    microprofile::flip();
    microprofile::scope!("update", "all");
    res.space.run_system(KeyboardMover::new(&res.input_cache));
    {
        microprofile::scope!("update", "rigid body solver");
        res.space.run_stateful_system(RigidBodySolver);
    }
    res.space.run_system(Motion);
}

pub fn reload_space(space: &mut Space, recipes: &mut RecipeBook) {
    let mes = std::fs::read_to_string("./examples/testgame/src/test_space.mes")
        .expect("File read failed");

    space.destroy_all();

    let r = parse_into_space(mes.as_str(), space, recipes);

    match r {
        Ok(_) => (),
        Err(e) => eprintln!("Error parsing space: {}", e),
    }
}
