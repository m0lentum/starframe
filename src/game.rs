//! Tools for creating a window and starting a managed game loop.

use instant::{Duration, Instant};

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use crate::graphics::Renderer;

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
    fn init(renderer: &Renderer) -> Self;
    /// Advance the game forward by a timestep of `Game::dt_fixed` seconds. Return None to exit the game.
    fn tick(&mut self, game: &Game) -> Option<()>;
    /// Render the game onto the screen. `dt` is the time in seconds since last draw.
    fn draw(&mut self, renderer: &mut crate::graphics::Renderer, dt: f32);
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
    /// A global input cache that is automatically updated once per tick.
    pub input: crate::Input,
    /// A renderer that can draw to the game's window.
    pub renderer: crate::graphics::Renderer,
    nanos_per_frame: u128,
    pub dt_fixed: f64,
}
impl Game {
    pub fn run<State: GameState>(params: GameParams<State>) {
        let events: EventLoop<()> = EventLoop::new();
        let window = params
            .window
            .build(&events)
            .expect("Failed to create window");
        #[cfg(not(target_arch = "wasm32"))]
        {
            futures::executor::block_on(Self::run_async(
                params.fps,
                params.on_event,
                events,
                window,
            ));
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
            ));
        }
    }

    async fn run_async<State: GameState>(
        fps: u32,
        on_event: fn(&mut State, &Event<()>),
        events: EventLoop<()>,
        window: Window,
    ) {
        let _tracy_client = tracy_client::Client::start();

        //
        // init
        //

        let renderer = Renderer::init(window).await;

        let mut game = Game {
            input: crate::Input::new(renderer.window_size().into()),
            renderer,
            nanos_per_frame: 1_000_000_000 / u128::from(fps),
            dt_fixed: 1.0 / fps as f64,
        };
        let mut state = State::init(&game.renderer);

        //
        // loop
        //

        let mut frame_start_t = Instant::now();
        let mut acc = 0;
        events.run(move |event, _, control_flow| {
            (on_event)(&mut state, &event);

            *control_flow = ControlFlow::Poll;

            match event {
                Event::MainEventsCleared => {
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

                            if state.tick(&game).is_none() {
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                            game.input.tick();
                            acc -= game.nanos_per_frame;
                        }

                        {
                            let _draw_span = tracy_client::span!("draw");

                            state.draw(&mut game.renderer, game.dt_fixed as f32);
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
                            *control_flow = ControlFlow::WaitUntil(wait_until_t);
                        } else {
                            // we're at or almost at the next frame threshold,
                            // spin for accurate timing
                            *control_flow = ControlFlow::Poll;
                        }
                    }
                }
                Event::WindowEvent { event, .. } => {
                    game.input.track_window_event(&event);
                    match event {
                        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                        WindowEvent::Resized(new_size) => {
                            game.renderer.resize_swap_chain(new_size);
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        });
    }
}
