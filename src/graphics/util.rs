//! Utilities for communicating with the GPU.

use crate::math::{self as m, uv};
use zerocopy::{AsBytes, FromBytes};

//
// GPU interface types
//

/// Type for sending 3x3 matrices to the GPU with appropriate padding.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuMat3(pub [[f32; 4]; 3]);

impl From<uv::DMat3> for GpuMat3 {
    fn from(mat: uv::DMat3) -> Self {
        let ma = mat.as_array();
        Self([
            [ma[0] as f32, ma[1] as f32, ma[2] as f32, 0.0],
            [ma[3] as f32, ma[4] as f32, ma[5] as f32, 0.0],
            [ma[6] as f32, ma[7] as f32, ma[8] as f32, 0.0],
        ])
    }
}

/// Type for sending 4x4 matrices to the GPU.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuMat4(pub [[f32; 4]; 4]);

impl From<[[f32; 4]; 4]> for GpuMat4 {
    fn from(m: [[f32; 4]; 4]) -> Self {
        Self(m)
    }
}

impl From<uv::Mat4> for GpuMat4 {
    fn from(m: uv::Mat4) -> Self {
        Self(m.cols.map(|c| *c.as_array()))
    }
}

/// Type for putting 2D vectors in vertex buffers.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec2(pub [f32; 2]);

impl From<m::Vec2> for GpuVec2 {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32])
    }
}

impl From<[f32; 2]> for GpuVec2 {
    fn from(v: [f32; 2]) -> Self {
        Self(v)
    }
}

/// Type for putting 2D vectors in uniform buffers.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec2Padded(pub [f32; 4]);

impl From<m::Vec2> for GpuVec2Padded {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32, 0.0, 0.0])
    }
}

impl From<[f32; 2]> for GpuVec2Padded {
    fn from(v: [f32; 2]) -> Self {
        Self([v[0], v[1], 0.0, 0.0])
    }
}

/// Type for putting 3D vectors in vertex buffers.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec3(pub [f32; 3]);

impl From<m::Vec2> for GpuVec3 {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32, 0.0])
    }
}

impl From<[f32; 2]> for GpuVec3 {
    fn from(v: [f32; 2]) -> Self {
        Self([v[0], v[1], 0.0])
    }
}

impl From<uv::Vec3> for GpuVec3 {
    fn from(v: uv::Vec3) -> Self {
        Self([v.x, v.y, v.z])
    }
}

impl From<[f32; 3]> for GpuVec3 {
    fn from(v: [f32; 3]) -> Self {
        Self(v)
    }
}

/// Type for sending 4D vectors (like colors) to the GPU.
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
#[repr(transparent)]
pub struct GpuVec4(pub [f32; 4]);

impl From<m::Vec2> for GpuVec4 {
    fn from(v: m::Vec2) -> Self {
        Self([v.x as f32, v.y as f32, 0.0, 0.0])
    }
}

impl From<[f32; 2]> for GpuVec4 {
    fn from(v: [f32; 2]) -> Self {
        Self([v[0], v[1], 0.0, 0.0])
    }
}

impl From<uv::Vec3> for GpuVec4 {
    fn from(v: uv::Vec3) -> Self {
        Self([v.x, v.y, v.z, 0.0])
    }
}

impl From<[f32; 3]> for GpuVec4 {
    fn from(v: [f32; 3]) -> Self {
        Self([v[0], v[1], v[2], 0.0])
    }
}

impl From<uv::Vec4> for GpuVec4 {
    fn from(v: uv::Vec4) -> Self {
        Self([v.x, v.y, v.z, v.w])
    }
}

impl From<[f32; 4]> for GpuVec4 {
    fn from(v: [f32; 4]) -> Self {
        Self(v)
    }
}

//
// dynamic buffers
//

/// A wgpu Buffer managed like a Vec, i.e. automatically reallocated
/// when it can't fit its contents.
///
/// Useful for batched drawing of things whose number varies over time.
#[derive(Debug)]
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
    #[inline]
    pub fn write<Data: AsBytes>(&mut self, ctx: &super::RenderContext, data: &[Data]) {
        self.write_split_borrow(ctx.device, ctx.queue, data)
    }

    /// Like [`write`][Self::write], but takes the required members of
    /// [`RenderContext`][super::RenderContext] to facilitate partial borrows.
    pub fn write_split_borrow<Data: AsBytes>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[Data],
    ) {
        self.len = data.len();
        let size_needed = data.len() as u64 * std::mem::size_of::<Data>() as u64;
        if self.buf.is_none() || size_needed > self.size {
            self.buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: self.label,
                size: size_needed,
                usage: self.usage,
                mapped_at_creation: false,
            }));
            self.size = size_needed;
        }
        queue.write_buffer(self.buf.as_ref().unwrap(), 0, data.as_bytes());
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
