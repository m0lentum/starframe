use std::thread;
use std::time::{Duration, Instant};

use super::statemachine::StateMachine;

// time snapping technique from Tyler Glaiel's blog post
// https://medium.com/@tglaiel/how-to-make-your-game-run-at-60fps-24c61210fe75
const NANOS_120FPS: u128 = 1_000_000_000 / 120;
const NANOS_60FPS: u128 = 1_000_000_000 / 60;
const NANOS_30FPS: u128 = 1_000_000_000 / 30;
const NANOS_20FPS: u128 = 1_000_000_000 / 20;
const NANOS_15FPS: u128 = 1_000_000_000 / 15;
const SNAP_THRESHOLD: u128 = 200_000;

const MAX_ACC_VALUE: u128 = 1_000_000_000 / 8;

pub enum LoopState {
    Continue,
    End,
}

pub trait GameLoop {
    fn begin<D>(&self, state_machine: &mut StateMachine<D>);
}

pub struct LockstepLoop {
    nanos_per_frame: u128,
}

impl LockstepLoop {
    pub fn from_fps(fps: u32) -> Self {
        LockstepLoop {
            nanos_per_frame: 1_000_000_000 / (fps as u128),
        }
    }
}

impl GameLoop for LockstepLoop {
    fn begin<D>(&self, state_machine: &mut StateMachine<D>) {
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
                match state_machine.update() {
                    LoopState::Continue => (),
                    LoopState::End => break 'main,
                }

                acc -= self.nanos_per_frame;
            }

            state_machine.render();

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
