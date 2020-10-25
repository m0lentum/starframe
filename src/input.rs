use std::collections::HashMap;
use winit::dpi::PhysicalPosition;

use winit::event as ev;

pub use ev::ElementState;
pub use ev::MouseButton;
pub use ev::VirtualKeyCode as Key;

/// Track the state of input devices so that they can be looked up from a single location
/// instead of moving window events around.
#[derive(Clone)]
pub struct InputCache {
    keyboard: HashMap<Key, AgedState>,
    mouse_buttons: MouseButtonState,
    cursor_pos: CursorPosition,
    scroll_delta: f32,
    drag_state: Option<DragState>,
}

impl InputCache {
    pub fn new() -> Self {
        InputCache {
            // immediately allocate enough space to fit every key the user presses
            keyboard: HashMap::with_capacity(128),
            mouse_buttons: Default::default(),
            cursor_pos: CursorPosition::OutOfWindow(PhysicalPosition::new(0.0, 0.0)),
            scroll_delta: 0.0,
            drag_state: None,
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

        match self.drag_state {
            Some(DragState::InProgress {
                ref mut duration, ..
            }) => *duration += 1,
            Some(DragState::Completed { .. }) => self.drag_state = None,
            None => (),
        }
    }

    //
    // Getters
    //

    /// Get the state of a keyboard key along with the number of frames since it last changed.
    /// Returns None if the key has never been touched.
    pub fn get_key_state(&self, key: Key) -> Option<&AgedState> {
        self.keyboard.get(&key)
    }

    /// True if the requested key is currently pressed
    /// (for fewer frames than age_limit if provided), false otherwise.
    pub fn is_key_pressed(&self, key: Key, age_limit: Option<u32>) -> bool {
        match self.get_key_state(key) {
            None => false,
            Some(AgedState {
                state: ElementState::Released,
                ..
            }) => false,
            Some(AgedState {
                age,
                state: ElementState::Pressed,
            }) => {
                if let Some(al) = age_limit {
                    *age <= al
                } else {
                    true
                }
            }
        }
    }

    /// Get the state of an axis defined by a positive and negavite key.
    /// Prefers the positive key if both are pressed.
    pub fn get_key_axis_state(&self, pos_key: Key, neg_key: Key) -> KeyAxisState {
        use ElementState::*;
        use KeyAxisState::*;
        match (
            self.get_key_state(pos_key).map(|s| s.state),
            self.get_key_state(neg_key).map(|s| s.state),
        ) {
            (Some(Pressed), _) => Pos,
            (_, Some(Pressed)) => Neg,
            _ => Zero,
        }
    }

    /// True if the requested mouse button is currently pressed
    /// (for fewer frames than age_limit if provided), false otherwise.
    /// # Panics
    /// Panics if the requested mouse button is not tracked.
    /// Left, Middle and Right are tracked by default.
    pub fn is_mouse_button_pressed(&self, button: ev::MouseButton, age_limit: Option<u32>) -> bool {
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

    pub fn drag_state(&self) -> &Option<DragState> {
        &self.drag_state
    }

    //
    // Trackers
    //

    /// Track the effect of a keyboard event.
    pub fn track_keyboard(&mut self, evt: ev::KeyboardInput) {
        if let Some(code) = evt.virtual_keycode {
            self.keyboard
                .entry(code)
                .and_modify(|e| match evt.state {
                    ElementState::Pressed => {
                        if let ElementState::Released = e.state {
                            *e = AgedState::new(ElementState::Pressed);
                        }
                    }
                    ElementState::Released => {
                        if let ElementState::Pressed = e.state {
                            *e = AgedState::new(ElementState::Released);
                        }
                    }
                })
                .or_insert(AgedState::new(evt.state));
        }
    }

    /// Perform whatever tracking is available for the given window event.
    pub fn track_window_event(&mut self, event: &ev::WindowEvent) {
        use ev::WindowEvent::*;
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
    pub fn track_mouse_button(&mut self, button: ev::MouseButton, new_state: ElementState) {
        self.mouse_buttons
            .get_mut(button)
            .map(|s| *s = AgedState::new(new_state));

        // drag, at least for now hardcoded to only work with left click
        match (button, new_state, self.drag_state) {
            (ev::MouseButton::Left, ElementState::Pressed, None) => self.begin_drag(),
            (ev::MouseButton::Left, ElementState::Released, _) => self.finish_drag(),
            _ => (),
        }
    }

    /// Track the screen position of the mouse cursor.
    pub fn track_cursor_movement(&mut self, position: PhysicalPosition<f64>) {
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
    pub fn track_mouse_wheel(&mut self, delta: ev::MouseScrollDelta) {
        const PIXELS_PER_LINE: f32 = 10.0;

        use ev::MouseScrollDelta::*;
        match delta {
            LineDelta(_, y) => self.scroll_delta += PIXELS_PER_LINE * y,
            PixelDelta(PhysicalPosition { y, .. }) => self.scroll_delta += y as f32,
        }
    }

    fn begin_drag(&mut self) {
        self.drag_state = Some(DragState::InProgress {
            start: *self.cursor_pos.get(),
            duration: 0,
        });
    }

    fn finish_drag(&mut self) {
        if let Some(DragState::InProgress { start, duration }) = self.drag_state {
            self.drag_state = Some(DragState::Completed {
                start,
                duration,
                end: *self.cursor_pos.get(),
            });
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
    pub state: ElementState,
    pub age: u32,
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

/// The state of an input axis defined by a positive and negative key.
pub enum KeyAxisState {
    Pos,
    Zero,
    Neg,
}

// Mouse

/// Cursor position taking into account whether it's in the window or not.
/// Usually you don't want to do anything if you're outside the window.
#[derive(Clone, Copy)]
pub enum CursorPosition {
    InWindow(PhysicalPosition<f64>),
    OutOfWindow(PhysicalPosition<f64>),
}

impl CursorPosition {
    pub fn get(&self) -> &PhysicalPosition<f64> {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }

    pub fn get_mut(&mut self) -> &mut PhysicalPosition<f64> {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }

    pub fn take(self) -> PhysicalPosition<f64> {
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
    pub fn get(&self, button: MouseButton) -> Option<&AgedState> {
        use MouseButton as MB;
        match button {
            MB::Left => Some(&self.left),
            MB::Middle => Some(&self.middle),
            MB::Right => Some(&self.right),
            MB::Other(_) => None,
        }
    }

    pub fn get_mut(&mut self, button: MouseButton) -> Option<&mut AgedState> {
        use MouseButton as MB;
        match button {
            MB::Left => Some(&mut self.left),
            MB::Middle => Some(&mut self.middle),
            MB::Right => Some(&mut self.right),
            MB::Other(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum DragState {
    InProgress {
        start: PhysicalPosition<f64>,
        duration: u32,
    },
    Completed {
        start: PhysicalPosition<f64>,
        end: PhysicalPosition<f64>,
        duration: u32,
    },
}
