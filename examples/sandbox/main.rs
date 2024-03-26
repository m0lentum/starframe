#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

use rand::{distributions as distr, distributions::Distribution};

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
            if let winit::event::Event::WindowEvent { event, .. } = evt {
                let egui_resp = state.egui_state.on_window_event(&state.egui_context, event);
                if egui_resp.consumed {
                    // TODO: don't propagate the event
                }
            }
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
    world: sf::hecs::World,
    physics: sf::PhysicsWorld,
    hecs_sync: sf::HecsSyncManager,
    // gameplay
    mouse_grabber: MouseGrabber,
    // graphics
    graphics: sf::GraphicsManager,
    camera: sf::Camera,
    light: sf::DirectionalLight,
    light_rotating: bool,
    camera_ctl: sf::MouseDragCameraController,
    mesh_renderer: sf::MeshRenderer,
    outline_renderer: sf::OutlineRenderer,
    debug_visualizer: sf::DebugVisualizer,
    // egui stuff
    egui_context: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::renderer::Renderer,
    last_egui_output: egui::FullOutput,
    // UI states
    outline_interp: f32,
    bvh_vis_active: bool,
    bvh_vis_levels: usize,
    island_vis_active: bool,
    spawner_circle_r: f64,
    spawner_obj_count: usize,
    time_scale: f64,
}
impl State {
    fn init(game: &mut sf::Game) -> Self {
        let mut graphics = sf::GraphicsManager::new(&game.renderer);

        graphics
            .load_gltf(&game.renderer, "examples/sandbox/assets/library.glb")
            .expect("Failed to load shared assets");

        player::controller::upload_meshes(&game.renderer, &mut graphics);

        let mesh_renderer = sf::MeshRenderer::new(&game.renderer, &graphics);

        let egui_context = egui::Context::default();
        let viewport_id = egui_context.viewport_id();
        State {
            scenes_available: read_available_scenes().expect("Failed to read scenes directory"),
            scene: Scene::default(),
            state: StateEnum::Playing,
            world: sf::hecs::World::new(),
            physics: sf::PhysicsWorld::new(
                sf::physics::TuningConstants::default(),
                sf::CollisionMaskMatrix::default(),
            ),
            hecs_sync: sf::HecsSyncManager::new_autosync(sf::HecsSyncOptions::both_ways()),
            mouse_grabber: MouseGrabber::new(),
            graphics,
            camera: sf::Camera::default(),
            light: sf::DirectionalLight {
                direct_color: [1.0, 0.949, 0.8],
                ambient_color: [0.686, 0.875, 0.918],
                direction: sf::uv::Vec3::new(-1.0, -3.0, 2.0),
            },
            light_rotating: false,
            camera_ctl: sf::MouseDragCameraController {
                activate_button: sf::MouseButton::Middle.into(),
                reset_button: Some(sf::Key::R.into()),
                ..Default::default()
            },
            mesh_renderer,
            outline_renderer: sf::OutlineRenderer::new(
                sf::OutlineParams {
                    thickness: 10,
                    color: [0.0, 0.0, 0.0, 1.0],
                    shape: sf::OutlineShape::octagon(),
                },
                &game.renderer,
            ),
            debug_visualizer: sf::DebugVisualizer::new(&game.renderer),
            egui_context,
            egui_state: egui_winit::State::new(viewport_id, &game.renderer.window, None, None),
            egui_renderer: egui_wgpu::Renderer::new(
                &game.renderer.device,
                game.renderer.swapchain_format(),
                Some(game.renderer.window_depth_buffer.texture.format()),
                game.renderer.msaa_samples(),
            ),
            last_egui_output: Default::default(),
            outline_interp: 0.0,
            bvh_vis_active: false,
            bvh_vis_levels: 30,
            island_vis_active: false,
            spawner_circle_r: 0.0,
            spawner_obj_count: 1,
            time_scale: 1.0,
        }
    }

