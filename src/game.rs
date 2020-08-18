//! Tools for creating a window and starting a managed game loop.

use std::time::{Duration, Instant};

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

/// A Game manages the global resources a game needs like a window and a graphics renderer.
///
/// # Example
/// ```
/// # use starframe::game::{LockstepLoop, Game, GameState};
/// # struct MyState;
/// # impl MyState {
/// #    fn init() -> Self { Self }
/// # }
/// # impl GameState for MyState {
/// #   fn tick(&mut self, dt: f32, game: &Game) -> Option<()> { None }
/// #   fn draw(&mut self, renderer: &mut starframe::graphics::Renderer) {}
/// # }
/// let game = Game::init(winit::window::WindowBuilder::new());
/// let game_loop = LockstepLoop::from_fps(60);
/// let state = MyState::init();
/// game.run(game_loop, state);
/// ```
pub struct Game {
    /// A global input cache that is automatically updated once per tick.
    pub input: crate::InputCache,
    /// The window the game draws to.
    pub window: Window,
    /// A renderer that can draw to the game's window.
    pub renderer: crate::graphics::Renderer,
    // awkwardly moving the event loop around for the sake of clean API
    events: Option<EventLoop<()>>,
}
impl Game {
    /// Create the resources you need for a game.
    ///
    /// This does not immediately start the game, since you may want to
    /// use e.g. the renderer to initialize some resources first.
    pub fn init(window_b: WindowBuilder) -> Self {
        let events: EventLoop<()> = EventLoop::new();
        let window = window_b.build(&events).expect("Failed to create window");

        let renderer = futures::executor::block_on(crate::graphics::Renderer::init(&window));
        Game {
            input: crate::InputCache::new(),
            window,
            events: Some(events),
            renderer,
        }
    }

    /// Begin the game loop.
    pub fn run<State: GameState>(self, gameloop: impl GameLoop, state: State) {
        gameloop.run(self, state);
    }
}
/// The entire state of a game.
pub trait GameState: Sized + 'static {
    /// Advance the game forward by a timestep. Return None to exit the game.
    fn tick(&mut self, dt: f32, game: &Game) -> Option<()>;
    /// Render the game onto the screen.
    fn draw(&mut self, renderer: &mut crate::graphics::Renderer);
}

/// A game loop's job is to call the `GameState`'s `tick` and `render` methods
/// at appropriate times. These times can be different between different loop types.
pub trait GameLoop {
    /// Start the loop. This is usually done via `Game::run`.
    fn run<S: GameState>(self, game: Game, initial_state: S);
}

// time snapping technique from Tyler Glaiel's blog post
// https://medium.com/@tglaiel/how-to-make-your-game-run-at-60fps-24c61210fe75
const NANOS_120FPS: u128 = 1_000_000_000 / 120;
const NANOS_60FPS: u128 = 1_000_000_000 / 60;
const NANOS_30FPS: u128 = 1_000_000_000 / 30;
const NANOS_20FPS: u128 = 1_000_000_000 / 20;
const NANOS_15FPS: u128 = 1_000_000_000 / 15;
const SNAP_THRESHOLD: u128 = 200_000;

const MAX_ACC_VALUE: u128 = 1_000_000_000 / 8;

/// A loop that runs both simulation and rendering at a fixed framerate.
pub struct LockstepLoop {
    nanos_per_frame: u128,
    dt: f32,
}

impl LockstepLoop {
    pub fn from_fps(fps: u32) -> Self {
        LockstepLoop {
            nanos_per_frame: 1_000_000_000 / u128::from(fps),
            dt: 1.0 / fps as f32,
        }
    }
}

impl GameLoop for LockstepLoop {
    fn run<State: GameState>(self, mut game: Game, initial_state: State) {
        let mut state = initial_state;
        let events = game.events.take().unwrap();

        let mut frame_start_t = Instant::now();
        let mut acc = 0;
        events.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Poll;
            match event {
                Event::MainEventsCleared => {
                    // if vsynced, pretend frame timing is exact (see blog post mentioned above)
                    let mut dt = frame_start_t.elapsed().as_nanos();
                    if should_snap(dt, NANOS_120FPS) {
                        dt = NANOS_120FPS;
                        acc = 0;
                    } else if should_snap(dt, NANOS_60FPS) {
                        dt = NANOS_60FPS;
                        acc = 0;
                    } else if should_snap(dt, NANOS_30FPS) {
                        dt = NANOS_30FPS;
                        acc = 0;
                    } else if should_snap(dt, NANOS_20FPS) {
                        dt = NANOS_20FPS;
                        acc = 0;
                    } else if should_snap(dt, NANOS_15FPS) {
                        dt = NANOS_15FPS;
                        acc = 0;
                    }

                    // if we're going too fast just wait, otherwise run as many ticks
                    // as have been passed since last update and draw once
                    if dt >= self.nanos_per_frame - acc {
                        frame_start_t = Instant::now();

                        acc += dt;
                        // limit acc to prevent spiral of death
                        if acc > MAX_ACC_VALUE {
                            acc = MAX_ACC_VALUE;
                        }

                        while acc >= self.nanos_per_frame {
                            if let None = state.tick(self.dt, &game) {
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                            game.input.tick();
                            acc -= self.nanos_per_frame;
                        }

                        state.draw(&mut game.renderer);

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

fn should_snap(dt: u128, target: u128) -> bool {
    if dt < target {
        target - dt < SNAP_THRESHOLD
    } else {
        dt - target < SNAP_THRESHOLD
    }
}
