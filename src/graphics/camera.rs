use crate::core::{
    inputcache::{DragState, InputCache},
    math as m, Transform,
};

use nalgebra as na;

pub trait Camera {
    fn view_matrix(&self, viewport_size: (u32, u32)) -> m::Mat3;
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

impl ScalingStrategy {
    /// Get the uniform scaling factor that will result in the desired field of view
    /// in the given viewport size.
    pub fn scaling_factor(&self, viewport_size: (u32, u32)) -> f32 {
        let (vp_w, vp_h) = viewport_size;
        match self {
            ScalingStrategy::ConstantScale { pixels_per_unit } => *pixels_per_unit,
            ScalingStrategy::ConstantDisplayArea { width, height } => {
                (vp_w as f32 / width).min(vp_h as f32 / height)
            }
        }
    }

    /// Get a nonuniform scaling vector to scale the camera's view to the given viewport.
    pub fn scaling(&self, viewport_size: (u32, u32)) -> m::Vec2 {
        let (vp_w, vp_h) = viewport_size;
        let vp_scaling = m::Vec2::new(2.0 / vp_w as f32, 2.0 / vp_h as f32);

        self.scaling_factor(viewport_size) * vp_scaling
    }
}

pub struct MouseDragCamera {
    pub scaling_strategy: ScalingStrategy,
    pub transform: Transform,
    pub zoom_speed: f32,
    pub min_zoom_out: f32,
    pub max_zoom_out: f32,
    drag_start: Option<Transform>,
}

impl MouseDragCamera {
    pub fn new(scaling_strategy: ScalingStrategy) -> Self {
        MouseDragCamera {
            scaling_strategy,
            transform: Transform::identity(),
            zoom_speed: 0.01,
            min_zoom_out: 0.1,
            max_zoom_out: 10.0,
            drag_start: None,
        }
    }

    /// Update the camera's position using cached drag state.
    ///
    /// Viewport size is needed to scale mouse movements to the right size of camera movements.
    pub fn update(&mut self, input_cache: &InputCache, viewport_size: (u32, u32)) {
        let scaling_factor = self.scaling_strategy.scaling_factor(viewport_size);
        match (input_cache.drag_state(), self.drag_start) {
            (None, _) => self.drag_start = None,
            (Some(DragState::InProgress { .. }), None) => self.drag_start = Some(self.transform),
            (Some(DragState::InProgress { start, .. }), Some(tr_at_start)) => {
                let cursor_pos = input_cache.cursor_position().get();
                let offset = m::Vec2::new(
                    (cursor_pos.x - start.x) as f32,
                    -(cursor_pos.y - start.y) as f32,
                );
                self.transform = tr_at_start;
                self.transform
                    .append_translation_mut(&na::Translation2::from(
                        -offset * self.transform.scaling() / scaling_factor,
                    ));
            }
            (Some(DragState::Completed { start, end, .. }), Some(tr_at_start)) => {
                let offset = m::Vec2::new((end.x - start.x) as f32, -(end.y - start.y) as f32);
                self.transform = tr_at_start;
                self.transform
                    .append_translation_mut(&na::Translation2::from(
                        -offset * self.transform.scaling() / scaling_factor,
                    ));
                self.drag_start = None;
            }
            _ => (),
        }

        let scroll = input_cache.scroll_delta();
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_scaling = (1.0 + scroll * -self.zoom_speed) * self.transform.scaling();
            let new_scaling = new_scaling.max(self.min_zoom_out).min(self.max_zoom_out);
            self.transform.set_scaling(new_scaling);
        }
    }
}

impl Camera for MouseDragCamera {
    fn view_matrix(&self, viewport_size: (u32, u32)) -> m::Mat3 {
        let vp_scaling =
            m::Mat3::new_nonuniform_scaling(&self.scaling_strategy.scaling(viewport_size));
        let my_transform_inv = self.transform.inverse().to_homogeneous();
        vp_scaling * my_transform_inv
    }
}
