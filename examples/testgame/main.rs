#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

use rand::{distributions as distr, distributions::Distribution};

use starframe::{
    self as sf,
    game::{self, Game},
    graph, graphics as gx,
    input::{Key, MouseButton},
    math::{self as m, uv},
    physics as phys,
};

mod mousegrab;
use mousegrab::MouseGrabber;
mod player;
mod recipes;
use recipes::Recipe;

fn main() {
    let game = Game::init(
        60,
        winit::window::WindowBuilder::new()
            .with_title("starframe test")
            .with_inner_size(winit::dpi::LogicalSize {
                width: 800.0,
                height: 600.0,
            }),
    );
    let state = State::init(&game.renderer.device);
    game.run(state);
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
    mouse_mode: MouseMode,
    mouse_grabber: MouseGrabber,
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
            mouse_mode: MouseMode::Grab,
            mouse_grabber: MouseGrabber::new(),
            physics: phys::Physics::new(phys::collision::HGridParams {
                approx_bounds: phys::collision::AABB {
                    min: m::Vec2::new(-20.0, -10.0),
                    max: m::Vec2::new(20.0, 10.0),
                },
                smallest_obj_radius: 0.5,
                largest_obj_radius: 3.0,
                expected_obj_count: 100,
            }),
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
        self.physics.clear_constraints();
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

#[derive(Clone, Copy, Debug)]
pub enum MouseMode {
    /// Grab objects with the mouse
    Grab,
    /// Move the camera with the mouse
    Camera,
}

/// The recipes in a scene plus some adjustable parameters.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Scene {
    gravity: [f64; 2],
    spawn_zone: phys::collision::AABB,
    recipes: Vec<Recipe>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81],
            spawn_zone: phys::collision::AABB {
                min: m::Vec2::new(-5.0, 1.0),
                max: m::Vec2::new(5.0, 4.0),
            },
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
    l_pose: graph::Layer<m::Pose>,
    l_collider: graph::Layer<phys::Collider>,
    l_body: graph::Layer<phys::Body>,
    l_rope: graph::Layer<phys::Rope>,
    l_shape: graph::Layer<gx::Shape>,
    l_player: graph::Layer<player::Player>,
    evt_graph: sf::event::EventGraph<MyGraph>,
}
impl MyGraph {
    pub fn new() -> Self {
        let mut graph = graph::Graph::new();
        let l_pose = graph.create_layer();
        let l_collider = graph.create_layer();
        let l_body = graph.create_layer();
        let l_rope = graph.create_layer();
        let l_shape = graph.create_layer();
        let l_player = graph.create_layer();
        let evt_graph = sf::event::EventGraph::new(&mut graph);
        MyGraph {
            graph,
            l_pose,
            l_collider,
            l_body,
            l_rope,
            l_shape,
            l_player,
            evt_graph,
        }
    }
}

//
// State updates
//

