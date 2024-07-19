use std::mem::size_of;

use zerocopy::{AsBytes, FromBytes};

use crate::math::uv;

/// Distance between cascade 0 probes measured in screen pixels.
///
/// Currently this needs to be manually set to the same value as SPACING_C0 in the shader,
/// once we update wgpu we could use pipeline-overridable constants instead
const PROBE_INTERVAL: f32 = 4.;
/// Workgroups are arranged in squares of this size.
const TILE_SIZE: u32 = 16;

/// Scaling factor of the light texture in a constant for now,
/// TODO: make this configurable
const LIGHT_TEX_SCALING: f32 = 0.5;

pub(crate) struct GlobalIlluminationPipeline {
    // render target for drawing lights and occluders
    pub(super) light_tex: wgpu::TextureView,
    // size of the light texture relative to the screen size
    light_tex_scaling: f32,
    // compute pipeline for radiance cascades
    cascade_pl: wgpu::ComputePipeline,
    cascade_tex: wgpu::TextureView,
    bilinear_samp: wgpu::Sampler,
    probe_count: [u32; 2],
    params_buf: wgpu::Buffer,
    cascade_bind_group_layout: wgpu::BindGroupLayout,
    cascade_bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct CascadeParams {
    cascade_count: u32,
    probe_count_c0: [u32; 2],
}

/// Resources that need to be resized when screen size changes
struct ResizeResults {
    light_tex: wgpu::TextureView,
    cascade_tex: wgpu::TextureView,
    params: CascadeParams,
}

impl GlobalIlluminationPipeline {
    pub fn new() -> Self {
        let device = crate::Renderer::device();

        let light_tex_scaling = LIGHT_TEX_SCALING;
        let resizables = Self::create_resizables(
            crate::Renderer::window().inner_size().into(),
            light_tex_scaling,
        );

        // sampler with bilinear interpolation
        // to make use of hardware interpolation in combining cascades
        let bilinear_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cascade sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        use wgpu::util::DeviceExt;
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cascade params"),
            contents: resizables.params.as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let cascade_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cascade"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(size_of::<CascadeParams>() as _),
                        },
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                        count: None,
                    },wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        visibility: wgpu::ShaderStages::COMPUTE,
                        count: None,
                    },
                ],
            });

        let cascade_bind_group = Self::create_cascade_bind_group(
            &cascade_bind_group_layout,
            &resizables,
            &bilinear_samp,
            &params_buf,
        );

        let casc_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("global illumination"),
            bind_group_layouts: &[
                &cascade_bind_group_layout,
                super::Renderer::depth_bind_group_layout(),
            ],
            push_constant_ranges: &[],
        });

        let casc_shader =
            device.create_shader_module(wgpu::include_wgsl!("./shaders/radiance_cascades.wgsl"));

        let cascade_pl = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("global illumination"),
            module: &casc_shader,
            entry_point: "main",
            layout: Some(&casc_pl_layout),
        });

        Self {
            light_tex: resizables.light_tex,
            light_tex_scaling,
            cascade_pl,
            cascade_tex: resizables.cascade_tex,
            bilinear_samp,
            params_buf,
            probe_count: resizables.params.probe_count_c0,
            cascade_bind_group_layout,
            cascade_bind_group,
        }
    }

    fn create_resizables(target_size: (u32, u32), light_tex_scaling: f32) -> ResizeResults {
        let device = crate::Renderer::device();

        let light_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lights"),
            dimension: wgpu::TextureDimension::D2,
            size: wgpu::Extent3d {
                width: (target_size.0 as f32 * light_tex_scaling) as u32,
                height: (target_size.1 as f32 * light_tex_scaling) as u32,
                depth_or_array_layers: 1,
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
            mip_level_count: 1,
            sample_count: 1,
        });
        let light_tex = light_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // compute needed probe and cascade count to get full texture dimensions

        let screen_diag = uv::Vec2::new(target_size.0 as f32, target_size.1 as f32).mag();
        let cascade_count = (screen_diag / PROBE_INTERVAL).log(4.).ceil() as u32;

        let probe_count_x = (target_size.0 as f32 / PROBE_INTERVAL).floor() as u32;
        let probe_count_y = (target_size.1 as f32 / PROBE_INTERVAL).floor() as u32;

        // cascade 0 spans two columns, the rest are packed two per column.
        // see radiance_cascades.wgsl for a picture
        let tex_column_count = 2 + cascade_count / 2;
        let width = tex_column_count * probe_count_x;
        let height = 2 * probe_count_y;

        let cascade_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("radiance cascades"),
            dimension: wgpu::TextureDimension::D2,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
            mip_level_count: 1,
            sample_count: 1,
        });
        let cascade_tex = cascade_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let params = CascadeParams {
            cascade_count,
            probe_count_c0: [probe_count_x, probe_count_y],
        };

        ResizeResults {
            light_tex,
            cascade_tex,
            params,
        }
    }

    fn create_cascade_bind_group(
        layout: &wgpu::BindGroupLayout,
        resizables: &ResizeResults,
        samp: &wgpu::Sampler,
        params_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let device = crate::Renderer::device();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cascade"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&resizables.cascade_tex),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(samp),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&resizables.light_tex),
                },
            ],
        })
    }

    /// Recompute needed cascade and probe counts when window size changes.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let queue = crate::Renderer::queue();

        let res = Self::create_resizables(new_size.into(), self.light_tex_scaling);
        queue.write_buffer(&self.params_buf, 0, res.params.as_bytes());
        self.probe_count = res.params.probe_count_c0;
        self.cascade_bind_group = Self::create_cascade_bind_group(
            &self.cascade_bind_group_layout,
            &res,
            &self.bilinear_samp,
            &self.params_buf,
        );
        self.cascade_tex = res.cascade_tex;
        self.light_tex = res.light_tex;
    }

    pub fn compute_gi<'pass>(
        &'pass self,
        pass: &mut wgpu::ComputePass<'pass>,
        depth_bind_group: &'pass wgpu::BindGroup,
    ) {
        let tiles_x = (self.probe_count[0] as f32 / TILE_SIZE as f32).ceil() as u32;
        let tiles_y = (self.probe_count[1] as f32 / TILE_SIZE as f32).ceil() as u32;
        pass.set_pipeline(&self.cascade_pl);
        pass.set_bind_group(0, &self.cascade_bind_group, &[]);
        pass.set_bind_group(1, depth_bind_group, &[]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }
}
