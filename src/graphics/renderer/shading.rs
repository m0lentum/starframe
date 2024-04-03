use std::{borrow::Cow, mem::size_of};

use wgpu::util::DeviceExt;
use zerocopy::{AsBytes, FromBytes};

use super::GBuffers;
use crate::{
    graphics::{mesh::CameraUniforms, util::DynamicBuffer},
    math::{uv, Pose},
};

/// Primary light source covering the entire screen.
/// There can only be one active at a time.
///
/// A directional light has no position,
/// instead casting parallel rays over the entire scene.
/// This emulates a distant, powerful point light source like the sun.
///
/// This light also includes a stylized ambient light,
/// whose color can be configured independently from the direct light color
/// The ambient light is more intense for surfaces facing away from the light,
/// creating a "core shadow" effect.
#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    /// Color of the light itself.
    pub direct_color: [f32; 3],
    /// Color of light applied on every surface
    /// regardless of whether it's hit by the direct light.
    pub ambient_color: [f32; 3],
    /// Direction in which the light rays travel.
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

/// A point-like light source. There can be many in a scene.
///
/// Can be created on its own or attached to entities in a [`hecs::World`].
/// To get all lights in a world, use [`gather_from_world`][Self::gather_from_world].
///
/// The parameters `radius`, `falloff`, and `cutoff` control the shape of the light.
/// For an interactive visualization of the way these work,
/// see [this Desmos calculator](https://www.desmos.com/calculator/f7impirvum).
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    /// Position of the light. Default: origin.
    ///
    /// If the light is attached to an entity
    /// and acquired with [`gather_from_world`][Self::gather_from_world],
    /// the position is interpreted to be relative to the entity's [`Pose`].
    /// Otherwise, this is in world space.
    pub position: uv::Vec3,
    /// Color of the light. Default: white.
    pub color: [f32; 3],
    /// Maximum radius the light can reach. Default: 10.
    ///
    /// At this distance, the light intensity will reach
    /// the cutoff value defined in `cutoff`.
    /// Light reaching further won't be rendered,
    /// so there will be a slight border visible at this distance
    /// unless `cutoff` is set very low.
    pub radius: f32,
    /// Falloff rate of light intensity. Default: 5.
    ///
    /// The higher this is, the faster the light intensity
    /// will decrease with distance.
    pub falloff: f32,
    /// Cutoff value for light intensity at a distance of `radius`.
    pub cutoff: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: uv::Vec3::zero(),
            color: [1.; 3],
            radius: 10.,
            falloff: 5.,
            cutoff: 1. / 256.,
        }
    }
}

