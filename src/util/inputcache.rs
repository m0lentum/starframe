use glutin::{dpi::LogicalPosition, VirtualKeyCode};
use std::collections::HashMap;

pub use glutin::ElementState;

/// A global input state cache that you can feed input events into
/// and poll from anywhere to avoid complicated event piping.
#[derive(Clone)]
pub struct InputCache {
    keyboard: HashMap<VirtualKeyCode, AgedState>,
    mouse_buttons: MouseButtonState,
    cursor_pos: CursorPosition,
    scroll_delta: f32,
}

impl InputCache {
    pub fn new() -> Self {
        InputCache {
            keyboard: HashMap::new(),
            mouse_buttons: Default::default(),
            cursor_pos: CursorPosition::OutOfWindow(LogicalPosition::new(0.0, 0.0)),
            scroll_delta: 0.0,
        }
    }

    /// Do maintenance such as updating the ages of pressed keys.
    /// Call this at the end of every frame.
    pub fn tick(&mut self) {
        for state in self.keyboard.values_mut() {
            state.age += 1;
        }

        self.mouse_buttons.left.age += 1;
        self.mouse_buttons.middle.age += 1;
        self.mouse_buttons.right.age += 1;

        self.scroll_delta = 0.0;
    }

    /// Add keys for tracking. Only keys added with this method will have their state stored.
    pub fn track_keys(&mut self, keys: &[VirtualKeyCode]) {
        self.keyboard.reserve(keys.len());
        for key in keys {
            self.keyboard
                .insert(*key, AgedState::new(ElementState::Released));
        }
    }

    //
    // Getters
    //

    /// Get the state of a keyboard key along with its age, or None if it isn't tracked.
    pub fn get_key_state(&self, key: VirtualKeyCode) -> Option<&AgedState> {
        self.keyboard.get(&key)
    }

    /// True if the requested key is currently pressed
    /// (for fewer frames than age_limit if provided), false otherwise.
    /// # Panics
    /// Panics if the requested key is not tracked.
    pub fn is_key_pressed(&self, key: VirtualKeyCode, age_limit: Option<u32>) -> bool {
        let AgedState { state, age } = self
            .keyboard
            .get(&key)
            .unwrap_or_else(|| panic!("Untracked key: {:?}", key));

        if let ElementState::Pressed = state {
            if let Some(al) = age_limit {
                *age <= al
            } else {
                true
            }
        } else {
            false
        }
    }

    /// True if the requested mouse button is currently pressed
    /// (for fewer frames than age_limit if provided), false otherwise.
    /// # Panics
    /// Panics if the requested mouse button is not tracked.
    /// Left, Middle and Right are tracked by default.
    pub fn is_mouse_button_pressed(
        &self,
        button: glutin::MouseButton,
        age_limit: Option<u32>,
    ) -> bool {
        let AgedState { age, state } = self
            .mouse_buttons
            .get(button)
            .unwrap_or_else(|| panic!("Untracked mouse button: {:?}", button));

        if let ElementState::Pressed = state {
            if let Some(al) = age_limit {
                *age <= al
            } else {
                true
            }
        } else {
            false
        }
    }

    /// Get the cursor position in logical pixels down and right from the top left.
    pub fn cursor_position(&self) -> &CursorPosition {
        &self.cursor_pos
    }

    /// Get the vertical scroll distance in pixels during the last tick.
    pub fn scroll_delta(&self) -> f32 {
        self.scroll_delta
    }

    //
    // Trackers
    //

    /// Track the effect of a keyboard event.
    pub fn track_keyboard(&mut self, evt: glutin::KeyboardInput) {
        if let Some(code) = evt.virtual_keycode {
            if let Some(state) = self.keyboard.get_mut(&code) {
                match evt.state {
                    ElementState::Pressed => {
                        if let ElementState::Released = state.state {
                            *state = AgedState::new(ElementState::Pressed);
                        }
                    }
                    ElementState::Released => {
                        if let ElementState::Pressed = state.state {
                            *state = AgedState::new(ElementState::Released);
                        }
                    }
                }
            }
        }
    }