impl game::GameState for State {
    fn tick(&mut self, dt: f64, game: &Game) -> Option<()> {
        //
        // State-independent stuff
        //

        // exit on esc
        if game.input.is_key_pressed(Key::Escape, None) {
            return None;
        }

        // adjust physics substeps
        if game.input.is_key_pressed(Key::NumpadAdd, Some(0)) {
            self.physics.substeps += 1;
            println!("Substeps: {}", self.physics.substeps);
        } else if game.input.is_key_pressed(Key::NumpadSubtract, Some(0))
            && self.physics.substeps > 1
        {
            self.physics.substeps -= 1;
            println!("Substeps: {}", self.physics.substeps);
        }

        // mouse controls

        if game.input.is_key_pressed(Key::V, Some(0)) {
            self.mouse_mode = match self.mouse_mode {
                MouseMode::Grab => MouseMode::Camera,
                MouseMode::Camera => MouseMode::Grab,
            };
            println!("Mouse mode: {:?}", self.mouse_mode);
        }
        match self.mouse_mode {
            MouseMode::Grab => {
                self.mouse_grabber.update(
                    &game.input,
                    &self.camera,
                    game.renderer.window_size().into(),
                    &mut self.physics,
                    &self.graph,
                );
            }
            MouseMode::Camera => {
                self.camera
                    .update(&game.input, game.renderer.window_size().into());
                if (game.input).is_mouse_button_pressed(MouseButton::Middle, Some(0)) {
                    self.camera.pose = uv::DSimilarity2::identity();
                }
            }
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

        // spawn stuff also when paused
        let zone = self.scene.spawn_zone;
        let random_pos = || {
            let mut rng = rand::thread_rng();
            m::Vec2::new(
                distr::Uniform::from(zone.min.x..zone.max.x).sample(&mut rng),
                distr::Uniform::from(zone.min.y..zone.max.y).sample(&mut rng),
            )
        };
        let random_angle =
            || m::Angle::Deg(distr::Uniform::from(0.0..360.0).sample(&mut rand::thread_rng()));
        let random_vel = || {
            let mut rng = rand::thread_rng();
            [
                distr::Uniform::from(-5.0..5.0).sample(&mut rng),
                distr::Uniform::from(-5.0..5.0).sample(&mut rng),
            ]
        };
        let mut rng = rand::thread_rng();
        if game.input.is_key_pressed(Key::S, Some(0)) {
            Recipe::Block(recipes::Block {
                pose: m::PoseBuilder::new()
                    .with_position(random_pos())
                    .with_rotation(random_angle()),
                width: distr::Uniform::from(0.6..1.0).sample(&mut rng),
                height: distr::Uniform::from(0.5..0.8).sample(&mut rng),
                is_static: false,
            })
            .spawn(&mut self.graph, &mut self.physics);
        }
        if game.input.is_key_pressed(Key::T, Some(0)) {
            Recipe::Ball(recipes::Ball {
                position: random_pos().into(),
                radius: distr::Uniform::from(0.1..0.4).sample(&mut rng),
                restitution: 1.0,
                start_velocity: random_vel(),
                is_static: false,
            })
            .spawn(&mut self.graph, &mut self.physics);
        }
        if game.input.is_key_pressed(Key::D, Some(0)) {
            Recipe::Capsule(recipes::Capsule {
                pose: m::PoseBuilder::new()
                    .with_position(random_pos())
                    .with_rotation(random_angle()),
                length: distr::Uniform::from(0.4..0.8).sample(&mut rng),
                radius: distr::Uniform::from(0.1..0.5).sample(&mut rng),
                is_static: false,
            })
            .spawn(&mut self.graph, &mut self.physics);
        }

        match (&self.state, game.input.is_key_pressed(Key::Space, Some(0))) {
            //
            // Playing or stepping manually
            //
            (StateEnum::Playing, _) | (StateEnum::Paused, true) => {
                if game.input.is_key_pressed(Key::P, Some(0)) {
                    self.state = StateEnum::Paused;
                    return Some(());
                }

                {
                    let grav = phys::forcefield::Gravity(self.scene.gravity.into());
                    self.physics.tick(
                        &self.graph.graph,
                        &mut self.graph.l_pose,
                        &mut self.graph.l_body,
                        &self.graph.l_collider,
                        &self.graph.l_rope,
                        &mut self.graph.evt_graph.sinks,
                        dt,
                        &grav,
                    );
                }
                {
                    self.player.tick(&mut self.graph, &game.input);
                }

                self.graph.evt_graph.flush(&self.graph.graph)(&mut self.graph);

                Some(())
            }
            //
            // Paused
            //
            (StateEnum::Paused, false) => {
                if game.input.is_key_pressed(Key::P, Some(0)) {
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

        self.shape_renderer.draw(
            &self.graph.l_shape,
            &self.graph.l_pose,
            &self.graph.graph,
            &self.camera,
            &mut ctx,
        );

        ctx.submit();
    }
}
