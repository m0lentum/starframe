use std::mem::size_of;

use wgpu::util::DeviceExt;
use zerocopy::{AsBytes, FromBytes};

use crate::Renderer;

/// Singleton collection of shared resources stored in `GraphicsManager`.
pub(super) struct MaterialDefaults {
    /// Bind group layout shared by all materials.
    bind_group_layout: wgpu::BindGroupLayout,
    /// Placeholder texture to bind when the material doesn't have a texture.
    blank_texture: Texture,
}

impl MaterialDefaults {
    pub(super) fn new(rend: &Renderer) -> Self {
        let bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("material"),
                    entries: &[
                        // parameter uniforms
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(
                                    size_of::<MaterialUniforms>() as _,
                                ),
                            },
                            count: None,
                        },
                        // texture and sampler for diffuse
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        // same for normal map
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });

        let blank_texture = TextureData {
            label: Some("blank".to_string()),
            pixels: &[255, 255, 255, 255],
            format: wgpu::TextureFormat::Rgba8Unorm,
            dimensions: (1, 1),
        }
        .upload(rend);

        Self {
            bind_group_layout,
            blank_texture,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MaterialParams<'a> {
    pub base_color: Option<[f32; 4]>,
    pub diffuse_tex: Option<TextureData<'a>>,
    pub normal_tex: Option<TextureData<'a>>,
}

pub struct Material {
    uniform_buf: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
    // textures stored to avoid dropping them
    _diffuse_tex: Option<Texture>,
    _normal_tex: Option<Texture>,
}

impl Material {
    pub(super) fn new(
        rend: &Renderer,
        defaults: &MaterialDefaults,
        params: MaterialParams,
    ) -> Self {
        let diffuse_tex = params.diffuse_tex.map(|t| t.upload(rend));
        let normal_tex = params.normal_tex.map(|t| t.upload(rend));

        let diffuse = diffuse_tex.as_ref().unwrap_or(&defaults.blank_texture);
        let normal = normal_tex.as_ref().unwrap_or(&defaults.blank_texture);

        let uniform_buf = rend
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("material uniforms"),
                contents: MaterialUniforms {
                    base_color: params.base_color.unwrap_or([1.; 4]),
                }
                .as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &defaults.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(uniform_buf.as_entire_buffer_binding()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&diffuse.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&diffuse.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&normal.view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&normal.sampler),
                },
            ],
        });

        Self {
            uniform_buf,
            bind_group,
            _diffuse_tex: diffuse_tex,
            _normal_tex: normal_tex,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct MaterialUniforms {
    base_color: [f32; 4],
}

#[derive(Debug)]
pub struct Texture {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
}

#[derive(Debug, Clone)]
pub struct TextureData<'a> {
    // this is a string due to complications in glTF loading that a &str would cause here
    pub label: Option<String>,
    pub pixels: &'a [u8],
    pub format: wgpu::TextureFormat,
    pub dimensions: (u32, u32),
}

impl<'a> TextureData<'a> {
    pub fn upload(self, rend: &crate::Renderer) -> Texture {
        let label = self.label.as_deref();
        let size = wgpu::Extent3d {
            width: self.dimensions.0,
            height: self.dimensions.1,
            depth_or_array_layers: 1,
        };
        let texture = rend.device.create_texture_with_data(
            &rend.queue,
            &wgpu::TextureDescriptor {
                label,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            self.pixels,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // TODO: get the sampler from gltf
        let sampler = rend.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Texture {
            texture,
            view,
            sampler,
        }
    }
}
