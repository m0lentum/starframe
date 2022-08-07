use crate::{
    input::{Button, DragState, InputCache, MouseButton},
    math as m,
};

pub struct MouseDragCameraController {
    pub activate_button: Button,
    pub zoom_speed: f64,
    pub min_zoom_out: f64,
    pub max_zoom_out: f64,
    drag_start: Option<m::Transform>,
}

impl Default for MouseDragCameraController {
    fn default() -> Self {
        Self {
            activate_button: MouseButton::Left.into(),
            zoom_speed: 0.01,
            min_zoom_out: 0.1,
            max_zoom_out: 10.0,
            drag_start: None,
        }
    }
}

impl MouseDragCameraController {
    /// Update the camera's position using cached drag state.
    ///
    /// Viewport size is needed to scale mouse movements to the right size of camera movements.
    pub fn update(
        &mut self,
        camera: &mut super::Camera,
        input: &InputCache,
        viewport_size: (u32, u32),
    ) {
        let scaling_factor = camera.scaling_strategy.scaling_factor(viewport_size);
        match (input.drag_state(), self.drag_start) {
            (None, _) => self.drag_start = None,
            (Some(DragState::InProgress { .. }), None) => self.drag_start = Some(camera.transform),
            (Some(DragState::InProgress { start, .. }), Some(pose_at_start)) => {
                let cursor_pos = input.cursor_position().get();
                let offset = m::Vec2::new(
                    (cursor_pos.x - start.x) as f64,
                    -(cursor_pos.y - start.y) as f64,
                );
                camera.transform = pose_at_start;
                camera
                    .transform
                    .append_translation(-offset * camera.transform.scale / scaling_factor);
            }
            (Some(DragState::Completed { start, end, .. }), Some(pose_at_start)) => {
                let offset = m::Vec2::new((end.x - start.x) as f64, -(end.y - start.y) as f64);
                camera.transform = pose_at_start;
                camera
                    .transform
                    .append_translation(-offset * camera.transform.scale / scaling_factor);
                self.drag_start = None;
            }
            _ => (),
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
