#[macro_use]
extern crate microprofile;

//

use glium::{glutin, Surface};
use rand::{distributions as distr, distributions::Distribution};
use ultraviolet as uv;

use glutin::VirtualKeyCode as Key;
use moleengine::{
    core::{self, Transform},
    graphics::{self as gx, camera as cam},
    physics2d::{self as phys},
    util::{
        gameloop::{GameLoop, LockstepLoop},
        inputcache::InputCache,
        statemachine::{GameState, StateMachine, StateOp},
    },
};

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const _CYAN_COLOR: [f32; 4] = [0.3, 0.7, 0.8, 1.0];
const _LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

mod recipes;

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
    pub input_cache: InputCache,
}

pub struct MainSpaceFeatures {
    pub tr: core::TransformFeature,
    pub shape: gx::ShapeFeature,
    pub physics: phys::PhysicsFeature,
    pub camera: Camera,
}

impl core::space::FeatureSet for MainSpaceFeatures {
    fn init(cont: core::container::Init) -> Self {
        MainSpaceFeatures {
            tr: core::TransformFeature::new(cont),
            shape: gx::ShapeFeature::new(cont),
            physics: phys::PhysicsFeature::new(cont)
                .with_forcefield(phys::ForceField::gravity(uv::Vec2::new(0.0, -9.81))),
            camera: Camera::new(
                cam::MouseDragController::new(Transform::identity()),
                gx::camera::ScalingStrategy::ConstantDisplayArea {
                    width: 8.0,
                    height: 6.0,
                },
            ),
        }
    }

    fn tick(&mut self, dt: f32, space: core::SpaceAccess) {
        microprofile::flip();
        microprofile::scope!("update", "all");
        {
            microprofile::scope!("update", "rigid body solver");
            self.physics.tick(&space.read(), &mut self.tr, dt);
        }
    }

    fn draw(&self, space: core::SpaceReadAccess) {
        microprofile::scope!("render", "all");

        // TODO: consider abstracting context creation into the game loop
        let ctx = gx::Context::get();

        let mut target = ctx.display.draw();

        target.clear_color(BG_COLOR[0], BG_COLOR[1], BG_COLOR[2], BG_COLOR[3]);

        self.shape
            .draw(&space, &self.tr, &mut target, &self.camera, &ctx.shaders);

        target.finish().unwrap();
    }
}

pub type MainSpace = core::Space<MainSpaceFeatures>;

// ================== Setup resources ===========================

pub fn init_resources() -> Resources {
    let events = unsafe { gx::Context::init() };

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, T, P, LShift,
        ]);
    }

    let space = load_main_space().unwrap();

    Resources {
        events,
        space,
        input_cache,
    }
}

// ================ Playing ==================

pub struct StatePlaying;

impl GameState<Resources> for StatePlaying {
    fn update(&mut self, res: &mut Resources, dt: f32) -> StateOp<Resources> {
        if let Some(op) = handle_events(
            &mut res.events,
            &mut res.input_cache,
            &mut res.space.features.camera,
        ) {
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

        let random_pos = || {
            let mut rng = rand::thread_rng();
            uv::Vec2::new(
                distr::Uniform::from(-3.0..3.0).sample(&mut rng),
                distr::Uniform::from(0.0..2.0).sample(&mut rng),
            )
        };
        let random_angle = || {
            core::transform::Angle::Degrees(
                distr::Uniform::from(0.0..360.0).sample(&mut rand::thread_rng()),
            )
        };
        let mut rng = rand::thread_rng();
        if res.input_cache.is_key_pressed(Key::S, Some(0)) {
            res.space.spawn(recipes::DynamicBlock {
                transform: Transform::new(random_pos(), random_angle(), 1.0),
                width: distr::Uniform::from(0.6..1.0).sample(&mut rng),
                height: distr::Uniform::from(0.3..0.8).sample(&mut rng),
            });
        }
        if res.input_cache.is_key_pressed(Key::T, Some(0)) {
            res.space.spawn(recipes::Ball {
                position: random_pos().into(),
                radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
            });
        }

        // mouse camera

        let camera = &mut res.space.features.camera;
        camera
            .controller
            .update_position(&res.input_cache, camera.scaling_factor());

        if res
            .input_cache
            .is_mouse_button_pressed(glutin::MouseButton::Middle, Some(0))
        {
            camera.controller.transform.0 = uv::Similarity2::identity();
        }

        //

        res.space.tick(dt);

        res.input_cache.tick();
        StateOp::Stay
    }

    fn render(&mut self, res: &mut Resources) {
        res.space.draw();
    }
}

// ===================== Paused ========================

pub struct StatePaused;

impl GameState<Resources> for StatePaused {
    fn update(&mut self, res: &mut Resources, _dt: f32) -> StateOp<Resources> {
        if let Some(op) = handle_events(
            &mut res.events,
            &mut res.input_cache,
            &mut res.space.features.camera,
        ) {
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
        res.space.draw();
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

fn load_main_space() -> Option<MainSpace> {
    let mut space = MainSpace::with_capacity(150);
    space.create_pool::<recipes::Player>(5).unwrap();
    space.create_pool::<recipes::Ball>(20).unwrap();
    space.create_pool::<recipes::DynamicBlock>(20).unwrap();

    let dir = "./examples/testgame/scenes";

    let file = std::fs::File::open(format!("{}/test.ron", dir)).expect("Failed to open file");
    space
        .read_ron_file::<recipes::Recipes>(file)
        .expect("Failed to load scene");

    Some(space)
}
