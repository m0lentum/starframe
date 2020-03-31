use crate::{
    core::Transform,
    util::inputcache::{DragState, InputCache},
};

use ultraviolet as uv;

/// Camera controllers are rules for transforming a camera.
pub trait CameraController {
    fn transform(&self) -> &Transform;
}

/// Tells a camera how to adapt to changing viewport size.
pub enum ScalingStrategy {
    /// Scale so that there are a constant number of viewport pixels
    /// per world coordinate unit at zoom level 1.0.
    ConstantScale { pixels_per_unit: f32 },
    /// Scale so that the area displayed at zoom level 1.0
    /// is the same regardless of viewport size.
    ConstantDisplayArea { width: f32, height: f32 },
}

pub struct Camera2D<C: CameraController> {
    pub controller: C,
    pub strategy: ScalingStrategy,
    scaling_factor: f32,
    viewport_scaling: uv::Vec2,
}

impl<C: CameraController> Camera2D<C> {
    pub fn new(controller: C, strategy: ScalingStrategy) -> Self {
        let mut him = Camera2D {
            controller,
            strategy,
            scaling_factor: 0.0,
            viewport_scaling: uv::Vec2::zero(),
        };
        him.update_scaling();
        him
    }

    pub fn with_framebuffer_size(
        controller: C,
        strategy: ScalingStrategy,
        framebuffer_size: (u32, u32),
    ) -> Self {
        let mut him = Camera2D {
            controller,
            strategy,
            scaling_factor: 0.0,
            viewport_scaling: uv::Vec2::zero(),
        };
        him.update_scaling_for_viewport(framebuffer_size);
        him
    }

    /// Update the camera's scaling factor based on the viewport of the game window.
    pub fn update_scaling(&mut self) {
        self.update_scaling_for_viewport(
            super::Context::get().display.get_framebuffer_dimensions(),
        );
    }

    /// Update the camera's scaling factor based on an arbitrary framebuffer size.
    pub fn update_scaling_for_viewport(&mut self, framebuffer_size: (u32, u32)) {
        let (fb_width, fb_height) = framebuffer_size;
        self.viewport_scaling = uv::Vec2::new(2.0 / fb_width as f32, 2.0 / fb_height as f32);

        use self::ScalingStrategy::*;
        self.scaling_factor = match self.strategy {
            ConstantScale { pixels_per_unit } => pixels_per_unit,
            ConstantDisplayArea { width, height } => {
                (fb_width as f32 / width).min(fb_height as f32 / height)
            }
        }
    }

    /// Get the scaling factor that determines how many pixels each unit takes on screen at 1.0 zoom.
    pub fn scaling_factor(&self) -> f32 {
        self.scaling_factor
    }

    /// Get the 3x3 view transformation matrix for this camera.
    pub fn view_matrix(&self) -> uv::Mat3 {
        let full_scaling = self.viewport_scaling * self.scaling_factor;

        uv::Mat3::from_nonuniform_scale_homogeneous(uv::Vec3::new(
            full_scaling.x,
            full_scaling.y,
            1.0,
        )) * self
            .controller
            .transform()
            .0
            .inversed()
            .into_homogeneous_matrix()
    }
}

/// A camera controller that can be dragged with the mouse.
pub struct MouseDragController {
    pub transform: Transform,
    pub zoom_speed: f32,
    pub min_zoom_out: f32,
    pub max_zoom_out: f32,
    drag_start: Option<Transform>,
}

impl MouseDragController {
    pub fn new(transform: Transform) -> Self {
        MouseDragController {
            transform,
            zoom_speed: 0.01,
            min_zoom_out: 0.1,
            max_zoom_out: 10.0,
            drag_start: None,
        }
    }

    /// Update the camera's position using cached drag state.
    pub fn update_position(&mut self, input_cache: &InputCache, scaling_factor: f32) {
        match (input_cache.drag_state(), self.drag_start) {
            (None, _) => self.drag_start = None,
            (Some(DragState::InProgress { .. }), None) => self.drag_start = Some(self.transform),
            (Some(DragState::InProgress { start, .. }), Some(tr_at_start)) => {
                let cursor_pos = input_cache.cursor_position().get();
                let offset = uv::Vec2::new(
                    (cursor_pos.x - start.x) as f32,
                    -(cursor_pos.y - start.y) as f32,
                );
                self.transform = tr_at_start;
                self.transform
                    .0
                    .append_translation(-offset * self.transform.0.scale / scaling_factor);
            }
            (Some(DragState::Completed { start, end, .. }), Some(tr_at_start)) => {
                let offset = uv::Vec2::new((end.x - start.x) as f32, -(end.y - start.y) as f32);
                self.transform = tr_at_start;
                self.transform
                    .0
                    .append_translation(-offset * self.transform.0.scale / scaling_factor);
                self.drag_start = None;
            }
            _ => (),
        }

        let scroll = input_cache.scroll_delta();
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_scaling = (1.0 + scroll * -self.zoom_speed) * self.transform.0.scale;
            let new_scaling = new_scaling.max(self.min_zoom_out).min(self.max_zoom_out);
            self.transform.0.scale = new_scaling;
        }
    }
}

impl CameraController for MouseDragController {
    fn transform(&self) -> &Transform {
        &self.transform
    }
}
