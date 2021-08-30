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

/// A wgpu Buffer managed like a Vec, i.e. automatically reallocated
/// when it can't fit its contents.
///
/// Useful for batched drawing of things whose number varies over time.
pub struct DynamicVertexBuffer {
    buf: Option<wgpu::Buffer>,
    len: usize,
    size: u64,
    label: Option<&'static str>,
}

impl DynamicVertexBuffer {
    pub fn new(label: Option<&'static str>) -> Self {
        Self {
            buf: None,
            len: 0,
            size: 0,
            label,
        }
    }

    /// Reallocate the buffer if needed, then write the given vertices to it.
    pub fn write<Vert: AsBytes>(&mut self, ctx: &super::RenderContext, verts: &[Vert]) {
        self.len = verts.len();
        let size_needed = verts.len() as u64 * std::mem::size_of::<Vert>() as u64;
        if self.buf.is_none() || size_needed > self.size {
            self.buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: self.label,
                size: size_needed,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.size = size_needed;
        }
        ctx.queue
            .write_buffer(self.buf.as_ref().unwrap(), 0, verts.as_bytes());
    }

    /// Get a slice of the entire buffer.
    /// # Panics
    /// Panics if the buffer has never been written to.
    pub fn slice(&self) -> wgpu::BufferSlice {
        self.buf.as_ref().unwrap().slice(..)
    }

    /// Get the number of vertices written to the buffer.
    pub fn len(&self) -> usize {
        self.len
    }
}
