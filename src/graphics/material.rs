use std::{mem::size_of, sync::OnceLock};

use wgpu::util::DeviceExt;
use zerocopy::{AsBytes, FromBytes};

static RESOURCES: OnceLock<MaterialResources> = OnceLock::new();
// white material that gets loaded if a mesh has no material
static DEFAULT_MATERIAL: OnceLock<Material> = OnceLock::new();

/// Singleton collection of shared resources.
struct MaterialResources {
    /// Bind group layout shared by all materials.
    pub bind_group_layout: wgpu::BindGroupLayout,
    /// White texture to bind when the material doesn't have a texture.
    pub blank_texture: Texture,
    /// Normal map facing directly in the normal direction,
    /// to bind when the material doesn't have a normal map.
    pub blank_normal: Texture,
}

impl MaterialResources {
    fn get<'a>() -> &'a Self {
        RESOURCES.get_or_init(Self::new)
    }

    fn new() -> Self {
        let device = crate::Renderer::device();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                // parameter uniforms
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<MaterialUniforms>() as _),
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
        .upload();

        let blank_normal = TextureData {
            label: Some("blank normal".to_string()),
            pixels: &[127, 127, 255, 0],
            format: wgpu::TextureFormat::Rgba8Unorm,
            dimensions: (1, 1),
        }
        .upload();

        Self {
            bind_group_layout,
            blank_texture,
            blank_normal,
        }
    }
}

/// Creation parameters for a material.
#[derive(Debug, Clone, Default)]
pub struct MaterialParams<'a> {
    pub base_color: Option<[f32; 4]>,
    pub emissive_color: Option<[f32; 4]>,
    pub diffuse_tex: Option<TextureData<'a>>,
    pub normal_tex: Option<TextureData<'a>>,
}

impl<'a> MaterialParams<'a> {
    /// Set the material to cast shadows.
    ///
    /// This sets emissive color to opaque black,
    /// which means the material can't both cast shadows and emit light.
    #[inline]
    pub fn cast_shadows(mut self) -> Self {
        self.emissive_color = Some([0., 0., 0., 1.]);
        self
    }
}

/// A material determines the color and lighting properties of a mesh.
pub struct Material {
    pub(crate) has_emissive: bool,
    pub(crate) bind_group: wgpu::BindGroup,
    // textures and buffer stored to avoid dropping them
    _uniform_buf: wgpu::Buffer,
    _diffuse_tex: Option<Texture>,
    _normal_tex: Option<Texture>,
}

impl Material {
    pub(super) fn new(params: MaterialParams) -> Self {
        let device = crate::Renderer::device();
        let res = MaterialResources::get();

        let diffuse_tex = params.diffuse_tex.map(|t| t.upload());
        let normal_tex = params.normal_tex.map(|t| t.upload());

        let diffuse = diffuse_tex.as_ref().unwrap_or(&res.blank_texture);
        let normal = normal_tex.as_ref().unwrap_or(&res.blank_normal);

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material uniforms"),
            contents: MaterialUniforms {
                base_color: params.base_color.unwrap_or([1.; 4]),
                emissive_color: params.emissive_color.unwrap_or([0.; 4]),
            }
            .as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &res.bind_group_layout,
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
            has_emissive: params.emissive_color.is_some(),
            bind_group,
            _uniform_buf: uniform_buf,
            _diffuse_tex: diffuse_tex,
            _normal_tex: normal_tex,
        }
    }

    pub(super) fn get_default<'a>() -> &'a Self {
        DEFAULT_MATERIAL.get_or_init(|| {
            Self::new(MaterialParams {
                base_color: Some([1.; 4]),
                emissive_color: None,
                diffuse_tex: None,
                normal_tex: None,
            })
        })
    }

    #[inline]
    pub(crate) fn bind_group_layout<'a>() -> &'a wgpu::BindGroupLayout {
        &MaterialResources::get().bind_group_layout
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct MaterialUniforms {
    base_color: [f32; 4],
    emissive_color: [f32; 4],
}

#[derive(Debug)]
pub struct Texture {
    pub(crate) _texture: wgpu::Texture,
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
    pub fn upload(self) -> Texture {
        let device = crate::Renderer::device();
        let queue = crate::Renderer::queue();
        let label = self.label.as_deref();
        let size = wgpu::Extent3d {
            width: self.dimensions.0,
            height: self.dimensions.1,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture_with_data(
            queue,
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Texture {
            _texture: texture,
            view,
            sampler,
        }
    }
}
