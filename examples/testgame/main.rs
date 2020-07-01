#[macro_use]
extern crate microprofile;

//

use rand::{distributions as distr, distributions::Distribution};

use starframe::{
    core::{
        game::{self, Game},
        graph,
        inputcache::{Key, MouseButton},
        math as m,
    },
    graphics as gx, physics as phys,
};

mod player;
mod recipes;
use recipes::Recipe;

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let game = Game::init(
        winit::window::WindowBuilder::new()
            .with_title("starframe test")
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
    state: StateEnum,
    graph: MyGraph,
    player: player::PlayerController,
    physics: phys::PhysicsSolver,
    camera: gx::camera::MouseDragCamera,
    shape_renderer: gx::ShapeRenderer,
}
impl State {
    fn init(device: &wgpu::Device) -> Self {
        State {
            state: StateEnum::Playing,
            graph: MyGraph::load_file(),
            player: player::PlayerController::new(),
            physics: phys::PhysicsSolver::new(),
            camera: gx::camera::MouseDragCamera::new(
                gx::camera::ScalingStrategy::ConstantDisplayArea {
                    width: 8.0,
                    height: 6.0,
                },
            ),
            shape_renderer: gx::ShapeRenderer::new(device),
        }
    }
}

pub struct MyGraph {
    graph: graph::Graph,
    l_transform: graph::Layer<m::Transform>,
    l_collider: graph::Layer<phys::Collider>,
    l_body: graph::Layer<phys::RigidBody>,
    l_shape: graph::Layer<gx::Shape>,
    l_playertag: graph::Layer<player::Tag>,
}
impl MyGraph {
    pub fn new() -> Self {
        let mut graph = graph::Graph::new();
        let l_transform = graph.create_layer();
        let l_collider = graph.create_layer();
        let l_body = graph.create_layer();
        let l_shape = graph.create_layer();
        let l_playertag = graph.create_layer();
        MyGraph {
            graph,
            l_transform,
            l_collider,
            l_body,
            l_shape,
            l_playertag,
        }
    }

    pub fn load_file() -> Self {
        let mut graph = Self::new();
        let dir = "./examples/testgame/scenes";
        let file = std::fs::File::open(format!("{}/test.ron", dir)).expect("Failed to open file");
        Recipe::read_from_file(file, &mut graph).expect("Failed to parse file");
        graph
    }
}

impl game::GameState for State {
    fn tick(&mut self, dt: f32, game: &Game) -> Option<()> {
        microprofile::flip();
        microprofile::scope!("update", "all");

        //
        // State-independent stuff
        //
        if game.input.is_key_pressed(Key::Escape, None) {
            return None;
        }

        // mouse camera

        self.camera
            .update(&game.input, game.renderer.window_size().into());
        if (game.input).is_mouse_button_pressed(MouseButton::Middle, Some(0)) {
            self.camera.transform = m::Transform::identity();
        }

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
                    self.graph = MyGraph::load_file();
                }

                let random_pos = || {
                    let mut rng = rand::thread_rng();
                    m::Vec2::new(
                        distr::Uniform::from(-3.0..3.0).sample(&mut rng),
                        distr::Uniform::from(0.0..2.0).sample(&mut rng),
                    )
                };
                let random_angle = || {
                    m::Angle::Deg(distr::Uniform::from(0.0..360.0).sample(&mut rand::thread_rng()))
                };
                let mut rng = rand::thread_rng();
                if game.input.is_key_pressed(Key::S, Some(0)) {
                    Recipe::DynamicBlock(recipes::Block {
                        transform: m::TransformBuilder::new()
                            .with_position(random_pos())
                            .with_rotation(random_angle()),
                        width: distr::Uniform::from(0.6..1.0).sample(&mut rng),
                        height: distr::Uniform::from(0.3..0.8).sample(&mut rng),
                    })
                    .spawn(&mut self.graph);
                }
                if game.input.is_key_pressed(Key::T, Some(0)) {
                    Recipe::Ball {
                        position: random_pos().into(),
                        radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
                    }
                    .spawn(&mut self.graph);
                }

                //

                {
                    microprofile::scope!("update", "physics");
                    let grav = phys::forcefield::Gravity(m::Vec2::new(0.0, -9.81));
                    let _contact_evts = self.physics.tick(
                        &self.graph.graph,
                        &mut self.graph.l_transform,
                        &mut self.graph.l_body,
                        &self.graph.l_collider,
                        dt,
                        Some(&grav),
                    );
                }
                {
                    microprofile::scope!("update", "player");
                    self.player.tick(
                        &self.graph.graph,
                        &mut self.graph.l_transform,
                        &mut self.graph.l_body,
                        &self.graph.l_playertag,
                        &game.input,
                    );
                }

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
        microprofile::scope!("render", "all");

        let mut ctx = renderer.draw_to_window();
        ctx.clear(wgpu::Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        });

        self.shape_renderer.draw(
            &self.graph.l_shape,
            &self.graph.l_transform,
            &self.graph.graph,
            &self.camera,
            &mut ctx,
        );

        ctx.submit();
    }
}
