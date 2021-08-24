//! Tools for creating a window and starting a managed game loop.

use std::time::{Duration, Instant};

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
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

fn should_snap(dt: u128, target: u128) -> bool {
    if dt < target {
        target - dt < SNAP_THRESHOLD
    } else {
        dt - target < SNAP_THRESHOLD
    }
}

/// A Game manages the global resources a game needs like a window and a graphics renderer
/// and handles timing of the game loop.
///
/// # Example
/// ```
/// # use starframe::{game::{Game, GameState}, graphics::Renderer};
/// # struct MyState;
/// # impl MyState {
/// #    fn init(_: &Renderer) -> Self { Self }
/// # }
/// # impl GameState for MyState {
/// #   fn tick(&mut self, dt: f64, game: &Game) -> Option<()> { None }
/// #   fn draw(&mut self, renderer: &mut starframe::graphics::Renderer) {}
/// # }
/// let game = Game::init(60, winit::window::WindowBuilder::new());
/// let state = MyState::init(&game.renderer);
/// game.run(state);
/// ```
pub struct Game {
    /// A global input cache that is automatically updated once per tick.
    pub input: crate::InputCache,
    /// The window the game draws to.
    pub window: Window,
    /// A renderer that can draw to the game's window.
    pub renderer: crate::graphics::Renderer,
    nanos_per_frame: u128,
    dt_fixed: f64,
    // Winit event loop. In an option because we need to take it out in `run`
    // to avoid lifetime problems with self.
    events: Option<EventLoop<()>>,
}
impl Game {
    /// Create the resources you need for a game.
    ///
    /// This does not immediately start the game, since you may want to
    /// use e.g. the renderer to initialize some resources first.
    pub fn init(fps: u32, window_b: WindowBuilder) -> Self {
        let events: EventLoop<()> = EventLoop::new();
        let window = window_b.build(&events).expect("Failed to create window");
        let renderer = futures::executor::block_on(crate::graphics::Renderer::init(&window));
        Game {
            input: crate::InputCache::new(),
            window,
            renderer,
            nanos_per_frame: 1_000_000_000 / u128::from(fps),
            dt_fixed: 1.0 / fps as f64,
            events: Some(events),
        }
    }

    /// Begin the game loop.
    pub fn run<State: GameState>(mut self, initial_state: State) {
        let mut state = initial_state;
        let mut frame_start_t = Instant::now();
        let mut acc = 0;
        let events = self.events.take().unwrap();
        events.run(move |event, _, control_flow| {
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
                    if dt_nanos >= self.nanos_per_frame - acc {
                        frame_start_t = Instant::now();

                        acc += dt_nanos;
                        // limit acc to prevent spiral of death
                        if acc > MAX_ACC_VALUE {
                            acc = MAX_ACC_VALUE;
                        }

                        while acc >= self.nanos_per_frame {
                            #[cfg(feature = "tracy-client")]
                            let _frame = tracy_client::start_noncontinuous_frame!("tick");

                            if state.tick(self.dt_fixed, &self).is_none() {
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                            self.input.tick();
                            acc -= self.nanos_per_frame;
                        }

                        {
                            let _draw_span = tracy_span!("draw", "run");

                            state.draw(&mut self.renderer);
                        }

                        #[cfg(feature = "tracy-client")]
                        tracy_client::finish_continuous_frame!();

                        let nanos_this_frame = frame_start_t.elapsed().as_nanos();
                        // acc represents drift from the perfect tick timing that we should correct by
                        let target_frame_duration = self.nanos_per_frame - acc;
                        // sleep till next frame if we have time to kill
                        if nanos_this_frame < target_frame_duration {
                            let next_frame_t =
                                frame_start_t + Duration::from_nanos(target_frame_duration as u64);
                            *control_flow = ControlFlow::WaitUntil(next_frame_t);
                        } else {
                            *control_flow = ControlFlow::Poll;
                        }
                    }
                }
                Event::WindowEvent { event, .. } => {
                    self.input.track_window_event(&event);
                    match event {
                        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                        WindowEvent::Resized(new_size) => {
                            self.renderer.resize_swap_chain(new_size);
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        });
    }
}

/// The state of a game.
pub trait GameState: Sized + 'static {
    /// Advance the game forward by a timestep. Return None to exit the game.
    fn tick(&mut self, dt: f64, game: &Game) -> Option<()>;
    /// Render the game onto the screen.
    fn draw(&mut self, renderer: &mut crate::graphics::Renderer);
}