impl PointLight {
    /// Get all point lights attached to entities in a world.
    ///
    /// If the entity has a [`Pose`],
    /// the position of the light is set to `pose * light.position`
    /// (i.e. the original light position is interpreted as being in the entity's local space),
    /// otherwise the light is returned as is.
    pub fn gather_from_world(world: &mut hecs::World) -> impl '_ + Iterator<Item = PointLight> {
        world
            .query_mut::<(&PointLight, Option<&Pose>)>()
            .into_iter()
            .map(|(_, (light, pose))| {
                let mut ret = *light;
                if let Some(pose) = pose {
                    let pos_offset =
                        *pose * uv::DVec2::new(ret.position[0] as f64, ret.position[1] as f64);
                    ret.position[0] = pos_offset.x as f32;
                    ret.position[1] = pos_offset.y as f32;
                }
                ret
            })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct DirectLightUniforms {
    direct_color: [f32; 3],
    _pad0: u32,
    ambient_color: [f32; 3],
    _pad1: u32,
    direction: [f32; 3],
    _pad2: u32,
}

impl From<crate::DirectionalLight> for DirectLightUniforms {
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

/// Point lights are drawn as instanced light volumes
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct PointLightInstance {
    position: [f32; 3],
    color: [f32; 3],
    radius: f32,
    attn_linear: f32,
    attn_quadratic: f32,
}

impl From<PointLight> for PointLightInstance {
    fn from(light: PointLight) -> Self {
        // see https://www.desmos.com/calculator/f7impirvum
        let attn_linear = light.falloff / light.radius;
        let intensity = light.color.into_iter().max_by(f32::total_cmp).unwrap();
        let attn_quadratic =
            (1. / light.radius.powi(2)) * ((intensity / light.cutoff) - light.cutoff - attn_linear);
        Self {
            position: light.position.into(),
            color: light.color,
            radius: light.radius,
            attn_linear,
            attn_quadratic,
        }
    }
}

/// Pipeline that performs deferred shading with a single direct light,
/// followed by additional lighting with a set of point lights.
pub struct ShadingPipeline {
    // pipeline for the first pass
    direct_pipeline: wgpu::RenderPipeline,
    gbufs_bind_group_layout: wgpu::BindGroupLayout,
    // bind group must be recreated when the window size changes
    pub(super) gbufs_bind_group: wgpu::BindGroup,
    dir_light_buf: wgpu::Buffer,
    dir_light_bind_group: wgpu::BindGroup,
    // pipeline for the point light pass
    point_pipeline: wgpu::RenderPipeline,
    // vertex and index buffers for a single point light volume
    point_volume_vertex_buf: wgpu::Buffer,
    point_volume_index_buf: wgpu::Buffer,
    point_volume_index_count: u32,
    camera_buf: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    // instance buffer for all the point lights
    point_instance_buf: DynamicBuffer,
}

impl ShadingPipeline {
    pub fn new(gbufs: &GBuffers, msaa_samples: u32) -> Self {
        let device = super::Renderer::device();

        let dir_light_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred shading"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../shaders/deferred_shade.wgsl"
            ))),
        });

        let point_light_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("point lights"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../shaders/point_light.wgsl"
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

        let dir_light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<DirectLightUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh lights"),
            mapped_at_creation: false,
        });

        let dir_light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            size_of::<DirectLightUniforms>() as _
                        ),
                    },
                    count: None,
                }],
                label: Some("directional light"),
            });

        let dir_light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("directional light"),
            layout: &dir_light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: dir_light_buf.as_entire_binding(),
            }],
        });

        // pipeline

        let dir_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("deferred shading"),
            bind_group_layouts: &[&gbufs_bind_group_layout, &dir_light_bind_group_layout],
            push_constant_ranges: &[],
        });
        let direct_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deferred shading"),
            layout: Some(&dir_pl_layout),
            vertex: wgpu::VertexState {
                module: &dir_light_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &dir_light_shader,
                entry_point: "fs_main",
                targets: &[Some(super::SWAPCHAIN_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            // bind the depth texture so it's ready for point lights,
            // but don't do anything with it
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::graphics::renderer::deferred::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        //
        // point light buffers and pipeline
        //

        // unit circle for the light volume (2D is enough for us)
        const VOLUME_POINT_COUNT: usize = 32;
        let angle_incr = std::f32::consts::TAU / VOLUME_POINT_COUNT as f32;
        let point_volume_verts: Vec<[f32; 2]> = (0..VOLUME_POINT_COUNT)
            .map(|i| {
                let (x, y) = (i as f32 * angle_incr).sin_cos();
                [x, y]
            })
            .collect();

        let point_volume_indices: Vec<u16> = (2..VOLUME_POINT_COUNT as u16)
            .flat_map(|i| [0, i - 1, i])
            .collect();

        let point_volume_vertex_buf =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("point light volume vertices"),
                usage: wgpu::BufferUsages::VERTEX,
                contents: point_volume_verts.as_bytes(),
            });

        let point_volume_index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("point light volume indices"),
            usage: wgpu::BufferUsages::INDEX,
            contents: point_volume_indices.as_bytes(),
        });
        let point_volume_index_count = point_volume_indices.len() as u32;

        let point_instance_buf =
            DynamicBuffer::new(Some("point light instance"), wgpu::BufferUsages::VERTEX);

        // camera bind group layout and buffer
        // (TODO: refactor these into one place instead of redoing for every pipeline)

        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<CameraUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh camera"),
            mapped_at_creation: false,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<CameraUniforms>() as _),
                    },
                    count: None,
                }],
                label: Some("camera"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        // pipeline

        let point_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("point lights"),
            bind_group_layouts: &[&gbufs_bind_group_layout, &camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let point_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("point lights"),
            layout: Some(&point_pl_layout),
            vertex: wgpu::VertexState {
                module: &point_light_shader,
                entry_point: "vs_main",
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<[f32; 2]>() as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            // position
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        }],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<PointLightInstance>() as _,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            // light position
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 1,
                            },
                            // light color
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 4 * 3,
                                shader_location: 2,
                            },
                            // light radius
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 4 * 3 * 2,
                                shader_location: 3,
                            },
                            // linear attenuation
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 4 * 3 * 2 + 4,
                                shader_location: 4,
                            },
                            // quadratic attenuation
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 4 * 3 * 2 + 4 + 4,
                                shader_location: 5,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &point_light_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: super::SWAPCHAIN_FORMAT,
                    // accumulate lights on top instead of replacing
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::DstAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::graphics::renderer::deferred::DEPTH_FORMAT,
                // read but don't write the depth buffer,
                // so that lights behind things don't get drawn
                // and lights don't obscure each other
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        Self {
            direct_pipeline,
            gbufs_bind_group_layout,
            gbufs_bind_group,
            dir_light_buf,
            dir_light_bind_group,
            point_volume_vertex_buf,
            point_volume_index_buf,
            point_volume_index_count,
            camera_buf,
            camera_bind_group,
            point_instance_buf,
            point_pipeline,
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
        &'s mut self,
        pass: &mut wgpu::RenderPass<'pass>,
        camera: &crate::Camera,
        dir_light: DirectionalLight,
        point_lights: impl Iterator<Item = PointLight>,
    ) {
        let window = super::Renderer::window();
        let queue = super::Renderer::queue();

        let light_unif = DirectLightUniforms::from(dir_light);
        queue.write_buffer(&self.dir_light_buf, 0, light_unif.as_bytes());

        let point_instances: Vec<PointLightInstance> =
            point_lights.map(PointLightInstance::from).collect();
        self.point_instance_buf.write(&point_instances);

        let view_proj = camera.view_proj_matrix(window.inner_size().into());
        queue.write_buffer(&self.camera_buf, 0, view_proj.as_byte_slice());

        pass.set_pipeline(&self.direct_pipeline);
        pass.set_bind_group(0, &self.gbufs_bind_group, &[]);
        pass.set_bind_group(1, &self.dir_light_bind_group, &[]);
        pass.draw(0..3, 0..1);

        if point_instances.is_empty() {
            return;
        }

        pass.set_pipeline(&self.point_pipeline);
        pass.set_bind_group(1, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, self.point_volume_vertex_buf.slice(..));
        pass.set_index_buffer(
            self.point_volume_index_buf.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        pass.set_vertex_buffer(1, self.point_instance_buf.slice());
        pass.draw_indexed(
            0..self.point_volume_index_count,
            0,
            0..point_instances.len() as u32,
        );
    }
}
