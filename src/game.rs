//! Tools for creating a window and starting a managed game loop.

use instant::{Duration, Instant};

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use crate::{
    graphics::renderer::RendererInitError,
    physics::{hecs_sync::HecsSyncManager, ForceField, PhysicsWorld},
};

// time snapping technique from Tyler Glaiel's blog post
// https://medium.com/@tglaiel/how-to-make-your-game-run-at-60fps-24c61210fe75
const NANOS_120FPS: u128 = 1_000_000_000 / 120;
const NANOS_60FPS: u128 = 1_000_000_000 / 60;
const NANOS_30FPS: u128 = 1_000_000_000 / 30;
const NANOS_20FPS: u128 = 1_000_000_000 / 20;
const NANOS_15FPS: u128 = 1_000_000_000 / 15;
const SNAP_THRESHOLD: u128 = 200_000;

const MAX_ACC_VALUE: u128 = 1_000_000_000 / 8;

// use winit's ControlFlow::WaitUntil to wait for the next frame,
// but schedule it to SPIN_DURATION before the actual frame time.
// spin the rest of the time for accurate timing.
const SPIN_DURATION: u128 = 200_000;

fn should_snap(dt: u128, target: u128) -> bool {
    if dt < target {
        target - dt < SNAP_THRESHOLD
    } else {
        dt - target < SNAP_THRESHOLD
    }
}

/// Implement this on your main state type to have [`Game`][self::Game]
/// manage the game loop for you.
pub trait GameState: Sized + 'static {
    /// Create the initial state.
    ///
    /// This is called immediately after the game loop is started.
    /// It is done this way due to async functions involved in the creation of the renderer,
    /// which is easiest to handle within a single encompassing async function (especially in wasm).
    fn init(game: &mut Game) -> Self;
    /// Advance the game forward by a timestep of `Game::dt_fixed` seconds. Return None to exit the game.
    fn tick(&mut self, game: &mut Game) -> Option<()>;
    /// Render the game onto the screen. `dt` is the time in seconds since last draw.
    fn draw(&mut self, game: &mut Game, dt: f32);
}

pub struct GameParams<State: GameState> {
    pub window: WindowBuilder,
    pub fps: u32,
    pub on_event: fn(&mut State, &Event<()>),
}

impl<State: GameState> Default for GameParams<State> {
    fn default() -> Self {
        Self {
            window: WindowBuilder::new()
                .with_title("starframe")
                .with_inner_size(winit::dpi::LogicalSize {
                    width: 1280.0,
                    height: 720.0,
                }),
            fps: 60,
            on_event: |_, _| {},
        }
    }
}

/// A Game manages the global resources a game needs like a window and a graphics renderer
/// and handles timing of the game loop.
pub struct Game {
    /// Current state of input devices.
    pub input: crate::Input,
    /// A manager that handles the rendering context and GPU resources.
    pub renderer: crate::Renderer,
    /// Interface for loading and rendering graphics assets.
    pub graphics: crate::GraphicsManager,
    /// Main ECS world of the game.
    pub world: hecs::World,
    /// Main physics world of the game.
    pub physics: PhysicsWorld,
    /// Handler for interactions between the ECS world and the physics world.
    pub hecs_sync: HecsSyncManager,
    /// Fixed delta-time between frames.
    pub dt_fixed: f64,
    /// Duration of a frame in nanoseconds.
    nanos_per_frame: u128,
}

