use piston::input::keyboard::Key;
use piston::input::{Button, ButtonEvent, ButtonState, Event};
use std::collections::HashMap;

/// A global input state tracker that you can feed piston input events into
/// and then poll from Systems to avoid complicated event piping.
pub struct InputState {
    keyboard: HashMap<Key, (KeyState, u32)>,
}

impl InputState {
    /// Create a new InputState.
    pub fn new() -> Self {
        InputState {
            keyboard: HashMap::new(),
        }
    }

    /// Updates the ages of tracked keys. Call this every update loop.
    pub fn update_ages(&mut self) {
        for (_key, (_state, age)) in &mut self.keyboard {
            *age += 1;
        }
    }

    /// Add keys for tracking. Only keys added with this method will have their state stored.
    pub fn track_keys(&mut self, keys: &[Key]) {
        self.keyboard.reserve(keys.len());
        for key in keys {
            self.keyboard.insert(*key, (KeyState::Released, 0));
        }
    }

    /// Get the state of a keyboard key along with its age, or None if it isn't tracked.
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
                            ButtonState::Press => {
                                if let KeyState::Released = state {
                                    *state = KeyState::Pressed;
                                    *age = 0;
                                }
                            }
                            ButtonState::Release => {
                                if let KeyState::Pressed = state {
                                    *state = KeyState::Released;
                                    *age = 0;
                                }
                            }
                        }
                    }
                }
                _ => (),
            }
        }
    }
}

/// The state of an individual key.
pub enum KeyState {
    Released,
    Pressed,
}
