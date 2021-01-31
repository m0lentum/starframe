#[macro_use]
extern crate microprofile;

//

use rand::{distributions as distr, distributions::Distribution};

use starframe::{
    self as sf,
    game::{self, Game},
    graph, graphics as gx,
    input::{Key, MouseButton},
    math::{self, uv},
    physics as phys,
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
    scene: Scene,
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
            scene: Scene::default(),
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

    fn reset(&mut self) {
        self.physics.reset();
        self.graph = MyGraph::new();
    }

    fn read_scene(&mut self, file_idx: usize) {
        let dir = std::fs::read_dir("./examples/testgame/scenes");
        match dir {
            Err(err) => eprintln!("Scenes dir not found: {}", err),
            Ok(mut dir) => {
                if let Some(Ok(entry)) = dir.nth(file_idx) {
                    let file = std::fs::File::open(entry.path());
                    match file {
                        Ok(file) => {
                            let scene = Scene::read_from_file(file);
                            match scene {
                                Err(err) => eprintln!("Failed to parse file: {}", err),
                                Ok(scene) => self.scene = scene,
                            }
                        }
                        Err(err) => eprintln!("Failed to open file: {}", err),
                    }
                }
            }
        }
    }

    fn instantiate_scene(&mut self) {
        self.scene.instantiate(&mut self.graph, &mut self.physics);
    }
}

/// The recipes in a scene plus some adjustable parameters.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Scene {
    gravity: [f32; 2],
    recipes: Vec<Recipe>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81],
            recipes: vec![],
        }
    }
}

impl Scene {
    pub fn read_from_file(file: std::fs::File) -> Result<Self, ron::de::Error> {
        use serde::Deserialize;
        use std::io::Read;

        let mut reader = std::io::BufReader::new(file);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;

        let mut deser = ron::de::Deserializer::from_bytes(bytes.as_slice())?;
        Scene::deserialize(&mut deser)
    }

    pub fn instantiate(&self, graph: &mut crate::MyGraph, physics: &mut phys::Physics) {
        for recipe in &self.recipes {
            recipe.spawn(graph, physics);
        }
    }
}

/// The entity graph.
pub struct MyGraph {
    graph: graph::Graph,
    l_transform: graph::Layer<uv::Isometry2>,
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

//
// State updates
//

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
            self.camera.pose = uv::Similarity2::identity();
        }

        // reload

        for (idx, num_key) in [
            Key::Key1,
            Key::Key2,
            Key::Key3,
            Key::Key4,
            Key::Key5,
            Key::Key6,
            Key::Key7,
            Key::Key8,
            Key::Key9,
        ]
        .iter()
        .enumerate()
        {
            if game.input.is_key_pressed(*num_key, Some(0)) {
                self.reset();
                self.read_scene(idx);
                self.instantiate_scene();
            }
        }
        // reload current scene
        if game.input.is_key_pressed(Key::Return, Some(0)) {
            self.reset();
            self.instantiate_scene();
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
                    uv::Vec2::new(
                        distr::Uniform::from(-5.0..5.0).sample(&mut rng),
                        distr::Uniform::from(1.0..4.0).sample(&mut rng),
                    )
                };
                let random_angle = || {
                    math::Angle::Deg(
                        distr::Uniform::from(0.0..360.0).sample(&mut rand::thread_rng()),
                    )
                };
                let mut rng = rand::thread_rng();
                if game.input.is_key_pressed(Key::S, Some(0)) {
                    Recipe::DynamicBlock(recipes::Block {
                        pose: math::IsometryBuilder::new()
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
                    let grav = phys::forcefield::Gravity(self.scene.gravity.into());
                    self.physics.tick(
                        &self.graph.graph,
                        &mut self.graph.l_transform,
                        &mut self.graph.l_body,
                        &self.graph.l_collider,
                        &mut self.graph.l_evt_sink,
                        dt,
                        &grav,
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
