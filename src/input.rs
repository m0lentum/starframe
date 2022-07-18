use crate::math as m;

use winit::dpi::PhysicalPosition;
use winit::event as ev;

pub use ev::{ElementState, MouseButton, VirtualKeyCode as Key};

/// Track the state of input devices so that they can be looked up from a single location
/// instead of moving window events around.
#[derive(Clone, Debug)]
pub struct InputCache {
    // keyboard stored as an array addressed by `Key as usize`.
    // when updating winit, make sure this is as big as the enum!
    keyboard: [AgedState; 163],
    mouse_buttons: MouseButtonState,
    cursor_pos: CursorPosition,
    scroll_delta: f64,
    drag_state: Option<DragState>,
}

impl InputCache {
    pub fn new() -> Self {
        InputCache {
            keyboard: [AgedState::default(); 163],
            mouse_buttons: Default::default(),
            cursor_pos: CursorPosition::OutOfWindow(PhysicalPosition::new(0.0, 0.0)),
            scroll_delta: 0.0,
            drag_state: None,
        }
    }

    //
    // Getters
    //

    /// Check the state of a button (keyboard key, mouse button, controller button (TODO))
    /// against a query.
    ///
    /// ```
    /// # use starframe::input::{InputCache, ButtonQuery, Key};
    /// # let input = InputCache::new();
    /// if input.button(
    ///     ButtonQuery::kb(Key::Z).held_min(10)
    /// ) {
    ///     // character does a big jump or something idk
    /// }
    /// ```
    pub fn button(&self, q: ButtonQuery) -> bool {
        let AgedState { state, age } = self.get_button_state(q.button);
        if state != q.state {
            return false;
        }
        if age < q.min_age || age > q.max_age {
            return false;
        }
        true
    }

    /// Get the state of an axis defined by a positive and negative key
    /// or an analog axis (TODO).
    /// Returns a value between -1.0 and 1.0.
    ///
    /// Prefers the positive key if both are pressed.
    pub fn axis(&self, q: AxisQuery) -> f64 {
        match (
            self.get_button_state(q.pos_btn).state,
            self.get_button_state(q.neg_btn).state,
        ) {
            (ElementState::Pressed, _) => 1.0,
            (_, ElementState::Pressed) => -1.0,
            _ => 0.0,
        }
    }

    /// Get the cursor position in logical pixels down and right from the top left.
    #[inline]
    pub fn cursor_position(&self) -> CursorPosition {
        self.cursor_pos
    }

    /// Get the vertical scroll distance in pixels during the last tick.
    #[inline]
    pub fn scroll_delta(&self) -> f64 {
        self.scroll_delta
    }

    #[inline]
    pub fn drag_state(&self) -> Option<DragState> {
        self.drag_state
    }

    /// Get the state of a keyboard key along with the number of frames since it last changed.
    #[inline]
    pub fn get_key_state(&self, key: Key) -> AgedState {
        self.keyboard[key as usize]
    }

    #[inline]
    pub fn get_mouse_button_state(&self, mb: MouseButton) -> AgedState {
        *self
            .mouse_buttons
            .get(mb)
            .unwrap_or_else(|| panic!("Untracked mouse button: {:?}", mb))
    }

    #[inline]
    pub fn get_button_state(&self, btn: Button) -> AgedState {
        match btn {
            Button::Keyboard(key) => self.get_key_state(key),
            Button::Mouse(mb) => self.get_mouse_button_state(mb),
        }
    }

    //
    // internal state updates
    //

