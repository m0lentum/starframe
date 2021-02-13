//! Utilities for communicating with the GPU.

use crate::math::uv;
use zerocopy::{AsBytes, FromBytes};

/// Utility type to convert transform matrices to a form that can
/// be sent to a shader.
///
/// Note: a GLSL `mat3` is actually three `vec4`s in memory.
#[derive(Clone, Copy, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GlslMat3([[f32; 4]; 3]);

impl From<uv::DMat3> for GlslMat3 {
    fn from(mat: uv::DMat3) -> Self {
        let ma = mat.as_array();
        // converting to f32 here for cheaper work on the gpu
        GlslMat3([
            [ma[0] as f32, ma[1] as f32, ma[2] as f32, 0.0],
            [ma[3] as f32, ma[4] as f32, ma[5] as f32, 0.0],
            [ma[6] as f32, ma[7] as f32, ma[8] as f32, 0.0],
        ])
    }
}
