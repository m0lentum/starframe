#[macro_use]
extern crate microprofile;

//

use rand::{distributions as distr, distributions::Distribution};
use ultraviolet as uv;

use glutin::VirtualKeyCode as Key;
use moleengine::{
    core::{
        self,
        gameloop::{GameLoop, GameState, Globals, LockstepLoop},
        Transform,
    },
    graphics::{self as gx, camera as cam},
    physics2d::{self as phys},
};

mod recipes;

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    LockstepLoop::from_fps(60).run(State::init());

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}

//
// Types
//

pub type Camera = cam::Camera2D<cam::MouseDragController>;

pub enum StateEnum {
    Playing,
    Paused,
}
pub struct State {
    pub state: StateEnum,
    pub space: MainSpace,
}
impl State {
    fn init() -> Self {
        State {
            state: StateEnum::Playing,
            space: load_main_space().unwrap(),
        }
    }
}

pub type MainSpace = core::Space<MainSpaceFeatures>;

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

    fn draw<S: glium::Surface>(&self, space: core::SpaceReadAccess, target: &mut S) {
        microprofile::scope!("render", "all");

        self.shape.draw(&space, &self.tr, target, &self.camera);
    }
}

impl GameState for State {
    fn tick(mut self, dt: f32, globals: &Globals) -> Option<Self> {
        //
        // State-independent stuff
        //
        if globals.input.is_key_pressed(Key::Escape, None) {
            return None;
        }

        // mouse camera

        let camera = &mut self.space.features.camera;
        camera
            .controller
            .update_position(&globals.input, camera.scaling_factor());

        if globals
            .input
            .is_mouse_button_pressed(glutin::MouseButton::Middle, Some(0))
        {
            camera.controller.transform.0 = uv::Similarity2::identity();
        }

        match self.state {
            //
            // Playing
            //
            StateEnum::Playing => {
                if globals.input.is_key_pressed(Key::Space, Some(0)) {
                    self.state = StateEnum::Paused;
                    return Some(self);
                }

                if globals.input.is_key_pressed(Key::Return, Some(0)) {
                    self.space = load_main_space().unwrap();
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
                if globals.input.is_key_pressed(Key::S, Some(0)) {
                    self.space.spawn(recipes::DynamicBlock {
                        transform: Transform::new(random_pos(), random_angle(), 1.0),
                        width: distr::Uniform::from(0.6..1.0).sample(&mut rng),
                        height: distr::Uniform::from(0.3..0.8).sample(&mut rng),
                    });
                }
                if globals.input.is_key_pressed(Key::T, Some(0)) {
                    self.space.spawn(recipes::Ball {
                        position: random_pos().into(),
                        radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
                    });
                }

                //

                self.space.tick(dt);

                Some(self)
            }
            //
            // Paused
            //
            StateEnum::Paused => {
                if globals.input.is_key_pressed(Key::Space, Some(0)) {
                    self.state = StateEnum::Playing;
                    return Some(self);
                }

                Some(self)
            }
        }
    }

    fn draw<S: glium::Surface>(&self, target: &mut S, _globals: &Globals) {
        self.space.draw(target);
    }

    fn on_event(&mut self, evt: &glutin::Event, _globals: &Globals) {
        match evt {
            glutin::Event::WindowEvent {
                event: glutin::WindowEvent::Resized(_),
                ..
            } => self.space.features.camera.update_scaling(),
            _ => (),
        }
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
