#[macro_use]
extern crate microprofile;

//

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
    visuals_glium::{self as vis, camera as cam},
};

use rand::{distributions as distr, distributions::Distribution};
use ultraviolet as uv;

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const _CYAN_COLOR: [f32; 4] = [0.3, 0.7, 0.8, 1.0];
const _LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let res = init_resources();
    let mut sm = StateMachine::new(res, Box::new(StatePlaying));
    let l = LockstepLoop::from_fps(60);
    l.begin(&mut sm);

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}

// ================ Main types ===========================

pub type Camera = cam::Camera2D<cam::MouseDragController>;

pub struct Resources {
    pub events: glutin::EventsLoop,
    pub space: MainSpace,
    pub camera: Camera,
    pub input_cache: InputCache,
    pub impulse_cache: phys::constraint::ImpulseCache, // TODO: this can now exist inside a spacefeature
}

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

    fn containers(&mut self) -> Vec<&mut dyn core::container::ContainerAccess> {
        vec![&mut self.tr, &mut self.shape]
    }

    fn tick(&mut self, dt: f32) {}
}

pub type MainSpace = core::Space<MainSpaceFeatures>;

// ================== Setup resources ===========================

pub fn init_resources() -> Resources {
    let events = unsafe { vis::Context::init() };

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, T, LShift,
        ]);
    }

    let space = load_main_space().unwrap();

    let camera = cam::Camera2D::new(
        cam::MouseDragController::new(Transform::identity()),
        vis::camera::ScalingStrategy::ConstantDisplayArea {
            width: 8.0,
            height: 6.0,
        },
    );

    let impulse_cache = phys::constraint::ImpulseCache::new();

    Resources {
        events,
        space,
        camera,
        input_cache,
        impulse_cache,
    }
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
            res.space = load_main_space().unwrap();
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

    res.space.features.shape.draw(
        &res.space.features.tr,
        &mut target,
        &res.camera,
        &ctx.shaders,
    );

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

fn load_main_space() -> Option<MainSpace> {
    let ctx = vis::Context::get();
    let square =
        |size| vis::Shape::new_square(&ctx.display, size, vis::shape::ShapeStyle::Fill([1.0; 4]));
    let tr = |x, y| Transform::from_position(uv::Vec2::new(x, y));

    let mut space = MainSpace::with_capacity(10);

    space.create_object_with(|id, feat| {
        feat.tr.insert(id, tr(1.0, 0.0));
        feat.shape.insert(id, square(1.0));
    })?;

    space.create_object_with(|id, feat| {
        feat.tr.insert(id, tr(0.0, 0.0));
    })?;

    space.create_object_with(|id, feat| {
        feat.shape.insert(id, square(1.2));
    })?;

    let mut obj_whomst_will_die = space.create_object_with(|id, feat| {
        feat.tr.insert(id, tr(-1.0, 1.0));
        feat.shape.insert(id, square(1.2));
    })?;
    obj_whomst_will_die.kill();

    space.create_object_with(|id, feat| {
        feat.shape.insert(id, square(1.2));
    })?;

    Some(space)
}