/// An error that occurred during in the initialization
/// of a game window and renderer.
#[derive(Debug, thiserror::Error)]
pub enum GameError {
    #[error("Error with winit event loop")]
    EventLoopError(#[from] winit::error::EventLoopError),
    #[error("Failed to create a window")]
    WindowOSError(#[from] winit::error::OsError),
    #[error("Failed to initialize wgpu renderer")]
    RendererInitError(#[from] RendererInitError),
}

impl Game {
    pub fn run<State: GameState>(params: GameParams<State>) -> Result<(), GameError> {
        let events: EventLoop<()> = EventLoop::new()?;
        let window = params.window.build(&events)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            futures::executor::block_on(Self::run_async(
                params.fps,
                params.on_event,
                events,
                window,
            ))?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init().expect("Failed to initialize console logger");
            use winit::platform::web::WindowExtWebSys;
            let canvas = web_sys::Element::from(window.canvas());
            web_sys::window()
                .and_then(|win| win.document())
                // made-up convention for putting the game in a specific spot in the DOM
                .and_then(|doc| match doc.get_element_by_id("starframe-game") {
                    Some(parent) => parent.append_child(&canvas).ok(),
                    None => doc.body().and_then(|body| body.append_child(&canvas).ok()),
                })
                .expect("couldn't append canvas to document body");
            wasm_bindgen_futures::spawn_local(Self::run_async(
                params.fps,
                params.on_event,
                events,
                window,
            ))?;
        }
        Ok(())
    }

    async fn run_async<State: GameState>(
        fps: u32,
        on_event: fn(&mut State, &Event<()>),
        events: EventLoop<()>,
        window: Window,
    ) -> Result<(), GameError> {
        let _tracy_client = tracy_client::Client::start();

        //
        // init
        //

        let renderer = crate::Renderer::init(window).await?;

        let mut game = Game {
            input: crate::Input::new(),
            graphics: crate::GraphicsManager::new(),
            renderer,
            world: hecs::World::new(),
            physics: PhysicsWorld::new(
                crate::physics::TuningConstants::default(),
                crate::CollisionMaskMatrix::default(),
            ),
            hecs_sync: HecsSyncManager::new_autosync(crate::HecsSyncOptions::both_ways()),
            nanos_per_frame: 1_000_000_000 / u128::from(fps),
            dt_fixed: 1.0 / fps as f64,
        };
        let mut state = State::init(&mut game);

        //
        // loop
        //

        let mut frame_start_t = Instant::now();
        let mut acc = 0;
        events.run(move |event, elwt| {
            (on_event)(&mut state, &event);

            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::AboutToWait => {
                    // if vsynced, pretend frame timing is exact (see blog post mentioned above)
                    let mut dt_nanos = frame_start_t.elapsed().as_nanos();
                    if should_snap(dt_nanos, NANOS_120FPS) {
                        dt_nanos = NANOS_120FPS;
                        acc = 0;
                    } else if should_snap(dt_nanos, NANOS_60FPS) {
                        dt_nanos = NANOS_60FPS;
                        acc = 0;
                    } else if should_snap(dt_nanos, NANOS_30FPS) {
                        dt_nanos = NANOS_30FPS;
                        acc = 0;
                    } else if should_snap(dt_nanos, NANOS_20FPS) {
                        dt_nanos = NANOS_20FPS;
                        acc = 0;
                    } else if should_snap(dt_nanos, NANOS_15FPS) {
                        dt_nanos = NANOS_15FPS;
                        acc = 0;
                    }

                    // if we're going too fast just wait, otherwise run as many ticks
                    // as have been passed since last update and draw once
                    if dt_nanos >= game.nanos_per_frame - acc {
                        frame_start_t = Instant::now();

                        acc += dt_nanos;
                        // limit acc to prevent spiral of death
                        if acc > MAX_ACC_VALUE {
                            acc = MAX_ACC_VALUE;
                        }

                        while acc >= game.nanos_per_frame {
                            let _frame = tracy_client::non_continuous_frame!("tick");

                            if state.tick(&mut game).is_none() {
                                elwt.exit();
                                return;
                            }
                            game.input.tick();
                            acc -= game.nanos_per_frame;
                        }

                        {
                            let _draw_span = tracy_client::span!("draw");

                            let dt = game.dt_fixed as f32;
                            state.draw(&mut game, dt);
                        }

                        tracy_client::frame_mark();

                        let nanos_this_frame = frame_start_t.elapsed().as_nanos();
                        // acc represents drift from the perfect tick timing that we should correct by
                        let target_frame_duration = game.nanos_per_frame - acc;
                        // sleep till next frame if we have time to kill
                        if nanos_this_frame < target_frame_duration
                            && target_frame_duration > SPIN_DURATION
                            && nanos_this_frame < target_frame_duration - SPIN_DURATION
                        {
                            let wait_until_t = frame_start_t
                                + Duration::from_nanos(
                                    (target_frame_duration - SPIN_DURATION) as u64,
                                );
                            elwt.set_control_flow(ControlFlow::WaitUntil(wait_until_t));
                        } else {
                            // we're at or almost at the next frame threshold,
                            // spin for accurate timing
                            elwt.set_control_flow(ControlFlow::Poll);
                        }
                    }
                }
                Event::WindowEvent { event, .. } => {
                    game.input.track_window_event(&event);
                    match event {
                        WindowEvent::CloseRequested => {
                            elwt.exit();
                        }
                        WindowEvent::Resized(new_size) => {
                            game.renderer.resize_swap_chain(new_size);
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        })?;

        Ok(())
    }

    /// Step the game's physics world forward in time by a frame.
    ///
    /// Convenience method that calls [`HecsSyncManager::sync_hecs_to_physics`],
    /// [`PhysicsWorld::tick`], and [`HecsSyncManager::sync_physics_to_hecs`].
    pub fn physics_tick(&mut self, ff: &impl ForceField, time_scale: Option<f64>) {
        self.hecs_sync
            .sync_hecs_to_physics(&mut self.physics, &mut self.world);
        self.physics.tick(self.dt_fixed, time_scale, ff);
        self.hecs_sync
            .sync_physics_to_hecs(&self.physics, &mut self.world);
    }

    /// Clear all state stored in the game struct,
    /// namely `self.graphics`, `self.world`, `self.physics` and `self.hecs_sync`.
    pub fn clear_state(&mut self) {
        self.graphics.clear();
        self.world.clear();
        self.physics.clear();
        self.hecs_sync.clear();
    }
}
