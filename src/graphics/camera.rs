use crate::{
    input::{DragState, InputCache},
    math::{self as m, uv},
};

/// A camera determines the area of space to draw when rendering.
pub trait Camera {
    /// Generate a view matrix for rendering.
    fn view_matrix(&self, viewport_size: (u32, u32)) -> uv::DMat3;
    /// Map a point from screen space to world space.
    fn point_screen_to_world(&self, viewport_size: (u32, u32), point: m::Vec2) -> m::Vec2;
}

/// Tells a camera how to adapt to changing viewport size.
pub enum ScalingStrategy {
    /// Scale so that there are a constant number of viewport pixels
    /// per world coordinate unit at zoom level 1.0.
    ConstantScale { pixels_per_unit: f64 },
    /// Scale so that the area displayed at zoom level 1.0
    /// is the same regardless of viewport size.
    ConstantDisplayArea { width: f64, height: f64 },
}

impl ScalingStrategy {
    /// Get the uniform scaling factor that will result in the desired field of view
    /// in the given viewport size.
    pub fn scaling_factor(&self, viewport_size: (u32, u32)) -> f64 {
        let (vp_w, vp_h) = viewport_size;
        match self {
            ScalingStrategy::ConstantScale { pixels_per_unit } => *pixels_per_unit,
            ScalingStrategy::ConstantDisplayArea { width, height } => {
                (vp_w as f64 / width).min(vp_h as f64 / height)
            }
        }
    }

    /// Get a nonuniform scaling vector to scale the camera's view to the given viewport.
    pub fn scaling(&self, viewport_size: (u32, u32)) -> m::Vec2 {
        let (vp_w, vp_h) = viewport_size;
        let vp_scaling = m::Vec2::new(2.0 / vp_w as f64, 2.0 / vp_h as f64);

        self.scaling_factor(viewport_size) * vp_scaling
    }
}

pub struct MouseDragCamera {
    pub scaling_strategy: ScalingStrategy,
    pub pose: uv::DSimilarity2,
    pub zoom_speed: f64,
    pub min_zoom_out: f64,
    pub max_zoom_out: f64,
    drag_start: Option<uv::DSimilarity2>,
}

impl MouseDragCamera {
    pub fn new(scaling_strategy: ScalingStrategy) -> Self {
        MouseDragCamera {
            scaling_strategy,
            pose: uv::DSimilarity2::identity(),
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
            (Some(DragState::InProgress { .. }), None) => self.drag_start = Some(self.pose),
            (Some(DragState::InProgress { start, .. }), Some(pose_at_start)) => {
                let cursor_pos = input_cache.cursor_position().get();
                let offset = m::Vec2::new(
                    (cursor_pos.x - start.x) as f64,
                    -(cursor_pos.y - start.y) as f64,
                );
                self.pose = pose_at_start;
                self.pose
                    .append_translation(-offset * self.pose.scale / scaling_factor);
            }
            (Some(DragState::Completed { start, end, .. }), Some(pose_at_start)) => {
                let offset = m::Vec2::new((end.x - start.x) as f64, -(end.y - start.y) as f64);
                self.pose = pose_at_start;
                self.pose
                    .append_translation(-offset * self.pose.scale / scaling_factor);
                self.drag_start = None;
            }
            _ => (),
        }

        let scroll = input_cache.scroll_delta();
        if scroll != 0.0 {
            // TODO: zoom towards mouse cursor
            let new_scaling = (1.0 + scroll * -self.zoom_speed) * self.pose.scale;
            let new_scaling = new_scaling.max(self.min_zoom_out).min(self.max_zoom_out);
            self.pose.scale = new_scaling;
        }
    }
}

impl Camera for MouseDragCamera {
    fn view_matrix(&self, viewport_size: (u32, u32)) -> uv::DMat3 {
        let vp_scaling = uv::DMat3::from_nonuniform_scale_homogeneous(
            self.scaling_strategy.scaling(viewport_size),
        );
        let my_transform_inv = self.pose.inversed().into_homogeneous_matrix();
        vp_scaling * my_transform_inv
    }

    fn point_screen_to_world(
        &self,
        viewport_size: (u32, u32),
        point_screenspace: m::Vec2,
    ) -> m::Vec2 {
        let pixels_per_unit = self.scaling_strategy.scaling_factor(viewport_size);
        let half_vp_diag = m::Vec2::new(viewport_size.0 as f64 / 2.0, viewport_size.1 as f64 / 2.0);
        let point_screen_wrt_center = {
            let p = point_screenspace - half_vp_diag;
            m::Vec2::new(p.x, -p.y)
        };

        self.pose * (point_screen_wrt_center / pixels_per_unit)
    }
}
