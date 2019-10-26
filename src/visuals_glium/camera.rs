use crate::util::Transform;

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

impl Camera2D for SimpleCamera2D {
    fn view_matrix(&self, framebuffer_size: (u32, u32)) -> Matrix3<f32> {
        let (fb_width, fb_height) = framebuffer_size;
        let viewport_scaling = Vector2::new(2.0 / fb_width as f32, 2.0 / fb_height as f32);

        use self::ScalingStrategy::*;
        let additional_scaling_factor = match self.strategy {
            ConstantScale { pixels_per_unit } => pixels_per_unit,
            ConstantDisplayArea { width, height } => {
                (fb_width as f32 / width).min(fb_height as f32 / height)
            }
        };
        let full_scaling = viewport_scaling * additional_scaling_factor;

        Matrix3::new_nonuniform_scaling(&full_scaling) * self.transform.inverse().to_homogeneous()
    }
}
