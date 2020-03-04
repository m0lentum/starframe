use crate::{Camera, Resources};

use glium::{glutin, Surface};
use glutin::VirtualKeyCode as Key;
use moleengine::{
    core,
    physics2d::{self as phys, collision as coll, integrator},
    util::{
        gameloop::{GameLoop, LockstepLoop},
        inputcache::InputCache,
        statemachine::{GameState, StateMachine, StateOp},
        Transform,
    },
    visuals_glium as vis,
};

use rand::{distributions as distr, distributions::Distribution};
use ultraviolet as uv;

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const _CYAN_COLOR: [f32; 4] = [0.3, 0.7, 0.8, 1.0];
const _LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

// ================= Space Features =========================

pub struct MainSpaceFeatures {
    tr: core::space::TransformFeature,
    shape: core::space::ShapeFeature,
}

impl core::space::FeatureSet for MainSpaceFeatures {
    fn init(capacity: core::space::IdType) -> Self {
        MainSpaceFeatures {
            tr: core::space::TransformFeature::with_capacity(capacity),
            shape: core::space::ShapeFeature::with_capacity(capacity),
        }
    }

    fn tick(&mut self, dt: f32) {}
}

pub type MainSpace = core::Space<MainSpaceFeatures>;

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
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache, &mut res.camera) {
            return op;
        }
        if res.input_cache.is_key_pressed(Key::Escape, None) {
            return StateOp::Destroy;
        }
        if res.input_cache.is_key_pressed(Key::Space, Some(0)) {
            return StateOp::Push(Box::new(StatePaused));
        }

        if res.input_cache.is_key_pressed(Key::Return, Some(0)) {
            reload_main_space(&mut res.space);
        }

        // pool spawning

        // mouse camera

        res.camera
            .controller
            .update_position(&res.input_cache, res.camera.scaling_factor());

        if res
            .input_cache
            .is_mouse_button_pressed(glutin::MouseButton::Middle, Some(0))
        {
            res.camera.controller.transform.0 = uv::Similarity2::identity();
        }

        //

        update_space(res, dt);

        res.input_cache.tick();
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
        if let Some(op) = handle_events(&mut res.events, &mut res.input_cache, &mut res.camera) {
            return op;
        }
        if res.input_cache.is_key_pressed(Key::Escape, None) {
            return StateOp::Destroy;
        }
        if res.input_cache.is_key_pressed(Key::Space, Some(0)) {
            return StateOp::Pop;
        }

        res.input_cache.tick();
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
    camera: &mut Camera,
) -> Option<StateOp<Resources>> {
    let mut should_close = false;
    use glutin::WindowEvent::*;
    events.poll_events(|evt| match evt {
        glutin::Event::WindowEvent { event, .. } => {
            input_cache.track_window_event(&event);
            match event {
                CloseRequested => should_close = true,
                Resized(_) => camera.update_scaling(),
                _ => (),
            }
        }
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

    let f = &mut res.space.features;
    f.shape.sync_transforms(&f.tr);
    f.shape.draw(&mut target, &res.camera, &ctx.shaders);

    target.finish().unwrap();
}

fn update_space(res: &mut Resources, dt: f32) {
    microprofile::flip();
    microprofile::scope!("update", "all");
    {
        microprofile::scope!("update", "rigid body solver");
        // TODO
    }
}

fn reload_main_space(space: &mut MainSpace) {
    let ctx = vis::Context::get();
    let square =
        |size| vis::Shape::new_square(&ctx.display, size, vis::shape::ShapeStyle::Fill([1.0; 4]));
    let tr = |x, y| Transform::from_position(uv::Vec2::new(x, y));

    space.clear();

    let obj = space.create_object();
    space.features.tr.add_transform(&obj, tr(1.0, 0.0));
    space.features.shape.add_shape(&obj, square(1.0));

    let obj = space.create_object();
    space.features.tr.add_transform(&obj, tr(0.0, 0.0));

    let obj = space.create_object();
    space.features.tr.add_transform(&obj, tr(-1.0, 1.0));
    space.features.shape.add_shape(&obj, square(1.2));
}
