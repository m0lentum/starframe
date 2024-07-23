use std::mem::size_of;

use wgpu::util::DeviceExt;
use zerocopy::{AsBytes, FromBytes};

use crate::math::uv;

/// Distance between cascade 0 probes measured in screen pixels.
const C0_PROBE_INTERVAL: f32 = 2.;
/// Length of the radiance interval measured by cascade 0 probes.
/// This is half the diagonal
/// of a square with side C0_PROBE_INTERVAL
const C0_PROBE_RANGE: f32 = std::f32::consts::SQRT_2;
/// Workgroups are arranged in squares of this size.
const TILE_SIZE: u32 = 16;

/// Scaling factor of the light texture relative to the screen.
const LIGHT_TEX_SCALING: f32 = 0.5;

pub(crate) struct GlobalIlluminationPipeline {
    // render target for drawing lights and occluders
    pub(super) light_tex: wgpu::TextureView,
    // size of the light texture relative to the screen size
    light_tex_scaling: f32,
    light_bind_group_layout: wgpu::BindGroupLayout,
    light_bind_group: wgpu::BindGroup,
    // compute pipeline for radiance cascades
    cascade_pl: wgpu::ComputePipeline,
    // separate texture for the final cascade
    // because it's a different size from the rest
    cascade_0_tex: wgpu::TextureView,
    // rest of the cascades are done with alternating textures
    cascade_texs: [wgpu::TextureView; 2],
    cascade_bind_groups: CascadeBindGroups,
    cascade_bind_group_layout: wgpu::BindGroupLayout,
    cascade_count: usize,
    probe_count: [u32; 2],
    cascade_params_buf: wgpu::Buffer,
    // separate bind group for mesh rendering
    render_params_buf: wgpu::Buffer,
    bilinear_samp: wgpu::Sampler,
    pub(super) render_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) render_bind_group: wgpu::BindGroup,
}

struct CascadeBindGroups {
    // bind groups for each ordering of textures (one read, one write)
    intermediate: [wgpu::BindGroup; 2],
    // two possible bind groups for the final cascade
    final_: wgpu::BindGroup,
}

/// Parameters for the compute step
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct CascadeParams {
    level: u32,
    level_count: u32,
    probe_count: [u32; 2],
    rays_per_probe: u32,
    // spacing and range in the light texture's pixel space,
    // not screen space
    linear_spacing: f32,
    range_start: f32,
    range_length: f32,
    // padding to reach the minimum dynamic offset alignment
    _pad: [u32; 8],
}

impl CascadeParams {
    /// Compute the parameters for a given cascade level.
    ///
    /// On each level, the number of probes is reduced by a factor of 4
    /// (halved in both dimensions)
    /// and the number of rays per probe is increased by a factor of 4.
    fn for_level(level: u32, cascade_count: u32, probe_count_c0: [u32; 2]) -> Self {
        let level_exp2 = 1 << level;
        let level_exp4 = level_exp2 * level_exp2;
        // note the scaling by the light texture size
        let spacing_c0 = C0_PROBE_INTERVAL * LIGHT_TEX_SCALING;
        let range_c0 = C0_PROBE_INTERVAL * LIGHT_TEX_SCALING;
        CascadeParams {
            level,
            level_count: cascade_count,
            probe_count: probe_count_c0.map(|c| c / level_exp2),
            rays_per_probe: level_exp4,
            linear_spacing: spacing_c0 * level_exp2 as f32,
            // each range is 4 times larger than the previous,
            // and starts at the previous one's end,
            // hence the start distance is the sum of a geometric sequence
            range_start: range_c0 * (1. - level_exp4 as f32) / (1. - 4.),
            range_length: range_c0 * level_exp4 as f32,
            _pad: [0; 8],
        }
    }
}

/// Parameters for the mesh renderer to use the cascade results
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct RenderParams {
    probe_spacing: f32,
    _pad: u32,
    probe_count: [u32; 2],
}

/// Resources that need to be resized when screen size changes
struct ResizeResults {
    light_tex: wgpu::TextureView,
    cascade_0_tex: wgpu::TextureView,
    cascade_texs: [wgpu::TextureView; 2],
    cascade_params: Vec<CascadeParams>,
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
        let cascade_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cascade compute params"),
            contents: resizables.cascade_params.as_bytes(),
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

        let light_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light texture for cascades"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
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
        let light_bind_group = Self::create_light_bind_group(&light_bind_group_layout, &resizables.light_tex);

