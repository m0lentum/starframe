use std::mem::size_of;

use zerocopy::{AsBytes, FromBytes};

use crate::graphics::util::GpuVec4;

/// Map definining additional lighting from off-screen sources.
pub struct EnvironmentMap {
    needs_upload: bool,
    ambient_color: [f32; 3],

    // buffer containing the ambient color and directional lights for shading
    // (directional lights are used directly for background objects
    // outside of the GI field of influence,
    // otherwise lights are baked into the environment texture
    // and included during GI computation)
    pub(super) render_buf: wgpu::Buffer,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
pub(super) struct RenderData {
    ambient_color: GpuVec4,
}

impl Default for EnvironmentMap {
    fn default() -> Self {
        let device = crate::Renderer::device();
        let render_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment map render data"),
            size: size_of::<RenderData>() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        Self {
            needs_upload: true,
            ambient_color: [0.; 3],
            render_buf,
        }
    }
}

impl EnvironmentMap {
    pub fn set_ambient_light(&mut self, color: [f32; 3]) {
        if self.ambient_color != color {
            self.ambient_color = color;
            self.needs_upload = true;
        }
    }

    pub(crate) fn bake(&mut self) {
        if self.needs_upload {
            let queue = crate::Renderer::queue();

            let render_data = RenderData {
                ambient_color: self.ambient_color.into(),
            };
            queue.write_buffer(&self.render_buf, 0, render_data.as_bytes());
        }
    }
}