    /// Perform whatever tracking is available for the given window event.
    pub fn track_window_event(&mut self, event: &glutin::WindowEvent) {
        use glutin::WindowEvent::*;
        match event {
            KeyboardInput { input, .. } => self.track_keyboard(*input),
            MouseInput { button, state, .. } => self.track_mouse_button(*button, *state),
            MouseWheel { delta, .. } => self.track_mouse_wheel(*delta),
            CursorMoved { position, .. } => self.track_cursor_movement(*position),
            CursorEntered { .. } => self.track_cursor_enter(),
            CursorLeft { .. } => self.track_cursor_leave(),
            _ => (),
        }
    }

    /// Track a mouse button event.
    pub fn track_mouse_button(&mut self, button: glutin::MouseButton, state: ElementState) {
        self.mouse_buttons
            .get_mut(button)
            .map(|s| *s = AgedState::new(state));
    }

    /// Track the screen position of the mouse cursor.
    pub fn track_cursor_movement(&mut self, position: LogicalPosition) {
        *self.cursor_pos.get_mut() = position;
    }

    pub fn track_cursor_enter(&mut self) {
        self.cursor_pos = CursorPosition::InWindow(self.cursor_pos.take());
    }

    pub fn track_cursor_leave(&mut self) {
        self.cursor_pos = CursorPosition::OutOfWindow(self.cursor_pos.take());
    }

    /// Track a mouse wheel movement.
    ///
    /// TODO: test to make line and pixel delta effects match
    ///
    pub fn track_mouse_wheel(&mut self, delta: glutin::MouseScrollDelta) {
        const PIXELS_PER_LINE: f32 = 10.0;

        use glutin::MouseScrollDelta::*;
        match delta {
            LineDelta(_, y) => self.scroll_delta += PIXELS_PER_LINE * y,
            PixelDelta(LogicalPosition { y, .. }) => self.scroll_delta += y as f32,
        }
    }
}

impl Default for InputCache {
    fn default() -> Self {
        Self::new()
    }
}

//

/// The state of a button (keyboard key or mouse button)
/// and time in number of ticks since last state change.
#[derive(Clone, Copy)]
pub struct AgedState {
    state: ElementState,
    age: u32,
}

impl AgedState {
    pub fn new(state: ElementState) -> Self {
        AgedState { state, age: 0 }
    }
}

impl Default for AgedState {
    fn default() -> Self {
        Self::new(ElementState::Released)
    }
}

//

/// Cursor position taking into account whether it's in the window or not.
/// Usually you don't want to do anything if you're outside the window.
#[derive(Clone, Copy)]
pub enum CursorPosition {
    InWindow(LogicalPosition),
    OutOfWindow(LogicalPosition),
}

impl CursorPosition {
    pub fn get(&self) -> &LogicalPosition {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }

    pub fn get_mut(&mut self) -> &mut LogicalPosition {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }

    pub fn take(self) -> LogicalPosition {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }
}

//

#[derive(Clone, Copy, Default)]
struct MouseButtonState {
    left: AgedState,
    middle: AgedState,
    right: AgedState,
}

impl MouseButtonState {
    pub fn get(&self, button: glutin::MouseButton) -> Option<&AgedState> {
        use glutin::MouseButton as MB;
        match button {
            MB::Left => Some(&self.left),
            MB::Middle => Some(&self.middle),
            MB::Right => Some(&self.right),
            MB::Other(_) => None,
        }
    }

    pub fn get_mut(&mut self, button: glutin::MouseButton) -> Option<&mut AgedState> {
        use glutin::MouseButton as MB;
        match button {
            MB::Left => Some(&mut self.left),
            MB::Middle => Some(&mut self.middle),
            MB::Right => Some(&mut self.right),
            MB::Other(_) => None,
        }
    }
}
