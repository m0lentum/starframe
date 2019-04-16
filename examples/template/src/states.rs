use glium::{glutin, Surface};

use moleengine::{
    ecs::space::Space,
    physics2d::systems::Motion,
    util::{
        inputcache::InputCache,
        gameloop::{GameLoop, LockstepLoop},
        statemachine::{GameState, StateMachine, StateOp},
    },
};

use crate::Resources;

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

// ============= Begin ===============

pub fn begin(res: Resources) {
    let mut sm = StateMachine::new(res, Box::new(StatePlaying));
    let l = LockstepLoop::from_fps(60);
    l.begin(&mut sm);
}

// ============= Playing ===============

pub struct StatePlaying;

impl GameState<Resources> for StatePlaying {
    fn update(&mut self, res: &mut Resources) -> StateOp<Resources> {
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache) {
            return op;
        }

        if res.input_cache.is_key_pressed(glutin::VirtualKeyCode::Escape, None) {
            return StateOp::Destroy;
        }
        if res.input_cache.is_key_pressed(glutin::VirtualKeyCode::Space, Some(1)) {
            return StateOp::Push(Box::new(StatePaused));
        }

        update_space(&mut res.space);

        res.input_cache.update_ages();
        StateOp::Stay
    }

    fn render(&mut self, res: &mut Resources) {
        let mut target = res.display.draw();
        target.clear_color(BG_COLOR[0], BG_COLOR[1], BG_COLOR[2], BG_COLOR[3]);
        target.finish().unwrap();
    }
}

// ============= Paused ===============

pub struct StatePaused;

impl GameState<Resources> for StatePaused {
    fn update(&mut self, res: &mut Resources) -> StateOp<Resources> {
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache) {
            return op;
        }

        if res.input_cache.is_key_pressed(glutin::VirtualKeyCode::Space, Some(1)) {
            return StateOp::Pop;
        }

        StateOp::Stay
    }

    fn render(&mut self, res: &mut Resources) {}
}

// ============== Helper functions ==================

//fn draw_space(gl: &mut GlGraphics, args: RenderArgs, space: &mut Space) {
//    let ctx = gl.draw_begin(args.viewport());
//
//    graphics::clear(BG_COLOR, gl);
//
//    space.run_system(ShapeRenderer::new(&ctx, gl));
//
//    gl.draw_end();
//}

fn update_space(space: &mut Space) {
    space.run_system(Motion);
}

fn handle_events(events: &mut glutin::EventsLoop, input_cache: &mut InputCache) -> Option<StateOp<Resources>> {
    let mut should_close = false;
    events.poll_events(|evt| match evt {
        glutin::Event::WindowEvent { event, .. } => match event {
            glutin::WindowEvent::CloseRequested => should_close = true,
            glutin::WindowEvent::KeyboardInput { input, .. } => {
                input_cache.handle_keyboard(input)
            }
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
