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

impl From<uv::Mat3> for GlslMat3 {
    fn from(mat: uv::Mat3) -> Self {
        let ma = mat.as_array();
        GlslMat3([
            [ma[0], ma[1], ma[2], 0.0],
            [ma[3], ma[4], ma[5], 0.0],
            [ma[6], ma[7], ma[8], 0.0],
        ])
    }
}
