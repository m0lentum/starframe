use std::thread;
use std::time::{Duration, Instant};

// time snapping technique from Tyler Glaiel's blog post
// https://medium.com/@tglaiel/how-to-make-your-game-run-at-60fps-24c61210fe75
const NANOS_120FPS: u128 = 1_000_000_000 / 120;
const NANOS_60FPS: u128 = 1_000_000_000 / 60;
const NANOS_30FPS: u128 = 1_000_000_000 / 30;
const NANOS_20FPS: u128 = 1_000_000_000 / 20;
const NANOS_15FPS: u128 = 1_000_000_000 / 15;
const SNAP_THRESHOLD: u128 = 200_000;

const MAX_ACC_VALUE: u128 = 1_000_000_000 / 8;

/// Globally used information that is created before the game loop starts and owned by the game loop.
pub struct Globals {
    pub input: crate::core::InputCache,
}
impl Globals {
    fn init() -> Self {
        Globals {
            input: crate::core::InputCache::new(),
        }
    }
}
/// The entire state of a game.
pub trait GameState: Sized {
    /// Advance the game forward by a timestep and return the new state at the end of it.
    fn tick(self, dt: f32, globals: &Globals) -> Option<Self>;
    /// Render the game onto the screen.
    fn draw<S: glium::Surface>(&self, target: &mut S, globals: &Globals);
    /// Handle a winit event.
    /// For instance, you might use this to recalculate a camera's scaling factor on window resize.
    fn on_event(&mut self, event: &glutin::Event, globals: &Globals);
}

/// A game loop's job is to call the `GameState`'s `tick` and `render` methods
/// at appropriate times. These times can be different between different loop types.
pub trait GameLoop {
    /// Start the game.
    fn run<S: GameState>(self, initial_state: S);
}

/// A loop that runs both simulation and rendering at a fixed framerate.
///
/// ```
/// LockstepLoop::from_fps(60).run(MyState::init());
/// ```
pub struct LockstepLoop {
    nanos_per_frame: u128,
    dt: f32,
    events: glutin::EventsLoop,
    globals: Globals,
}

impl LockstepLoop {
    pub fn from_fps(fps: u32) -> Self {
        LockstepLoop {
            nanos_per_frame: 1_000_000_000 / u128::from(fps),
            dt: 1.0 / fps as f32,
            events: unsafe { crate::graphics::Context::init() },
            globals: Globals::init(),
        }
    }
}

impl GameLoop for LockstepLoop {
    fn run<S: GameState>(mut self, initial_state: S) {
        let mut state = initial_state;

        let mut acc = 0;
        let mut prev_time = Instant::now();
        'main: loop {
            // if vsynced, pretend frame timing is exact (see blog post mentioned above)
            let mut dt = prev_time.elapsed().as_nanos();
            if should_snap(dt, NANOS_120FPS) {
                dt = NANOS_120FPS;
            } else if should_snap(dt, NANOS_60FPS) {
                dt = NANOS_60FPS;
            } else if should_snap(dt, NANOS_30FPS) {
                dt = NANOS_30FPS;
            } else if should_snap(dt, NANOS_20FPS) {
                dt = NANOS_20FPS;
            } else if should_snap(dt, NANOS_15FPS) {
                dt = NANOS_15FPS;
            }

            acc += dt;
            // limit acc to prevent spiral of death
            if acc > MAX_ACC_VALUE {
                acc = MAX_ACC_VALUE;
            }

            while acc >= self.nanos_per_frame {
                // window events
                let mut should_close = false;
                let globals = &mut self.globals;
                use glutin::WindowEvent::*;
                self.events.poll_events(|evt| {
                    state.on_event(&evt, globals);
                    match evt {
                        glutin::Event::WindowEvent { event, .. } => {
                            globals.input.track_window_event(&event);
                            match event {
                                CloseRequested => should_close = true,
                                _ => (),
                            }
                        }
                        _ => (),
                    }
                });
                if should_close {
                    break 'main;
                }

                // tick
                match state.tick(self.dt, &self.globals) {
                    Some(new_state) => state = new_state,
                    None => break 'main,
                }
                self.globals.input.tick();

                acc -= self.nanos_per_frame;
            }

            // draw

            use glium::Surface;
            let ctx = crate::graphics::Context::get();
            let mut target = ctx.display.draw();
            target.clear_color(0.1, 0.1, 0.1, 1.0);

            state.draw(&mut target, &self.globals);

            target.finish().unwrap();

            // sleep till next frame

            prev_time = Instant::now();
            thread::sleep(Duration::from_nanos((self.nanos_per_frame - acc) as u64));
        }
    }
}

fn should_snap(dt: u128, target: u128) -> bool {
    if dt < target {
        target - dt < SNAP_THRESHOLD
    } else {
        dt - target < SNAP_THRESHOLD
    }
}
