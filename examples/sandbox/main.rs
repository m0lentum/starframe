#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

use rand::{distributions as distr, distributions::Distribution};

use egui_winit_platform as egui_wp;

use starframe as sf;

mod mousegrab;
use mousegrab::MouseGrabber;
mod player;
mod recipes;
use recipes::Recipe;

fn main() {
    let window = winit::window::WindowBuilder::new()
        .with_title("starframe test")
        .with_inner_size(winit::dpi::LogicalSize {
            width: 1280.0,
            height: 720.0,
        });

    sf::Game::run(sf::GameParams {
        window,
        fps: 60,
        on_event: |state: &mut State, evt| {
            state.egui_platform.handle_event(evt);
        },
    });
}

//
// Types
//

pub enum StateEnum {
    Playing,
    Paused,
}
pub struct State {
    scenes_available: Vec<std::path::PathBuf>,
    scene: Scene,
    state: StateEnum,
    graph: sf::Graph,
    // gameplay
    player: player::PlayerController,
    mouse_grabber: MouseGrabber,
    physics: sf::Physics,
    // graphics
    camera: sf::Camera,
    camera_ctl: sf::MouseDragCameraController,
    mesh_renderer: sf::MeshRenderer,
    outline_renderer: sf::OutlineRenderer,
    debug_visualizer: sf::DebugVisualizer,
    // egui stuff
    egui_platform: egui_wp::Platform,
    egui_pass: egui_wgpu_backend::RenderPass,
    last_egui_output: egui::FullOutput,
    // UI states
    outline_interp: f32,
    bvh_vis_active: bool,
    bvh_vis_levels: usize,
    island_vis_active: bool,
    spawner_circle_r: f64,
    time_scale: f64,
}
impl State {
    fn init(renderer: &sf::Renderer) -> Self {
        State {
            scenes_available: read_available_scenes().expect("Failed to read scenes directory"),
            scene: Scene::default(),
            state: StateEnum::Playing,
            graph: sf::new_graph! {
                // starframe types
                sf::Pose,
                sf::Body,
                sf::Collider,
                sf::Rope,
                sf::Mesh,
                // our types
                player::Player,
            },
            player: player::PlayerController::new(),
            mouse_grabber: MouseGrabber::new(),
            physics: sf::Physics::new(
                sf::physics::TuningConstants::default(),
                sf::CollisionMaskMatrix::default(),
            ),
            camera: sf::Camera::new(sf::CameraScalingStrategy::ConstantDisplayArea {
                width: 20.0,
                height: 10.0,
            }),
            camera_ctl: sf::MouseDragCameraController {
                activate_button: sf::MouseButton::Middle.into(),
                reset_button: Some(sf::Key::R.into()),
                ..Default::default()
            },
            mesh_renderer: sf::MeshRenderer::new(renderer),
            outline_renderer: sf::OutlineRenderer::new(
                sf::OutlineParams {
                    thickness: 15,
                    color: [0.0, 0.0, 0.0, 1.0],
                    shape: sf::OutlineShape::octagon(),
                },
                renderer,
            ),
            debug_visualizer: sf::DebugVisualizer::new(renderer),
            egui_platform: egui_wp::Platform::new(egui_wp::PlatformDescriptor {
                physical_width: renderer.window_size().width,
                physical_height: renderer.window_size().height,
                scale_factor: renderer.window_scale_factor(),
                font_definitions: egui::FontDefinitions::default(),
                style: egui::Style::default(),
            }),
            egui_pass: egui_wgpu_backend::RenderPass::new(
                &renderer.device,
                renderer.swapchain_format(),
                1,
            ),
            last_egui_output: Default::default(),
            outline_interp: 0.0,
            bvh_vis_active: false,
            bvh_vis_levels: 30,
            island_vis_active: false,
            spawner_circle_r: 0.0,
            time_scale: 1.0,
        }
    }

    fn reset(&mut self) {
        self.physics.reset();
        self.graph.reset();
    }
}

//
// scenes & loading
//

