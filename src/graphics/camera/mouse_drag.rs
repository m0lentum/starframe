use crate::{
    input::{Button, ButtonQuery, Input, MouseButton},
    math as m,
};

/// A camera controller that allows movement by dragging with the mouse and zooming
/// with the scroll wheel. Mainly meant for debugging purposes.
pub struct MouseDragCameraController {
    pub activate_button: Button,
    pub reset_button: Option<Button>,
    pub zoom_speed: f64,
    pub min_zoom_out: f64,
    pub max_zoom_out: f64,
}

impl Default for MouseDragCameraController {
    fn default() -> Self {
        Self {
            activate_button: MouseButton::Middle.into(),
            reset_button: None,
            zoom_speed: 0.01,
            min_zoom_out: 0.1,
            max_zoom_out: 10.0,
        }
    }
}

impl MouseDragCameraController {
    /// Update the camera's position using cached drag state.
    ///
    /// Viewport size is needed to scale mouse movements to the right size of camera movements.
    pub fn update(&mut self, camera: &mut super::Camera, input: &Input) {
        if let Some(reset_btn) = self.reset_button {
            if input.button(reset_btn.into()) {
                camera.transform = m::Transform::identity();
                return;
            }
        }

        if input.button(ButtonQuery::from(self.activate_button).held_min(1)) {
            let cursor_delta = input.cursor_movement_world(camera);
            camera.transform.append_translation(-cursor_delta);
        }

        let scroll = input.scroll_delta();
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_scaling = (1.0 + scroll * -self.zoom_speed) * camera.transform.scale;
            let new_scaling = new_scaling.max(self.min_zoom_out).min(self.max_zoom_out);
            camera.transform.scale = new_scaling;
        }
    }
}
