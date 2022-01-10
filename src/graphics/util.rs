//! Utilities for communicating with the GPU.

use crate::math::{self as m, uv};
use zerocopy::{AsBytes, FromBytes};

/// Utility type to convert transform matrices to a form with appropriate padding
/// for sending to the GPU.
#[derive(Clone, Copy, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuMat3([[f32; 4]; 3]);

impl From<uv::DMat3> for GpuMat3 {
    fn from(mat: uv::DMat3) -> Self {
        let ma = mat.as_array();
        // converting to f32 here for cheaper work on the gpu
        GpuMat3([
            [ma[0] as f32, ma[1] as f32, ma[2] as f32, 0.0],
            [ma[3] as f32, ma[4] as f32, ma[5] as f32, 0.0],
            [ma[6] as f32, ma[7] as f32, ma[8] as f32, 0.0],
        ])
    }
}

/// Utility type for putting 2D vectors in vertex buffers.
#[derive(Clone, Copy, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec2([f32; 2]);

impl From<m::Vec2> for GpuVec2 {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32])
    }
}

/// Utility type for putting 2D vectors in uniform buffers.
#[derive(Clone, Copy, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec2Padded([f32; 4]);

impl From<m::Vec2> for GpuVec2Padded {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32, 0.0, 0.0])
    }
}

/// A wgpu Buffer managed like a Vec, i.e. automatically reallocated
/// when it can't fit its contents.
///
/// Useful for batched drawing of things whose number varies over time.
pub struct DynamicBuffer {
    buf: Option<wgpu::Buffer>,
    len: usize,
    size: u64,
    label: Option<&'static str>,
    usage: wgpu::BufferUsages,
}

impl DynamicBuffer {
    /// Create a new dynamic buffer. Appends `wgpu::BufferUsages::COPY_DST` to the given `usage`,
    /// so you only need to provide the usage for the desired buffer type.
    pub fn new(label: Option<&'static str>, usage: wgpu::BufferUsages) -> Self {
        Self {
            buf: None,
            len: 0,
            size: 0,
            label,
            usage: usage | wgpu::BufferUsages::COPY_DST,
        }
    }

    /// Reallocate the buffer if needed, then write the given data to it.
    pub fn write<Data: AsBytes>(&mut self, ctx: &super::RenderContext, data: &[Data]) {
        self.len = data.len();
        let size_needed = data.len() as u64 * std::mem::size_of::<Data>() as u64;
        if self.buf.is_none() || size_needed > self.size {
            self.buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: self.label,
                size: size_needed,
                usage: self.usage,
                mapped_at_creation: false,
            }));
            self.size = size_needed;
        }
        ctx.queue
            .write_buffer(self.buf.as_ref().unwrap(), 0, data.as_bytes());
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

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A dynamic vertex buffer and index buffer bundled together for batched drawing of general
/// meshes. Also caches the CPU side buffers for reuse and handles padding for the index buffer.
pub struct DynamicMeshBuffers<Vert: AsBytes> {
    pub vertices: Vec<Vert>,
    vert_buf: DynamicBuffer,
    pub indices: Vec<u16>,
    /// indices might have to be padded, store len without padding
    unpadded_idx_len: u32,
    idx_buf: DynamicBuffer,
}

impl<Vert: AsBytes> DynamicMeshBuffers<Vert> {
    pub fn new(label: Option<&'static str>) -> Self {
        Self {
            vertices: Vec::new(),
            vert_buf: DynamicBuffer::new(label, wgpu::BufferUsages::VERTEX),
            indices: Vec::new(),
            unpadded_idx_len: 0,
            idx_buf: DynamicBuffer::new(
                label,
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            ),
        }
    }

    /// Write the data from `self.vertices` and `self.indices` to the GPU.
    pub fn write(&mut self, ctx: &super::RenderContext) {
        // pad the index buffer to 4 bytes
        self.unpadded_idx_len = self.indices.len() as u32;
        self.indices
            .resize(self.indices.len() + self.indices.len() % 2, 0);
        self.vert_buf.write(ctx, &self.vertices);
        self.idx_buf.write(ctx, &self.indices);
    }

    /// Clear `self.vertices` and `self.indices`.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Extend the vertex and index buffers in one go, shifting indices appropriately.
    pub fn extend(
        &mut self,
        vertices: impl IntoIterator<Item = Vert>,
        indices: impl IntoIterator<Item = u16>,
    ) {
        let root_idx = self.vertices.len() as u16;
        self.vertices.extend(vertices);
        self.indices
            .extend(indices.into_iter().map(|idx| root_idx + idx));
    }

    /// Set a render pass to draw from the contained buffers.
    pub fn set_buffers<'pr, 'p, 's: 'p>(&'s self, pass: &'pr mut wgpu::RenderPass<'p>) {
        pass.set_vertex_buffer(0, self.vert_buf.slice());
        pass.set_index_buffer(self.idx_buf.slice(), wgpu::IndexFormat::Uint16);
    }

    /// Get the range of indices that have been written to the index buffer.
    /// Use with [`RenderPass::draw_indexed`][wgpu::RenderPass::draw_indexed].
    pub fn index_range(&self) -> std::ops::Range<u32> {
        0..self.unpadded_idx_len
    }
}
