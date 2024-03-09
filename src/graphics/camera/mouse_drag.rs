use crate::{
    input::{Button, ButtonQuery, Input, MouseButton},
    math::uv,
};

/// A camera controller that allows movement by dragging with the mouse and zooming
/// with the scroll wheel. Mainly meant for debugging purposes.
pub struct MouseDragCameraController {
    pub activate_button: Button,
    pub reset_button: Option<Button>,
    pub zoom_speed: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
}

impl Default for MouseDragCameraController {
    fn default() -> Self {
        Self {
            activate_button: MouseButton::Middle.into(),
            reset_button: None,
            zoom_speed: 0.01,
            min_zoom: 0.1,
            max_zoom: 10.0,
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
                camera.pose = uv::Isometry3::identity();
                return;
            }
        }

        if input.button(ButtonQuery::from(self.activate_button).held_min(1)) {
            let cursor_delta = input.cursor_movement_world(camera);
            camera.pose.append_translation(uv::Vec3::new(
                -cursor_delta.x as f32,
                -cursor_delta.y as f32,
                0.,
            ));
        }

        let scroll = input.scroll_delta() as f32;
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_zoom = (1.0 + scroll * self.zoom_speed) * camera.zoom;
            camera.zoom = new_zoom.max(self.min_zoom).min(self.max_zoom);
        }
    }
}
