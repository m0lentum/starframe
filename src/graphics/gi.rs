use std::mem::size_of;

use zerocopy::{AsBytes, FromBytes};

use crate::math::uv;

/// Distance between cascade 0 probes measured in screen pixels.
const C0_PROBE_INTERVAL: f32 = 2.;
/// Length of the radiance interval measured by cascade 0 probes.
/// This should be proportional to the diagonal
/// of a square with side C0_PROBE_INTERVAL
const C0_PROBE_RANGE: f32 = 2.8;
/// Workgroups are arranged in squares of this size.
const TILE_SIZE: u32 = 16;

/// Scaling factor of the light texture relative to the screen.
const LIGHT_TEX_SCALING: f32 = 0.5;

pub(crate) struct GlobalIlluminationPipeline {
    // render target for drawing lights and occluders
    pub(super) light_tex: wgpu::TextureView,
    // size of the light texture relative to the screen size
    light_tex_scaling: f32,
    // compute pipeline for radiance cascades
    cascade_pl: wgpu::ComputePipeline,
    cascade_tex: wgpu::TextureView,
    probe_count: [u32; 2],
    compute_params_buf: wgpu::Buffer,
    cascade_bind_group_layout: wgpu::BindGroupLayout,
    cascade_bind_group: wgpu::BindGroup,
    // separate bind group for mesh rendering
    // because it requires a different texture type and a sampler
    render_params_buf: wgpu::Buffer,
    bilinear_samp: wgpu::Sampler,
    pub(super) render_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) render_bind_group: wgpu::BindGroup,
}

/// Parameters for the compute step
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct CascadeParams {
    cascade_count: u32,
    _pad0: u32,
    probe_count_c0: [u32; 2],
    // spacing and range in the light texture's pixel space,
    // not screen space
    spacing_c0: f32,
    range_c0: f32,
}

/// Parameters for the mesh renderer to use the cascade results
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct RenderParams {
    probe_spacing: f32,
}

/// Resources that need to be resized when screen size changes
struct ResizeResults {
    light_tex: wgpu::TextureView,
    cascade_tex: wgpu::TextureView,
    compute_params: CascadeParams,
    render_params: RenderParams,
}

impl GlobalIlluminationPipeline {
    pub fn new() -> Self {
        let device = crate::Renderer::device();

        let light_tex_scaling = LIGHT_TEX_SCALING;
        let resizables = Self::create_resizables(
            crate::Renderer::window().inner_size().into(),
            light_tex_scaling,
        );

        use wgpu::util::DeviceExt;
        let compute_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cascade params"),
            contents: resizables.compute_params.as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let render_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cascade render params"),
            contents: resizables.render_params.as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bilinear_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bilinear sampler for cascade rendering"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
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
                        visibility: wgpu::ShaderStages::COMPUTE,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        ty: wgpu::BindingType::StorageTexture { 
                            access: wgpu::StorageTextureAccess::ReadWrite,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2
                        },
                        visibility: wgpu::ShaderStages::COMPUTE,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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

        let render_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cascade render"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<RenderParams>() as _),
                    },
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    ty: wgpu::BindingType::Texture { 
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    count: None,
                },
            ],
        });

        let cascade_bind_group =
            Self::create_cascade_bind_group(&cascade_bind_group_layout, &resizables, &compute_params_buf);
        let render_bind_group = Self::create_render_bind_group(
            &render_bind_group_layout,
            &resizables.cascade_tex,
            &bilinear_samp,
            &render_params_buf
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
            compute_params_buf,
            probe_count: resizables.compute_params.probe_count_c0,
            cascade_bind_group_layout,
            cascade_bind_group,
            render_params_buf,
            bilinear_samp,
            render_bind_group_layout,
            render_bind_group,
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
        let cascade_count = (screen_diag / C0_PROBE_INTERVAL).log(4.).ceil() as u32;

        let probe_count_x = (target_size.0 as f32 / C0_PROBE_INTERVAL).floor() as u32;
        let probe_count_y = (target_size.1 as f32 / C0_PROBE_INTERVAL).floor() as u32;

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

        let compute_params = CascadeParams {
            cascade_count,
            _pad0: 0,
            probe_count_c0: [probe_count_x, probe_count_y],
            // note the scaling by the light texture size
            spacing_c0: C0_PROBE_INTERVAL * LIGHT_TEX_SCALING,
            range_c0: C0_PROBE_RANGE * LIGHT_TEX_SCALING,
        };

        let render_params = RenderParams {
            // this one uses the actual framebuffer so no light texture scaling
            probe_spacing: C0_PROBE_INTERVAL,
        };

        ResizeResults {
            light_tex,
            cascade_tex,
            compute_params,
            render_params,
        }
    }

    fn create_cascade_bind_group(
        layout: &wgpu::BindGroupLayout,
        resizables: &ResizeResults,
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
                    resource: wgpu::BindingResource::TextureView(&resizables.light_tex),
                },
            ],
        })
    }

    fn create_render_bind_group(
        layout: &wgpu::BindGroupLayout,
        cascade_tex: &wgpu::TextureView,
        bilinear_samp: &wgpu::Sampler,
        params_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let device = crate::Renderer::device();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cascade render"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(cascade_tex),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(bilinear_samp),
                },
            ],
        })
    }

    /// Recompute needed cascade and probe counts when window size changes.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let queue = crate::Renderer::queue();

        let res = Self::create_resizables(new_size.into(), self.light_tex_scaling);
        queue.write_buffer(&self.compute_params_buf, 0, res.compute_params.as_bytes());
        queue.write_buffer(&self.render_params_buf, 0, res.render_params.as_bytes());
        self.probe_count = res.compute_params.probe_count_c0;
        self.cascade_bind_group = Self::create_cascade_bind_group(
            &self.cascade_bind_group_layout,
            &res,
            &self.compute_params_buf,
        );
        self.render_bind_group = Self::create_render_bind_group(
            &self.render_bind_group_layout,
            &res.cascade_tex,
            &self.bilinear_samp,
            &self.render_params_buf
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
