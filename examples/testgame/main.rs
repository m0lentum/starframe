#[macro_use]
extern crate microprofile;

//

use rand::{distributions as distr, distributions::Distribution};

use starframe::{
    self as sf,
    game::{self, Game},
    graph, graphics as gx,
    input::{Key, MouseButton},
    math as m, physics as phys,
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
    physics: phys::Physics,
    camera: gx::camera::MouseDragCamera,
    shape_renderer: gx::ShapeRenderer,
}
impl State {
    fn init(device: &wgpu::Device) -> Self {
        State {
            state: StateEnum::Playing,
            graph: MyGraph::new(),
            player: player::PlayerController::new(),
            physics: phys::Physics::new(),
            camera: gx::camera::MouseDragCamera::new(
                gx::camera::ScalingStrategy::ConstantDisplayArea {
                    width: 20.0,
                    height: 10.0,
                },
            ),
            shape_renderer: gx::ShapeRenderer::new(device),
        }
    }

    fn reset_from_file(&mut self) {
        self.physics.reset();

        self.graph = MyGraph::new();
        let dir = "./examples/testgame/scenes";
        let file = std::fs::File::open(format!("{}/test.ron", dir)).expect("Failed to open file");
        Recipe::read_from_file(file, &mut self.graph, &mut self.physics).unwrap_or_else(|err| {
            println!("Failed to parse file: {}", err);
        });
    }
}

pub struct MyGraph {
    graph: graph::Graph,
    l_transform: graph::Layer<m::Transform>,
    l_collider: graph::Layer<phys::Collider>,
    l_body: graph::Layer<phys::RigidBody>,
    l_shape: graph::Layer<gx::Shape>,
    l_player: graph::Layer<player::Player>,
    l_evt_sink: sf::EventSinkLayer<MyGraph>,
}
impl MyGraph {
    pub fn new() -> Self {
        let mut graph = graph::Graph::new();
        let l_transform = graph.create_layer();
        let l_collider = graph.create_layer();
        let l_body = graph.create_layer();
        let l_shape = graph.create_layer();
        let l_player = graph.create_layer();
        let l_evt_sinks = graph.create_layer();
        MyGraph {
            graph,
            l_transform,
            l_collider,
            l_body,
            l_shape,
            l_player,
            l_evt_sink: l_evt_sinks,
        }
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

        // reload

        if game.input.is_key_pressed(Key::Return, Some(0)) {
            self.reset_from_file();
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

                let random_pos = || {
                    let mut rng = rand::thread_rng();
                    m::Vec2::new(
                        distr::Uniform::from(-5.0..5.0).sample(&mut rng),
                        distr::Uniform::from(1.0..4.0).sample(&mut rng),
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
                        height: distr::Uniform::from(0.5..0.8).sample(&mut rng),
                    })
                    .spawn(&mut self.graph, &mut self.physics);
                }
                if game.input.is_key_pressed(Key::T, Some(0)) {
                    Recipe::Ball {
                        position: random_pos().into(),
                        radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
                    }
                    .spawn(&mut self.graph, &mut self.physics);
                }

                //

                {
                    microprofile::scope!("update", "physics");
                    let grav = phys::forcefield::Gravity(m::Vec2::new(0.0, -9.81));
                    self.physics.tick(
                        &self.graph.graph,
                        &mut self.graph.l_transform,
                        &mut self.graph.l_body,
                        &self.graph.l_collider,
                        &mut self.graph.l_evt_sink,
                        dt,
                        Some(&grav),
                    );
                }
                {
                    microprofile::scope!("update", "player");
                    self.player.tick(&mut self.graph, &game.input);
                }

                self.graph.l_evt_sink.flush(&self.graph.graph)(&mut self.graph);

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
