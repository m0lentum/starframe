use crate::math::{self as m, uv};

mod mouse_drag;
pub use mouse_drag::MouseDragCameraController;

/// A camera determines the area of space to draw when rendering.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub scaling_strategy: CameraScalingStrategy,
    pub transform: m::Transform,
}

impl Camera {
    pub fn new(scaling_strategy: CameraScalingStrategy) -> Self {
        Self {
            scaling_strategy,
            transform: m::Transform::default(),
        }
    }

    pub fn view_matrix(&self, viewport_size: (u32, u32)) -> uv::DMat3 {
        let vp_scaling = uv::DMat3::from_nonuniform_scale_homogeneous(
            self.scaling_strategy.scaling(viewport_size),
        );
        let my_transform_inv = self.transform.inversed().into_homogeneous_matrix();
        vp_scaling * my_transform_inv
    }

    /// Transform a point from camera space into world space.
    pub fn point_screen_to_world(
        &self,
        viewport_size: (u32, u32),
        point_screen: m::Vec2,
    ) -> m::Vec2 {
        let pixels_per_unit = self.scaling_strategy.scaling_factor(viewport_size);
        let half_vp_diag = m::Vec2::new(viewport_size.0 as f64 / 2.0, viewport_size.1 as f64 / 2.0);
        let point_screen_wrt_center = {
            let p = point_screen - half_vp_diag;
            m::Vec2::new(p.x, -p.y)
        };

        self.transform * (point_screen_wrt_center / pixels_per_unit)
    }

    /// Transform a displacement vector from camera space into world space.
    pub fn vector_screen_to_world(
        &self,
        viewport_size: (u32, u32),
        vec_screen: m::Vec2,
    ) -> m::Vec2 {
        let y_flipped = m::Vec2::new(vec_screen.x, -vec_screen.y);
        let pixels_per_unit = self.scaling_strategy.scaling_factor(viewport_size);
        self.transform.scale * y_flipped / pixels_per_unit
    }
}

/// Tells a camera how to adapt to changing viewport size.
#[derive(Clone, Copy, Debug)]
pub enum CameraScalingStrategy {
    /// Scale so that there are a constant number of viewport pixels
    /// per world coordinate unit at zoom level 1.0.
    ConstantScale { pixels_per_unit: f64 },
    /// Scale so that the area displayed at zoom level 1.0
    /// is the same regardless of viewport size.
    ConstantDisplayArea { width: f64, height: f64 },
}

impl CameraScalingStrategy {
    /// Get the uniform scaling factor that will result in the desired field of view
    /// in the given viewport size.
    pub fn scaling_factor(&self, viewport_size: (u32, u32)) -> f64 {
        let (vp_w, vp_h) = viewport_size;
        match self {
            CameraScalingStrategy::ConstantScale { pixels_per_unit } => *pixels_per_unit,
            CameraScalingStrategy::ConstantDisplayArea { width, height } => {
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
