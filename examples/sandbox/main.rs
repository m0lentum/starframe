#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);

use rand::{distributions as distr, distributions::Distribution};

use egui_winit_platform as egui_wp;

use starframe::{
    game::{self, Game},
    graph::{new_graph, Graph},
    graphics as gx,
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
    use winit::platform::unix::WindowBuilderExtUnix;
    let game = Game::init(
        60,
        winit::window::WindowBuilder::new()
            .with_title("starframe test")
            // X11 class I use for a window manager rule to make the game window floating
            .with_class("game".into(), "game".into())
            .with_inner_size(winit::dpi::LogicalSize {
                width: 1280.0,
                height: 720.0,
            }),
    );
    let state = State::init(&game.renderer);
    game.run(state, Some(handle_event));
}

fn handle_event(state: &mut State, evt: &winit::event::Event<()>) {
    state.egui_platform.handle_event(evt);
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
    graph: Graph,
    // gameplay
    player: player::PlayerController,
    mouse_mode: MouseMode,
    mouse_grabber: MouseGrabber,
    physics: phys::Physics,
    // graphics
    camera: gx::camera::MouseDragCamera,
    mesh_renderer: gx::MeshRenderer,
    outline_renderer: gx::OutlineRenderer,
    debug_visualizer: gx::DebugVisualizer,
    // egui stuff
    egui_platform: egui_wp::Platform,
    egui_pass: egui_wgpu_backend::RenderPass,
    last_egui_paint_cmds: Vec<egui::epaint::ClippedShape>,
    // UI states
    outline_interp: f32,
    grid_vis_active: bool,
    island_vis_active: bool,
    spawner_circle_r: f64,
}
impl State {
    fn init(renderer: &gx::Renderer) -> Self {
        State {
            scenes_available: read_available_scenes().expect("Failed to read scenes directory"),
            scene: Scene::default(),
            state: StateEnum::Playing,
            graph: new_graph! {
                // starframe types
                m::Pose,
                phys::Body,
                phys::Collider,
                phys::rope::Rope,
                gx::Mesh,
                // our types
                player::Player,
            },
            player: player::PlayerController::new(),
            mouse_mode: MouseMode::Grab,
            mouse_grabber: MouseGrabber::new(),
            physics: phys::Physics::new(
                phys::TuningConstants {
                    // uncomment to help testing collision detection and such
                    // substeps: 1,
                    ..Default::default()
                },
                phys::collision::HGridParams {
                    approx_bounds: phys::collision::AABB {
                        min: m::Vec2::new(-40.0, -15.0),
                        max: m::Vec2::new(40.0, 25.0),
                    },
                    lowest_spacing: 0.5,
                    level_count: 2,
                    spacing_ratio: 3,
                    initial_capacity: 600,
                },
                phys::collision::MaskMatrix::default(),
            ),
            camera: gx::camera::MouseDragCamera::new(
                gx::camera::ScalingStrategy::ConstantDisplayArea {
                    width: 20.0,
                    height: 10.0,
                },
            ),
            mesh_renderer: gx::MeshRenderer::new(renderer),
            outline_renderer: gx::OutlineRenderer::new(
                gx::OutlineParams {
                    thickness: 15,
                    shape: gx::OutlineShape::octagon(),
                },
                renderer,
            ),
            debug_visualizer: gx::DebugVisualizer::new(renderer),
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
            last_egui_paint_cmds: vec![],
            outline_interp: 0.0,
            grid_vis_active: false,
            island_vis_active: false,
            spawner_circle_r: 0.0,
        }
    }

    fn reset(&mut self) {
        self.physics.reset();
        self.graph.reset();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseMode {
    /// Grab objects with the mouse
    Grab,
    /// Move the camera with the mouse
    Camera,
}

//
// scenes & loading
//

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

    pub fn instantiate(&self, physics: &mut phys::Physics, graph: &Graph) {
        for recipe in &self.recipes {
            recipe.spawn(physics, graph);
        }
    }
}

fn read_available_scenes() -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    let dir = std::fs::read_dir("./examples/sandbox/scenes")?;
    Ok(dir
        .into_iter()
        .filter_map(|entry| entry.map(|e| e.path()).ok())
        .filter(|p| p.is_file())
        .collect())
}

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

//
// State updates
//

