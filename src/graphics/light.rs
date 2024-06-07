use crate::math::{uv, Pose};

use itertools::izip;
use std::mem::size_of;
use zerocopy::{AsBytes, FromBytes};

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
                if let Some(&pose) = pose {
                    let pos_with_offset = pose * ret.position;
                    ret.position = pos_with_offset;
                }
                ret
            })
    }
}

/// Gpu-side representation of a directional light.
#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct DirectLightUniforms {
    direct_color: [f32; 3],
    _pad0: u32,
    ambient_color: [f32; 3],
    _pad1: u32,
    direction: [f32; 3],
    // tag is either 0 or 1,
    // 0 means ambient light only, no directional light
    tag: f32,
}

impl From<MainLight> for DirectLightUniforms {
    fn from(main_light: MainLight) -> Self {
        match main_light {
            MainLight::Directional(l) => Self {
                direct_color: l.direct_color,
                _pad0: 0,
                ambient_color: l.ambient_color,
                _pad1: 0,
                direction: l.direction.normalized().into(),
                tag: 1.0,
            },
            MainLight::AmbientOnly(color) => Self {
                direct_color: [0.; 3],
                _pad0: 0,
                ambient_color: color,
                _pad1: 0,
                direction: [0., 0., 0.],
                tag: 1.,
            },
        }
    }
}

/// Gpu-side representation of a single point light.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
pub(crate) struct GpuPointLight {
    position: [f32; 3],
    _pad0: u32,
    color: [f32; 3],
    _pad1: u32,
    radius: f32,
    attn_linear: f32,
    attn_quadratic: f32,
    _pad2: u32,
}

impl From<PointLight> for GpuPointLight {
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
            ..Default::default()
        }
    }
}

const MAX_LIGHTS: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct GpuPointLightBuffer {
    count: u32,
    _pad: [u32; 3],
    lights: [GpuPointLight; MAX_LIGHTS],
}

impl Default for GpuPointLightBuffer {
    fn default() -> Self {
        Self {
            count: 0,
            _pad: [0; 3],
            // default is not derived for arrays this big so we need the manual impl
            lights: [GpuPointLight::default(); 1024],
        }
    }
}

/// Main light of the scene can be a directional light
/// or just a flat ambient light with no direction information.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MainLight {
    Directional(DirectionalLight),
    AmbientOnly([f32; 3]),
}

/// GPU-side structures for lighting.
pub(crate) struct LightBuffers {
    pub dir_light_buf: wgpu::Buffer,
    pub point_light_buf: wgpu::Buffer,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl LightBuffers {
    pub fn new() -> Self {
        let device = crate::Renderer::device();
        let dir_light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<DirectLightUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("directional light"),
            mapped_at_creation: false,
        });

        let point_buf_size = size_of::<GpuPointLightBuffer>() as u64;
        let point_light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: point_buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("point lights"),
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
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
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(point_buf_size),
                    },
                    count: None,
                },
            ],
            label: Some("lights"),
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("directional light"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: dir_light_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: point_light_buf.as_entire_binding(),
                },
            ],
        });

        Self {
            dir_light_buf,
            point_light_buf,
            bind_group_layout,
            bind_group,
        }
    }

    pub fn write_main_light(&self, main_light: MainLight) {
        let queue = crate::Renderer::queue();

        let light_unif = DirectLightUniforms::from(main_light);
        queue.write_buffer(&self.dir_light_buf, 0, light_unif.as_bytes());
    }

    pub fn write_point_lights(&self, mut lights: Vec<GpuPointLight>) {
        let queue = crate::Renderer::queue();

        lights.truncate(MAX_LIGHTS);

        let mut contents = GpuPointLightBuffer {
            count: lights.len() as u32,
            ..Default::default()
        };
        for (src, dst) in izip!(&lights[..], &mut contents.lights[..]) {
            *dst = *src;
        }

        queue.write_buffer(&self.point_light_buf, 0, contents.as_bytes());
    }
}
