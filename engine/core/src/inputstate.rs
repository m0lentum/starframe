use piston::input::keyboard::Key;
use piston::input::{Button, ButtonEvent, ButtonState, Event};
use std::collections::HashMap;

/// A global input state tracker that you can feed piston input events into
/// and then poll from Systems to avoid complicated event piping.
pub struct InputState {
    keyboard: HashMap<Key, (KeyState, u32)>,
    buffer_frames: u32,
}

impl InputState {
    /// Create a new InputState.
    pub fn new() -> Self {
        InputState {
            keyboard: HashMap::new(),
            buffer_frames: 1,
        }
    }

    /// Create a new InputState with the specified number of buffer frames,
    /// i.e. frames to hold the Released and Pressed states.
    pub fn with_buffer_frames(frames: u32) -> Self {
        InputState {
            keyboard: HashMap::new(),
            buffer_frames: frames,
        }
    }

    /// Updates the ages of tracked keys. Call this every update loop.
    pub fn update_ages(&mut self) {
        for (_key, (state, age)) in &mut self.keyboard {
            *age += 1;

            use KeyState::*;
            match state {
                Released => {
                    if *age >= self.buffer_frames {
                        *state = Unheld;
                        *age = 0;
                    }
                }
                Pressed => {
                    if *age >= self.buffer_frames {
                        *state = Held;
                        *age = 0;
                    }
                }
                Unheld | Held => (),
            }
        }
    }

    /// Add keys for tracking. Only keys added with this method will have their state stored.
    pub fn track_keys(&mut self, keys: &[Key]) {
        self.keyboard.reserve(keys.len());
        for key in keys {
            self.keyboard.insert(*key, (KeyState::Unheld, 0));
        }
    }

    /// Get the state of a keyboard key, or None if it isn't tracked.
    pub fn get_key(&self, key: Key) -> Option<&(KeyState, u32)> {
        self.keyboard.get(&key)
    }

    /// Changes the state of tracked inputs based on a piston Event if it is an applicable input event.
    pub fn handle_event(&mut self, evt: &Event) {
        if let Some(btn) = evt.button_args() {
            match btn.button {
                Button::Keyboard(key) => {
                    if let Some((state, age)) = self.keyboard.get_mut(&key) {
                        match btn.state {
                            ButtonState::Press => match state {
                                KeyState::Unheld | KeyState::Released => *state = KeyState::Pressed,
                                _ => (),
                            },
                            ButtonState::Release => match state {
                                KeyState::Held | KeyState::Pressed => *state = KeyState::Released,
                                _ => (),
                            },
                        }
                        *age = 0;
                    }
                }
                _ => unimplemented!(),
            }
        }
    }
}

/// The state of an individual key,
/// along with the number of updates it has been active for.
/// The Released and Pressed states indicate that the state just changed between held and unheld
/// and are kept active for however long the input buffer on the containing InputState is.
pub enum KeyState {
    Unheld,
    Held,
    Released,
    Pressed,
}