/// The recipes in a scene plus some adjustable parameters.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Scene {
    gravity: [f64; 2],
    spawn_zone: sf::AABB,
    recipes: Vec<Recipe>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81],
            spawn_zone: sf::AABB {
                min: sf::Vec2::new(-5.0, 1.0),
                max: sf::Vec2::new(5.0, 4.0),
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

    pub fn read_from_string(s: &str) -> Result<Self, ron::de::Error> {
        let mut deser = ron::de::Deserializer::from_str(s)?;
        <Self as serde::Deserialize>::deserialize(&mut deser)
    }

    pub fn instantiate(&self, physics: &mut sf::Physics, graph: &sf::Graph) {
        for recipe in &self.recipes {
            recipe.spawn(physics, graph);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_available_scenes() -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    let dir = std::fs::read_dir("./examples/sandbox/scenes")?;
    Ok(dir
        .into_iter()
        .filter_map(|entry| entry.map(|e| e.path()).ok())
        .filter(|p| p.is_file())
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_scene(path: &std::path::Path) -> Option<Scene> {
    let file = std::fs::File::open(path);
    match file {
        Ok(file) => match Scene::read_from_file(file) {
            Ok(scene) => Some(scene),
            Err(err) => {
                eprintln!("Failed to parse file: {}", err);
                None
            }
        },
        Err(err) => {
            eprintln!("Failed to open file: {}", err);
            None
        }
    }
}

// hackery to simulate loading scenes with include_str on the web
// so I don't have to implement downloading stuff at runtime
#[cfg(target_arch = "wasm32")]
fn read_available_scenes() -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    Ok([
        "compound_colliders.ron",
        "minimal.ron",
        "oscillators.ron",
        "ropes.ron",
        "test.ron",
    ]
    .into_iter()
    .map(|s| std::path::PathBuf::from(s))
    .collect())
}

#[cfg(target_arch = "wasm32")]
fn read_scene(path: &std::path::Path) -> Option<Scene> {
    let scene_str = match path.to_str().unwrap() {
        "compound_colliders.ron" => include_str!("scenes/compound_colliders.ron"),
        "minimal.ron" => include_str!("scenes/minimal.ron"),
        "oscillators.ron" => include_str!("scenes/oscillators.ron"),
        "ropes.ron" => include_str!("scenes/ropes.ron"),
        "test.ron" => include_str!("scenes/test.ron"),
        _ => {
            log::error!("Scene not included: {path:?}");
            return None;
        }
    };
    match Scene::read_from_string(scene_str) {
        Ok(scene) => Some(scene),
        Err(err) => {
            log::error!("Failed to parse scene: {err}");
            None
        }
    }
}

//
// State updates
//

impl sf::GameState for State {
    fn init(renderer: &sf::Renderer) -> Self {
        Self::init(renderer)
    }

    fn tick(&mut self, game: &sf::Game) -> Option<()> {
        let mut rng = rand::thread_rng();

        //
        // gui controls
        //

        self.egui_platform.begin_frame();
        let egui_ctx = self.egui_platform.context();

        let mut exit = false;
        let mut reload = false;
        let mut shape_to_spawn: Option<sf::ColliderPolygon> = None;
        egui::Window::new("Controls").show(&egui_ctx, |ui| {
            ui.heading("Load a scene");
            ui.horizontal_wrapped(|ui| {
                for scene_path in &self.scenes_available {
                    if ui
                        .button(
                            scene_path
                                .file_stem()
                                .expect("File with no name?")
                                .to_str()
                                .expect("File with invalid name?"),
                        )
                        .clicked()
                    {
                        if let Some(scene) = read_scene(scene_path) {
                            self.scene = scene;
                            reload = true;
                        }
                    }
                }
            });
            reload |= ui.button("Reload current").clicked();

            ui.separator();
            ui.heading("Spawn objects");
            ui.add(egui::Slider::new(&mut self.spawner_circle_r, 0.0..=1.0).text("Radius"));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Triangle").clicked() {
                    shape_to_spawn = Some(sf::ColliderPolygon::Triangle {
                        outer_r: distr::Uniform::from(0.5..0.8).sample(&mut rng),
                    });
                }
                if ui.button("Rect").clicked() {
                    shape_to_spawn = Some(sf::ColliderPolygon::Rect {
                        hw: distr::Uniform::from(0.4..0.6).sample(&mut rng),
                        hh: distr::Uniform::from(0.3..0.5).sample(&mut rng),
                    });
                }
                if ui.button("Hexagon").clicked() {
                    shape_to_spawn = Some(sf::ColliderPolygon::Hexagon {
                        outer_r: distr::Uniform::from(0.4..0.7).sample(&mut rng),
                    });
                }
            });
            if self.spawner_circle_r > 0.0 {
                ui.horizontal(|ui| {
                    if ui.button("Circle").clicked() {
                        shape_to_spawn = Some(sf::ColliderPolygon::Point);
                    }
                    if ui.button("Capsule").clicked() {
                        shape_to_spawn = Some(sf::ColliderPolygon::LineSegment {
                            hl: distr::Uniform::from(0.3..0.5).sample(&mut rng),
                        });
                    }
                });
            }

            ui.separator();
            ui.heading("Other controls");
            match self.state {
                StateEnum::Playing => {
                    if ui.button("Pause").clicked() {
                        self.state = StateEnum::Paused;
                    }
                }
                StateEnum::Paused => {
                    if ui.button("Unpause").clicked() {
                        self.state = StateEnum::Playing;
                    }
                }
            }
            ui.add(
                egui::Slider::new(
                    &mut self.camera.transform.scale,
                    self.camera_ctl.min_zoom_out..=self.camera_ctl.max_zoom_out,
                )
                .text("Camera zoom out"),
            );
            ui.add(
                egui::Slider::new(&mut self.physics.consts.substeps, 1..=15)
                    .text("Physics substeps"),
            );
            ui.add(egui::Slider::new(&mut self.time_scale, 0.05..=2.0).text("Time scale"));

            ui.separator();
            ui.heading("Visuals");
            ui.checkbox(&mut self.bvh_vis_active, "Display BVH");
            if self.bvh_vis_active {
                ui.add(
                    egui::Slider::new(&mut self.bvh_vis_levels, 0..=50).text("Tree levels to show"),
                );
            }
            ui.checkbox(&mut self.island_vis_active, "Display islands");
            ui.add(
                egui::Slider::new(&mut self.outline_renderer.params.thickness, 0..=100)
                    .text("Outline thickness"),
            );
            ui.add(egui::Slider::new(&mut self.outline_interp, 0.0..=1.0).text("Outline shape"));
            self.outline_renderer.params.shape =
                sf::OutlineShape::octagon().lerp(self.outline_interp, sf::OutlineShape::circle());
            ui.separator();
            if ui.button("exit").clicked() {
                exit = true;
            }
        });
        if exit {
            return None;
        }
        if reload {
            self.reset();
            self.scene.instantiate(&mut self.physics, &self.graph);
        }

        self.last_egui_output = self.egui_platform.end_frame(Some(&game.window));

        // mouse controls

        self.mouse_grabber
            .update(&game.input, &self.camera, &mut self.physics, &self.graph);
        self.camera_ctl.update(&mut self.camera, &game.input);

        // spawn stuff even when paused

        let zone = self.scene.spawn_zone;
        let random_pos = || {
            let mut rng = rand::thread_rng();
            sf::Vec2::new(
                distr::Uniform::from(zone.min.x..zone.max.x).sample(&mut rng),
                distr::Uniform::from(zone.min.y..zone.max.y).sample(&mut rng),
            )
        };
        let random_angle =
            || sf::Angle::Deg(distr::Uniform::from(0.0..360.0).sample(&mut rand::thread_rng()));

        if let Some(polygon) = shape_to_spawn {
            Recipe::GenericBody {
                pose: sf::PoseBuilder::new()
                    .with_position(random_pos())
                    .with_rotation(random_angle())
                    .build(),
                colliders: vec![sf::ColliderShape {
                    polygon,
                    circle_r: self.spawner_circle_r,
                }
                .into()],
            }
            .spawn(&mut self.physics, &self.graph);
        }

        match (&self.state, game.input.button(sf::Key::Space.into())) {
            //
            // Playing or stepping manually
            //
            (StateEnum::Playing, _) | (StateEnum::Paused, true) => {
                if game.input.button(sf::Key::P.into()) {
                    self.state = StateEnum::Paused;
                    return Some(());
                }

                let grav = sf::forcefield::Gravity(self.scene.gravity.into());
                self.physics.tick(
                    game.dt_fixed,
                    Some(self.time_scale),
                    &grav,
                    self.graph.get_layer_bundle(),
                );
                self.player
                    .tick(&game.input, &self.physics, &mut self.graph);

                Some(())
            }
            //
            // Paused
            //
            (StateEnum::Paused, false) => {
                if game.input.button(sf::Key::P.into()) {
                    self.state = StateEnum::Playing;
                    return Some(());
                }

                Some(())
            }
        }
    }

    fn draw(&mut self, renderer: &mut sf::Renderer, _dt: f32) {
        let window_scale_factor = renderer.window_scale_factor();

        let mut ctx = renderer.draw_to_window();
        ctx.clear(wgpu::Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        });

        if self.bvh_vis_active {
            self.debug_visualizer.draw_bvh(
                self.bvh_vis_levels,
                &self.physics,
                &self.camera,
                &mut ctx,
            );
        }
        if self.island_vis_active {
            self.debug_visualizer.draw_islands(
                &self.physics,
                &self.camera,
                &mut ctx,
                self.graph.get_layer_bundle(),
            );
        }

        self.mesh_renderer
            .draw(&self.camera, &mut ctx, self.graph.get_layer_bundle());

        ctx.submit();

        self.outline_renderer.draw(renderer);

        let mut ctx = renderer.draw_to_window();

        let paint_jobs = self
            .egui_platform
            .context()
            .tessellate(self.last_egui_output.shapes.clone());
        let screen_desc = egui_wgpu_backend::ScreenDescriptor {
            physical_width: ctx.target_size.0,
            physical_height: ctx.target_size.1,
            scale_factor: window_scale_factor as f32,
        };
        self.egui_pass
            .add_textures(ctx.device, ctx.queue, &self.last_egui_output.textures_delta)
            .expect("Failed to add egui textures");
        self.egui_pass
            .update_buffers(ctx.device, ctx.queue, &paint_jobs, &screen_desc);
        self.egui_pass
            .execute(
                &mut ctx.encoder.0,
                ctx.target.resolve_target.unwrap(),
                &paint_jobs,
                &screen_desc,
                None,
            )
            .expect("failed to draw egui");

        ctx.submit();

        renderer.present_frame();
    }
}