        let cascade_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cascade"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: wgpu::BufferSize::new(size_of::<CascadeParams>() as _),
                        },
                        visibility: wgpu::ShaderStages::COMPUTE,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        ty: wgpu::BindingType::StorageTexture { 
                            access: wgpu::StorageTextureAccess::ReadOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2
                        },
                        visibility: wgpu::ShaderStages::COMPUTE,
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        ty: wgpu::BindingType::StorageTexture { 
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2
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

        let cascade_bind_groups =
            Self::create_cascade_bind_groups(&cascade_bind_group_layout, &resizables, &cascade_params_buf);
        let render_bind_group = Self::create_render_bind_group(
            &render_bind_group_layout,
            &resizables.cascade_0_tex,
            &bilinear_samp,
            &render_params_buf
        );

        let casc_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("radiance cascades"),
            bind_group_layouts: &[
                &light_bind_group_layout,
                &cascade_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        let casc_shader =
            device.create_shader_module(wgpu::include_wgsl!("./shaders/radiance_cascades.wgsl"));

        let cascade_pl = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("radiance cascades"),
            module: &casc_shader,
            entry_point: "main",
            layout: Some(&casc_pl_layout),
        });

        Self {
            light_bind_group_layout,
            light_bind_group,
            light_tex: resizables.light_tex,
            light_tex_scaling,
            cascade_pl,
            cascade_0_tex: resizables.cascade_0_tex,
            cascade_texs: resizables.cascade_texs,
            cascade_params_buf,
            cascade_count: resizables.cascade_params.len(),
            probe_count: resizables.cascade_params[0].probe_count,
            cascade_bind_group_layout,
            cascade_bind_groups,
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
        // iterative computation for cascade count,
        // the number needed is the minimum where a probe reaches all the way across the screen
        let mut range = C0_PROBE_RANGE;
        let mut cascade_count = 1;
        while range < screen_diag {
            // each probe's range starts at the previous one's
            // and has a length of 4 times the previous
            range += C0_PROBE_RANGE * (4u32.pow(cascade_count)) as f32;
            cascade_count += 1;
        }

        let probe_count = [
            (target_size.0 as f32 / C0_PROBE_INTERVAL).floor() as u32,
            (target_size.1 as f32 / C0_PROBE_INTERVAL).floor() as u32,
        ];

        let create_tex = |scaling: u32| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("radiance cascades"),
                dimension: wgpu::TextureDimension::D2,
                size: wgpu::Extent3d {
                    width: probe_count[0] * scaling,
                    height: probe_count[1] * scaling,
                    depth_or_array_layers: 1,
                },
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
                mip_level_count: 1,
                sample_count: 1,
            });
            tex.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let cascade_0_tex = create_tex(2);
        let cascade_texs = [create_tex(1), create_tex(1)];

        let cascade_params = (0..cascade_count).map(|level| {
            CascadeParams::for_level(level, cascade_count, probe_count)
        }).collect();

        let render_params = RenderParams {
            // this one uses the actual framebuffer so no light texture scaling
            probe_spacing: C0_PROBE_INTERVAL,
            _pad: 0,
            probe_count,
        };

        ResizeResults {
            light_tex,
            cascade_0_tex,
            cascade_texs,
            cascade_params,
            render_params,
        }
    }

    fn create_light_bind_group(
        layout: &wgpu::BindGroupLayout,
        tex: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let device = crate::Renderer::device();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cascade light"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(tex),
                }
            ],
        })
    }

    fn create_cascade_bind_groups(
        layout: &wgpu::BindGroupLayout,
        resizables: &ResizeResults,
        params_buf: &wgpu::Buffer,
    ) -> CascadeBindGroups {
        let device = crate::Renderer::device();

        let create = |read: &wgpu::TextureView, write: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cascade"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { 
                            buffer: params_buf,
                            offset: 0,
                            size: wgpu::BufferSize::new(size_of::<CascadeParams>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(read),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(write),
                    },
                ],
            })
        };

        CascadeBindGroups {
            intermediate: [
                create(&resizables.cascade_texs[0], &resizables.cascade_texs[1]),
                create(&resizables.cascade_texs[1], &resizables.cascade_texs[0]),
            ],
            // cascade 1 always writes into the first texture
            final_: create(&resizables.cascade_texs[0], &resizables.cascade_0_tex),
        }
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
        let device = crate::Renderer::device();
        let queue = crate::Renderer::queue();

        let res = Self::create_resizables(new_size.into(), self.light_tex_scaling);
        // make room in the params buffer if we need more cascades than before
        if res.cascade_params.len() > self.cascade_count {
            self.cascade_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cascade compute params"),
                contents: res.cascade_params.as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        } else {
            queue.write_buffer(&self.cascade_params_buf, 0, res.cascade_params.as_bytes());
        }
        self.cascade_count = res.cascade_params.len();
        queue.write_buffer(&self.render_params_buf, 0, res.render_params.as_bytes());

        self.probe_count = res.cascade_params[0].probe_count;
        self.cascade_bind_groups = Self::create_cascade_bind_groups(
            &self.cascade_bind_group_layout,
            &res,
            &self.cascade_params_buf,
        );
        self.render_bind_group = Self::create_render_bind_group(
            &self.render_bind_group_layout,
            &res.cascade_0_tex,
            &self.bilinear_samp,
            &self.render_params_buf
        );
        self.cascade_0_tex = res.cascade_0_tex;
        self.cascade_texs = res.cascade_texs;
        self.light_bind_group = Self::create_light_bind_group(&self.light_bind_group_layout, &res.light_tex);
        self.light_tex = res.light_tex;
    }

    pub fn compute_gi<'pass>(
        &'pass self,
        pass: &mut wgpu::ComputePass<'pass>,
    ) {
        let tiles_x = (self.probe_count[0] as f32 / TILE_SIZE as f32).ceil() as u32;
        let tiles_y = (self.probe_count[1] as f32 / TILE_SIZE as f32).ceil() as u32;
        pass.set_pipeline(&self.cascade_pl);
        pass.set_bind_group(0, &self.light_bind_group, &[]);
        // cascades starting with the last
        for casc_idx in (1..self.cascade_count).rev() {
            pass.set_bind_group(
                1,
                &self.cascade_bind_groups.intermediate[casc_idx % 2],
                &[(casc_idx * size_of::<CascadeParams>()) as u32]
            );
            pass.dispatch_workgroups(tiles_x, tiles_y, 1);
        }
        // final cascade drawing to the special differently sized texture
        pass.set_bind_group(1, &self.cascade_bind_groups.final_, &[0]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }
}