    /// Do maintenance such as updating the ages of pressed keys.
    /// Call this at the end of every frame.
    ///
    /// Calling is handled internally by [`Game`][crate::game::Game].
    pub(crate) fn tick(&mut self) {
        for state in &mut self.keyboard {
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

    /// Track the effect of a keyboard event.
    pub(crate) fn track_keyboard(&mut self, evt: ev::KeyboardInput) {
        if let Some(code) = evt.virtual_keycode {
            let cached_key = &mut self.keyboard[code as usize];
            if evt.state != cached_key.state {
                *cached_key = AgedState::new(evt.state);
            }
        }
    }

    /// Perform whatever tracking is available for the given window event.
    pub(crate) fn track_window_event(&mut self, event: &ev::WindowEvent) {
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
    pub(crate) fn track_mouse_button(&mut self, button: ev::MouseButton, new_state: ElementState) {
        if let Some(s) = self.mouse_buttons.get_mut(button) {
            *s = AgedState::new(new_state);
        }

        // drag, at least for now hardcoded to only work with left click
        match (button, new_state, self.drag_state) {
            (ev::MouseButton::Left, ElementState::Pressed, None) => self.begin_drag(),
            (ev::MouseButton::Left, ElementState::Released, _) => self.finish_drag(),
            _ => (),
        }
    }

    #[inline]
    pub(crate) fn track_cursor_movement(&mut self, position: PhysicalPosition<f64>) {
        *self.cursor_pos.get_mut() = position;
    }

    #[inline]
    pub(crate) fn track_cursor_enter(&mut self) {
        self.cursor_pos = CursorPosition::InWindow(self.cursor_pos.get());
    }

    #[inline]
    pub(crate) fn track_cursor_leave(&mut self) {
        self.cursor_pos = CursorPosition::OutOfWindow(self.cursor_pos.get());
    }

    /// Track a mouse wheel movement.
    ///
    /// TODO: test to make line and pixel delta effects match
    pub(crate) fn track_mouse_wheel(&mut self, delta: ev::MouseScrollDelta) {
        const PIXELS_PER_LINE: f64 = 10.0;

        use ev::MouseScrollDelta::*;
        match delta {
            LineDelta(_, y) => self.scroll_delta += PIXELS_PER_LINE * y as f64,
            PixelDelta(PhysicalPosition { y, .. }) => self.scroll_delta += y as f64,
        }
    }

    fn begin_drag(&mut self) {
        self.drag_state = Some(DragState::InProgress {
            start: self.cursor_pos.get(),
            duration: 0,
        });
    }

    fn finish_drag(&mut self) {
        if let Some(DragState::InProgress { start, duration }) = self.drag_state {
            self.drag_state = Some(DragState::Completed {
                start,
                duration,
                end: self.cursor_pos.get(),
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
// queries
//

/// A button on any input device (keyboard, mouse, TODO: gamepad).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Keyboard(Key),
    Mouse(MouseButton),
}
impl From<Key> for Button {
    fn from(k: Key) -> Self {
        Button::Keyboard(k)
    }
}
impl From<MouseButton> for Button {
    fn from(mb: MouseButton) -> Self {
        Button::Mouse(mb)
    }
}

/// A query for matching against the state of a button.
/// See [InputCache::button][`self::InputCache::button] for usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonQuery {
    pub button: Button,
    pub state: ElementState,
    pub min_age: usize,
    pub max_age: usize,
}

impl ButtonQuery {
    /// Query a keyboard key.
    ///
    /// With no additional modifiers the query checks for that key
    /// having just been pressed down after last frame.
    ///
    /// `From<Key>` is also implemented for `ButtonQuery`,
    /// so these two are equivalent:
    /// ```
    /// # use starframe::input::{ButtonQuery, Key};
    /// assert_eq!(
    ///     ButtonQuery::kb(Key::P),
    ///     Key::P.into(),
    /// );
    /// ```
    #[inline]
    pub fn kb(key: Key) -> Self {
        Self {
            button: Button::Keyboard(key),
            state: ElementState::Pressed,
            min_age: 0,
            max_age: 0,
        }
    }

    /// Query a mouse button.
    ///
    /// With no additional modifiers the query checks for that button
    /// having just been pressed down after last frame.
    #[inline]
    pub fn mouse(btn: MouseButton) -> Self {
        Self {
            button: Button::Mouse(btn),
            state: ElementState::Pressed,
            min_age: 0,
            max_age: 0,
        }
    }

    /// Modify the query to look for the released state instead of pressed.
    ///
    /// Can be combined with the different variants of `held_*` to match keys
    /// that have been released for some amount of time.
    /// ```
    /// # use starframe::input::{ButtonQuery, Key};
    /// let havent_pressed_x_for_a_while = ButtonQuery::kb(Key::X)
    ///     .released()
    ///     .held_exact(42);
    /// ```
    #[inline]
    pub fn released(mut self) -> Self {
        self.state = ElementState::Released;
        self
    }

    /// Match if the button has been held for any amount of time.
    ///
    /// This overwrites the parameters set by any other variant of `held_*`.
    #[inline]
    pub fn held(mut self) -> Self {
        self.min_age = 0;
        self.max_age = usize::MAX;
        self
    }

    /// Match if the button has been held for exactly the given number of frames.
    ///
    /// This overwrites the parameters set by any other variant of `held_*`.
    #[inline]
    pub fn held_exact(mut self, frames: usize) -> Self {
        self.min_age = frames;
        self.max_age = frames;
        self
    }

    /// Match if the button has been held for at least the given number of frames.
    ///
    /// This overwrites the parameters set by any other variant of `held_*`.
    #[inline]
    pub fn held_min(mut self, frames: usize) -> Self {
        self.min_age = frames;
        self.max_age = usize::MAX;
        self
    }

    /// Match if the button has been held for at most the given number of frames.
    ///
    /// This overwrites the parameters set by any other variant of `held_*`.
    #[inline]
    pub fn held_max(mut self, frames: usize) -> Self {
        self.min_age = 0;
        self.max_age = frames;
        self
    }

    /// Match if the button has been held for a number of frames between the given values.
    ///
    /// This overwrites the parameters set by any other variant of `held_*`.
    #[inline]
    pub fn held_range(mut self, min_frames: usize, max_frames: usize) -> Self {
        self.min_age = min_frames;
        self.max_age = max_frames;
        self
    }
}

impl From<Key> for ButtonQuery {
    fn from(k: Key) -> Self {
        Self::kb(k)
    }
}

impl From<MouseButton> for ButtonQuery {
    fn from(mb: MouseButton) -> Self {
        Self::mouse(mb)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AxisQuery {
    pub pos_btn: Button,
    pub neg_btn: Button,
}

//
// state types
//

/// The state of a button (keyboard key or mouse button)
/// and time in number of ticks since last state change.
#[derive(Clone, Copy, Debug)]
pub struct AgedState {
    pub state: ElementState,
    pub age: usize,
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

// Mouse

/// Cursor position taking into account whether it's in the window or not.
/// Usually you don't want to do anything if you're outside the window.
#[derive(Clone, Copy, Debug)]
pub enum CursorPosition {
    InWindow(PhysicalPosition<f64>),
    OutOfWindow(PhysicalPosition<f64>),
}

impl CursorPosition {
    pub fn get(&self) -> PhysicalPosition<f64> {
        match self {
            CursorPosition::InWindow(p) => *p,
            CursorPosition::OutOfWindow(p) => *p,
        }
    }

    pub fn get_in_window(&self) -> Option<PhysicalPosition<f64>> {
        match self {
            Self::InWindow(p) => Some(*p),
            Self::OutOfWindow(_) => None,
        }
    }

    fn get_mut(&mut self) -> &mut PhysicalPosition<f64> {
        match self {
            CursorPosition::InWindow(p) => p,
            CursorPosition::OutOfWindow(p) => p,
        }
    }
}

impl From<CursorPosition> for m::Vec2 {
    fn from(cp: CursorPosition) -> m::Vec2 {
        let pos = cp.get();
        m::Vec2::new(pos.x, pos.y)
    }
}

//

#[derive(Clone, Copy, Debug, Default)]
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

#[derive(Clone, Copy, Debug)]
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
