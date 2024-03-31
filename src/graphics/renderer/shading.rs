use std::{borrow::Cow, mem::size_of};
use zerocopy::{AsBytes, FromBytes};

use super::GBuffers;
use crate::math::uv;

#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    pub direct_color: [f32; 3],
    pub ambient_color: [f32; 3],
    pub direction: uv::Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direct_color: [1.0, 1.0, 1.0],
            ambient_color: [1.0, 1.0, 1.0],
            direction: uv::Vec3::new(0.0, 0.0, 1.0),
        }
    }
}

pub struct ShadingPipeline {
    pipeline: wgpu::RenderPipeline,
    gbufs_bind_group_layout: wgpu::BindGroupLayout,
    // bind group must be recreated when the window size changes
    pub(super) gbufs_bind_group: wgpu::BindGroup,
    light_buf: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct LightUniforms {
    direct_color: [f32; 3],
    _pad0: u32,
    ambient_color: [f32; 3],
    _pad1: u32,
    direction: [f32; 3],
    _pad2: u32,
}

impl From<crate::DirectionalLight> for LightUniforms {
    fn from(l: crate::DirectionalLight) -> Self {
        Self {
            direct_color: l.direct_color,
            _pad0: 0,
            ambient_color: l.ambient_color,
            _pad1: 0,
            direction: l.direction.normalized().into(),
            _pad2: 0,
        }
    }
}

impl ShadingPipeline {
    pub fn new(gbufs: &GBuffers) -> Self {
        let device = super::Renderer::device();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred shading"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../shaders/deferred_shade.wgsl"
            ))),
        });

        // gbuffer bind group

        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let gbufs_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("deferred shading gbuffers"),
                entries: &[
                    tex_entry(0),
                    tex_entry(1),
                    tex_entry(2),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });
        let gbufs_bind_group = Self::create_gbufs_bind_group(&gbufs_bind_group_layout, gbufs);

        // light

        let light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<LightUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh lights"),
            mapped_at_creation: false,
        });

        let light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<LightUniforms>() as _),
                    },
                    count: None,
                }],
                label: Some("deferred lights"),
            });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deferred lights"),
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            }],
        });

        // pipeline

        let pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("deferred shading"),
            bind_group_layouts: &[&gbufs_bind_group_layout, &light_bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deferred shading"),
            layout: Some(&pl_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(super::SWAPCHAIN_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Self {
            pipeline,
            gbufs_bind_group_layout,
            gbufs_bind_group,
            light_buf,
            light_bind_group,
        }
    }

    fn create_gbufs_bind_group(
        layout: &wgpu::BindGroupLayout,
        gbufs: &GBuffers,
    ) -> wgpu::BindGroup {
        let device = super::Renderer::device();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deferred shading gbuffers"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&gbufs.position.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gbufs.normal.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&gbufs.albedo.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&gbufs.sampler),
                },
            ],
        })
    }

    pub(super) fn update_gbufs_bind_group(&mut self, gbufs: &GBuffers) {
        self.gbufs_bind_group = Self::create_gbufs_bind_group(&self.gbufs_bind_group_layout, gbufs);
    }

    pub fn draw<'pass, 's: 'pass>(
        &'s self,
        pass: &mut wgpu::RenderPass<'pass>,
        light: crate::DirectionalLight,
    ) {
        let queue = super::Renderer::queue();

        let light_unif = LightUniforms::from(light);
        queue.write_buffer(&self.light_buf, 0, light_unif.as_bytes());

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.gbufs_bind_group, &[]);
        pass.set_bind_group(1, &self.light_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