    fn reset(&mut self) {
        self.physics.clear();
        self.world.clear();
        self.hecs_sync.clear();
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

    pub fn instantiate(
        &self,
        physics: &mut sf::PhysicsWorld,
        world: &mut hecs::World,
        hecs_sync: &mut sf::HecsSyncManager,
        renderer: &sf::Renderer,
        graphics: &mut sf::GraphicsManager,
    ) {
        for recipe in &self.recipes {
            recipe.spawn(physics, world, hecs_sync, renderer, graphics);
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
    fn init(game: &mut sf::Game) -> Self {
        Self::init(game)
    }

    fn tick(&mut self, game: &sf::Game) -> Option<()> {
        let mut rng = rand::thread_rng();

        //
        // gui controls
        //

        let egui_input = self.egui_state.take_egui_input(&game.renderer.window);
        self.egui_context.begin_frame(egui_input);

        let mut exit = false;
        let mut reload = false;
        let mut step_one = false;
        let mut shape_to_spawn: Option<sf::ColliderPolygon> = None;
        egui::Window::new("Controls").show(&self.egui_context, |ui| {
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
            ui.add(
                egui::Slider::new(&mut self.spawner_obj_count, 1..=50)
                    .text("Number of objects to spawn"),
            );
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
            ui.heading("Light");
            ui.horizontal(|ui| {
                ui.color_edit_button_rgb(&mut self.light.direct_color);
                ui.label("Direct light color");
            });
            ui.horizontal(|ui| {
                ui.color_edit_button_rgb(&mut self.light.ambient_color);
                ui.label("Ambient light color");
            });
            ui.add(egui::Slider::new(&mut self.light.direction.x, -5.0..=5.0).text("Direction x"));
            ui.add(egui::Slider::new(&mut self.light.direction.y, -5.0..=5.0).text("Direction y"));
            ui.checkbox(&mut self.light_rotating, "Spin");

            ui.separator();
            ui.heading("Other controls");
            ui.horizontal(|ui| {
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
                if let StateEnum::Paused = self.state {
                    if ui.button("Step a frame").clicked() {
                        step_one = true;
                    }
                }
            });
            ui.add(
                egui::Slider::new(
                    &mut self.camera.zoom,
                    self.camera_ctl.min_zoom..=self.camera_ctl.max_zoom,
                )
                .text("Camera zoom"),
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
            self.scene.instantiate(
                &mut self.physics,
                &mut self.world,
                &mut self.hecs_sync,
                &game.renderer,
                &mut self.graphics,
            );
        }

        self.last_egui_output = self.egui_context.end_frame();

        // mouse controls

        self.mouse_grabber
            .update(&game.input, &self.camera, &mut self.physics);
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

        for _ in 0..self.spawner_obj_count {
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
                .spawn(
                    &mut self.physics,
                    &mut self.world,
                    &mut self.hecs_sync,
                    &game.renderer,
                    &mut self.graphics,
                );
            }
        }

        match (&self.state, step_one) {
            //
            // Playing or stepping manually
            //
            (StateEnum::Playing, _) | (StateEnum::Paused, true) => {
                if game.input.button(sf::Key::P.into()) {
                    self.state = StateEnum::Paused;
                    return Some(());
                }

                let grav = sf::forcefield::Gravity(self.scene.gravity.into());
                self.hecs_sync
                    .sync_hecs_to_physics(&mut self.physics, &mut self.world);
                self.physics
                    .tick(game.dt_fixed, Some(self.time_scale), &grav);
                self.hecs_sync
                    .sync_physics_to_hecs(&self.physics, &mut self.world);
                player::controller::tick(
                    &game.input,
                    &mut self.physics,
                    &self.graphics,
                    &mut self.world,
                );

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

    fn draw(&mut self, renderer: &mut sf::Renderer, dt: f32) {
        let mut ctx = renderer.draw_to_window();
        ctx.clear(sf::wgpu::Color {
            r: 0.00802,
            g: 0.0137,
            b: 0.02732,
            a: 1.0,
        });

        if matches!(self.state, StateEnum::Playing) {
            self.graphics.update_animations(dt);
        }

        if self.bvh_vis_active {
            self.debug_visualizer.draw_bvh(
                self.bvh_vis_levels,
                &self.physics,
                &self.camera,
                &mut ctx,
            );
        }
        if self.island_vis_active {
            self.debug_visualizer
                .draw_islands(&self.physics, &self.camera, &mut ctx);
        }

        if self.light_rotating {
            sf::uv::Rotor3::from_rotation_xy(0.02).rotate_vec(&mut self.light.direction);
        }

        self.mesh_renderer.draw(
            &mut self.graphics,
            &self.camera,
            self.light,
            &mut ctx,
            &mut self.world,
        );

        ctx.submit();

        self.outline_renderer.draw(renderer);

        let mut ctx = renderer.draw_to_window();

        let paint_jobs = self.egui_context.tessellate(
            self.last_egui_output.shapes.clone(),
            self.egui_context.pixels_per_point(),
        );
        self.egui_state.handle_platform_output(
            ctx.window,
            &self.egui_context,
            self.last_egui_output.platform_output.clone(),
        );

        for (tex_id, img_delta) in &self.last_egui_output.textures_delta.set {
            self.egui_renderer
                .update_texture(ctx.device, ctx.queue, *tex_id, img_delta);
        }

        for tex_id in &self.last_egui_output.textures_delta.free {
            self.egui_renderer.free_texture(tex_id);
        }

        let screen_desc = egui_wgpu::renderer::ScreenDescriptor {
            size_in_pixels: [ctx.target_size.0, ctx.target_size.1],
            pixels_per_point: self.egui_context.pixels_per_point(),
        };
        self.egui_renderer.update_buffers(
            ctx.device,
            ctx.queue,
            &mut ctx.encoder.0,
            &paint_jobs,
            &screen_desc,
        );

        let mut pass = ctx.pass(Some("egui"));
        self.egui_renderer
            .render(&mut pass, &paint_jobs, &screen_desc);
        drop(pass);

        ctx.submit();

        renderer.present_frame();
    }
}
