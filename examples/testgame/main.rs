#[macro_use]
extern crate microprofile;

//

use rand::{distributions as distr, distributions::Distribution};
use ultraviolet as uv;

use moleengine::{
    core::{
        self,
        game::{self, Game},
        inputcache::{Key, MouseButton},
        Transform,
    },
    graphics as gx, physics2d as phys,
};

mod recipes;

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let game = Game::init(
        winit::window::WindowBuilder::new()
            .with_title("MoleEngine test")
            .with_inner_size(winit::dpi::LogicalSize {
                width: 800.0,
                height: 600.0,
            }),
    );
    let state = State::init(&game.renderer.device);
    game.run(game::LockstepLoop::from_fps(60), state);

    microprofile::shutdown!();
}

//
// Types
//

pub enum StateEnum {
    Playing,
    Paused,
}
pub struct State {
    pub state: StateEnum,
    pub space: MainSpace,
}
impl State {
    fn init(device: &wgpu::Device) -> Self {
        State {
            state: StateEnum::Playing,
            space: load_main_space(device).unwrap(),
        }
    }
}

pub type MainSpace = core::Space<MainSpaceFeatures>;

pub struct MainSpaceFeatures {
    pub tr: core::TransformFeature,
    pub shape: gx::ShapeFeature,
    pub physics: phys::PhysicsFeature,
}

impl core::space::FeatureSet for MainSpaceFeatures {
    fn init(cont: core::space::FeatureSetInit) -> Self {
        MainSpaceFeatures {
            tr: core::TransformFeature::new(cont),
            shape: gx::ShapeFeature::new(cont),
            physics: phys::PhysicsFeature::new(cont)
                .with_forcefield(phys::ForceField::gravity(uv::Vec2::new(0.0, -9.81))),
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

    fn draw(&mut self, space: core::SpaceReadAccess, ctx: &mut gx::RenderContext) {
        microprofile::scope!("render", "all");

        self.shape.draw(&space, &self.tr, ctx);
    }
}

impl game::GameState for State {
    fn tick(&mut self, dt: f32, game: &Game) -> Option<()> {
        //
        // State-independent stuff
        //
        if game.input.is_key_pressed(Key::Escape, None) {
            return None;
        }

        // mouse camera

        // let camera = &mut self.space.features.camera;
        // camera
        //     .controller
        //     .update_position(&globals.input, camera.scaling_factor());

        // if globals
        //     .input
        //     .is_mouse_button_pressed(MouseButton::Middle, Some(0))
        // {
        //     camera.controller.transform.0 = uv::Similarity2::identity();
        // }

        match self.state {
            //
            // Playing
            //
            StateEnum::Playing => {
                if game.input.is_key_pressed(Key::Space, Some(0)) {
                    self.state = StateEnum::Paused;
                    return Some(());
                }

                if game.input.is_key_pressed(Key::Return, Some(0)) {
                    self.space = load_main_space(&game.renderer.device).unwrap();
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
                if game.input.is_key_pressed(Key::S, Some(0)) {
                    self.space.spawn(recipes::DynamicBlock {
                        transform: Transform::new(random_pos(), random_angle(), 1.0),
                        width: distr::Uniform::from(0.6..1.0).sample(&mut rng),
                        height: distr::Uniform::from(0.3..0.8).sample(&mut rng),
                    });
                }
                if game.input.is_key_pressed(Key::T, Some(0)) {
                    self.space.spawn(recipes::Ball {
                        position: random_pos().into(),
                        radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
                    });
                }

                //

                self.space.tick(dt);

                Some(())
            }
            //
            // Paused
            //
            StateEnum::Paused => {
                if game.input.is_key_pressed(Key::Space, Some(0)) {
                    self.state = StateEnum::Playing;
                    return Some(());
                }

                Some(())
            }
        }
    }

    fn draw(&mut self, renderer: &mut gx::Renderer) {
        let mut ctx = renderer.draw_to_window();
        ctx.clear(wgpu::Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        });
        self.space.draw(&mut ctx);
        ctx.submit();
    }
}

fn load_main_space(device: &wgpu::Device) -> Option<MainSpace> {
    let mut space = MainSpace::with_capacity(150, device);
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
