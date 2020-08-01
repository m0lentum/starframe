//! Utilities for communicating with the GPU.

use crate::math as m;
use zerocopy::{AsBytes, FromBytes};

/// Utility type to convert transform matrices to a form that can
/// be sent to a shader.
///
/// Note: a GLSL `mat3` is actually three `vec4`s in memory.
#[derive(Clone, Copy, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GlslMat3([[f32; 4]; 3]);

impl From<m::Mat3> for GlslMat3 {
    fn from(mat: m::Mat3) -> Self {
        GlslMat3(mat.insert_row(3, 0.0).into())
    }
}