impl game::GameState for State {
    fn tick(&mut self, dt: f64, game: &Game) -> Option<()> {
        let mut rng = rand::thread_rng();

        //
        // gui controls
        //

        self.egui_platform.begin_frame();
        let egui_ctx = self.egui_platform.context();

        let mut exit = false;
        let mut reload = false;
        let mut shape_to_spawn: Option<phys::ColliderPolygon> = None;
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
                    shape_to_spawn = Some(phys::ColliderPolygon::Triangle {
                        outer_r: distr::Uniform::from(0.5..0.8).sample(&mut rng),
                    });
                }
                if ui.button("Rect").clicked() {
                    shape_to_spawn = Some(phys::ColliderPolygon::Rect {
                        hw: distr::Uniform::from(0.4..0.6).sample(&mut rng),
                        hh: distr::Uniform::from(0.3..0.5).sample(&mut rng),
                    });
                }
            });
            if self.spawner_circle_r > 0.0 {
                ui.horizontal(|ui| {
                    if ui.button("Circle").clicked() {
                        shape_to_spawn = Some(phys::ColliderPolygon::Point);
                    }
                    if ui.button("Capsule").clicked() {
                        shape_to_spawn = Some(phys::ColliderPolygon::LineSegment {
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
                    &mut self.camera.pose.scale,
                    self.camera.min_zoom_out..=self.camera.max_zoom_out,
                )
                .text("Camera zoom out"),
            );
            ui.horizontal(|ui| {
                ui.label("Mouse mode:");
                ui.selectable_value(&mut self.mouse_mode, MouseMode::Grab, "Grab");
                ui.selectable_value(&mut self.mouse_mode, MouseMode::Camera, "Camera");
            });
            ui.add(
                egui::Slider::new(&mut self.physics.consts.substeps, 1..=15)
                    .text("Physics substeps"),
            );

            ui.separator();
            ui.heading("Visuals");
            ui.checkbox(&mut self.grid_vis_active, "Display grid");
            ui.checkbox(&mut self.island_vis_active, "Display islands");
            ui.add(
                egui::Slider::new(&mut self.outline_renderer.params.thickness, 0..=30)
                    .text("Outline thickness"),
            );
            ui.add(egui::Slider::new(&mut self.outline_interp, 0.0..=1.0).text("Outline shape"));
            self.outline_renderer.params.shape =
                gx::OutlineShape::octagon().lerp(self.outline_interp, gx::OutlineShape::circle());
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

        let (_output, paint_commands) = self.egui_platform.end_frame(Some(&game.window));
        self.last_egui_paint_cmds = paint_commands;

        // mouse controls

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

        // spawn stuff even when paused

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

        if let Some(polygon) = shape_to_spawn {
            Recipe::GenericObject {
                pose: m::PoseBuilder::new()
                    .with_position(random_pos())
                    .with_rotation(random_angle()),
                shape: phys::ColliderShape {
                    polygon,
                    circle_r: self.spawner_circle_r,
                },
                is_static: false,
            }
            .spawn(&mut self.physics, &self.graph);
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
                    self.physics.tick(dt, &grav, self.graph.get_layer_bundle());
                }
                {
                    self.player
                        .tick(&game.input, &self.physics, &mut self.graph);
                }

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
        let window_scale_factor = renderer.window_scale_factor();

        self.outline_renderer.prepare(renderer);
        self.outline_renderer
            .init_meshes(&self.camera, renderer, self.graph.get_layer_bundle());
        self.outline_renderer.compute(renderer);

        let mut ctx = renderer.draw_to_window();
        ctx.clear(wgpu::Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        });

        self.outline_renderer.draw(&mut ctx);

        if self.grid_vis_active {
            self.debug_visualizer
                .draw_spatial_index(&self.physics, &self.camera, &mut ctx);
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

        let paint_jobs = self
            .egui_platform
            .context()
            .tessellate(self.last_egui_paint_cmds.clone());
        let screen_desc = egui_wgpu_backend::ScreenDescriptor {
            physical_width: ctx.target_size.0,
            physical_height: ctx.target_size.1,
            scale_factor: window_scale_factor as f32,
        };
        self.egui_pass.update_texture(
            ctx.device,
            ctx.queue,
            &self.egui_platform.context().font_image(),
        );
        self.egui_pass.update_user_textures(ctx.device, ctx.queue);
        self.egui_pass
            .update_buffers(ctx.device, ctx.queue, &paint_jobs, &screen_desc);
        self.egui_pass
            .execute(
                &mut ctx.encoder,
                ctx.target.view(),
                &paint_jobs,
                &screen_desc,
                None,
            )
            .expect("failed to draw egui");

        ctx.submit();
    }
}
