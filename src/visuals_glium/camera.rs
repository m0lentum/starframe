use crate::util::{
    inputcache::{DragState, InputCache},
    Transform,
};

use nalgebra::{Matrix3, Vector2};

/// A 2D camera is anything that can generate a 3x3 view matrix
/// to determine screen position of rendered objects.
pub trait Camera2D {
    fn view_matrix(&self, framebuffer_size: (u32, u32)) -> Matrix3<f32>;
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

/// A 2D camera that isn't in any way attached to a game object
/// and can thus only be manipulated directly by the user.
pub struct SimpleCamera2D {
    pub transform: Transform,
    pub strategy: ScalingStrategy,
}

impl SimpleCamera2D {
    pub fn new(transform: Transform, strategy: ScalingStrategy) -> Self {
        SimpleCamera2D {
            transform,
            strategy,
        }
    }
}

impl SimpleCamera2D {
    pub fn scaling_factor(&self, framebuffer_size: (u32, u32)) -> f32 {
        let (fb_width, fb_height) = framebuffer_size;
        use self::ScalingStrategy::*;
        match self.strategy {
            ConstantScale { pixels_per_unit } => pixels_per_unit,
            ConstantDisplayArea { width, height } => {
                (fb_width as f32 / width).min(fb_height as f32 / height)
            }
        }
    }
}

impl Camera2D for SimpleCamera2D {
    fn view_matrix(&self, framebuffer_size: (u32, u32)) -> Matrix3<f32> {
        let (fb_width, fb_height) = framebuffer_size;
        let viewport_scaling = Vector2::new(2.0 / fb_width as f32, 2.0 / fb_height as f32);
        let full_scaling = viewport_scaling * self.scaling_factor(framebuffer_size);

        Matrix3::new_nonuniform_scaling(&full_scaling) * self.transform.inverse().to_homogeneous()
    }
}

/// A wrapper around SimpleCamera2D that adds movement by dragging with the mouse.
/// Mostly for use in testing, not very sophisticated or customizable at this time.
pub struct MouseDragCamera2D {
    pub camera: SimpleCamera2D,
    pub zoom_speed: f32,
    pub min_zoom_out: f32,
    pub max_zoom_out: f32,
    drag_start: Option<Transform>,
}

impl MouseDragCamera2D {
    pub fn new(transform: Transform, strategy: ScalingStrategy) -> Self {
        Self::from_simple(SimpleCamera2D::new(transform, strategy))
    }

    pub fn from_simple(camera: SimpleCamera2D) -> Self {
        MouseDragCamera2D {
            camera,
            zoom_speed: 0.01,
            min_zoom_out: 0.1,
            max_zoom_out: 10.0,
            drag_start: None,
        }
    }

    /// Update the camera's position using cached drag state.
    pub fn update(&mut self, input_cache: &InputCache, framebuffer_size: (u32, u32)) {
        match (input_cache.drag_state(), self.drag_start) {
            (None, _) => self.drag_start = None,
            (Some(DragState::InProgress { .. }), None) => {
                self.drag_start = Some(self.camera.transform)
            }
            (Some(DragState::InProgress { start, .. }), Some(tr_at_start)) => {
                let cursor_pos = input_cache.cursor_position().get();
                let offset = Vector2::new(
                    (cursor_pos.x - start.x) as f32,
                    -(cursor_pos.y - start.y) as f32,
                );
                // TODO: add stuff to the Transform interface to streamline this
                self.camera.transform = tr_at_start;
                self.camera.transform.translate(
                    -offset * self.camera.transform.scaling()
                        / self.camera.scaling_factor(framebuffer_size),
                );
            }
            (Some(DragState::Completed { start, end, .. }), Some(tr_at_start)) => {
                let offset = Vector2::new((end.x - start.x) as f32, -(end.y - start.y) as f32);
                self.camera.transform = tr_at_start;
                self.camera.transform.translate(
                    -offset * self.camera.transform.scaling()
                        / self.camera.scaling_factor(framebuffer_size),
                );
                self.drag_start = None;
            }
            _ => (),
        }

        let scroll = input_cache.scroll_delta();
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_scaling = (1.0 + scroll * -self.zoom_speed) * self.camera.transform.scaling();
            let new_scaling = new_scaling.max(self.min_zoom_out).min(self.max_zoom_out);
            self.camera.transform.set_scaling(new_scaling);
        }
    }
}

impl Camera2D for MouseDragCamera2D {
    fn view_matrix(&self, framebuffer_size: (u32, u32)) -> Matrix3<f32> {
        self.camera.view_matrix(framebuffer_size)
    }
}
