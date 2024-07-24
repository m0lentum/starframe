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

/// Hardcoded pass count to simplify things for JFA,
/// we don't need to cover the entire screen for it to be useful.
/// Maximum distance we get from this is 2^(1-JFA_PASS_COUNT)
const JFA_PASS_COUNT: usize = 8;

pub(crate) struct GlobalIlluminationPipeline {
    pipelines: Pipelines,
    pub(super) textures: Textures,
    pub(super) bind_group_layouts: BindGroupLayouts,
    pub(super) bind_groups: BindGroups,
    buffers: Buffers,
    light_tex_size: [u32; 2],
    cascade_count: usize,
    probe_count: [u32; 2],
}

struct Pipelines {
    // jump flood algorithm to generate a distance field of the light texture
    jfa_init: wgpu::ComputePipeline,
    jfa_iter: wgpu::ComputePipeline,
    jfa_finish: wgpu::ComputePipeline,
    // actual radiance cascade computation
    cascade: wgpu::ComputePipeline,
}

pub(super) struct BindGroupLayouts {
    jfa: wgpu::BindGroupLayout,
    light: wgpu::BindGroupLayout,
    cascade: wgpu::BindGroupLayout,
    pub(super) render: wgpu::BindGroupLayout,
}

pub(super) struct BindGroups {
    // bind groups for each ordering of textures (one read, one write)
    jfa: [wgpu::BindGroup; 2],
    // binds the light texture for the cascade compute step
    light: wgpu::BindGroup,
    // back and forth bind groups like with jfa
    cascades: [wgpu::BindGroup; 2],
    final_cascade: wgpu::BindGroup,
    pub(super) render: wgpu::BindGroup,
}

pub(super) struct Textures {
    jfa: [wgpu::TextureView; 2],
    // light emitters and occluders are drawn into this by MeshRenderer
    pub(super) light: wgpu::TextureView,
    // final cascade is a different size from the others
    // because we keep direction information
    // instead of pre-averaging rays
    cascade_0: wgpu::TextureView,
    // other cascades alternate between two textures of the same size
    cascades: [wgpu::TextureView; 2],
}

struct Buffers {
    jfa_params: wgpu::Buffer,
    cascade_params: wgpu::Buffer,
    render_params: wgpu::Buffer,
}

/// Resources that need to be resized when screen size changes
struct ResizeResults {
    light_tex_size: [u32; 2],
    textures: Textures,
    cascade_params: Vec<CascadeParams>,
    render_params: RenderParams,
}

/// Uniform parameters for JFA computation
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct JumpFloodParams {
    iter_idx: u32,
    // padding to reach the minimum dynamic offset alignment
    _pad: [u32; 15],
}

/// Uniform parameters for the cascade computation
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

impl GlobalIlluminationPipeline {
    pub fn new() -> Self {
        let device = crate::Renderer::device();

        let bind_group_layouts = Self::create_bind_group_layouts();

        let resizables = Self::create_resizables(crate::Renderer::window().inner_size().into());

        let jfa_params: Vec<_> = (0..JFA_PASS_COUNT as u32)
            .map(|iter_idx| JumpFloodParams {
                iter_idx,
                _pad: [0; 15],
            })
            .collect();

        use wgpu::util::DeviceExt;
        let buffers = Buffers {
            jfa_params: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cascade compute params"),
                contents: jfa_params.as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }),
            cascade_params: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cascade compute params"),
                contents: resizables.cascade_params.as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }),
            render_params: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cascade render params"),
                contents: resizables.render_params.as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }),
        };

        let bind_groups =
            Self::create_bind_groups(&bind_group_layouts, &resizables.textures, &buffers);

        let pipelines = Self::create_pipelines(&bind_group_layouts);

        Self {
            pipelines,
            textures: resizables.textures,
            bind_group_layouts,
            bind_groups,
            buffers,
            light_tex_size: resizables.light_tex_size,
            cascade_count: resizables.cascade_params.len(),
            probe_count: resizables.cascade_params[0].probe_count,
        }
    }

    fn create_pipelines(bg_layouts: &BindGroupLayouts) -> Pipelines {
        let device = crate::Renderer::device();

        let casc_shader =
            device.create_shader_module(wgpu::include_wgsl!("./shaders/radiance_cascades.wgsl"));

        let cascade_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("radiance cascades"),
            bind_group_layouts: &[&bg_layouts.light, &bg_layouts.cascade],
            push_constant_ranges: &[],
        });

        let cascade = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("radiance cascades"),
            module: &casc_shader,
            entry_point: "main",
            layout: Some(&cascade_layout),
        });

        let jfa_shader =
            device.create_shader_module(wgpu::include_wgsl!("./shaders/jump_flood.wgsl"));

        let jfa_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jfa"),
            bind_group_layouts: &[&bg_layouts.jfa],
            push_constant_ranges: &[],
        });

        let jfa_init = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("jfa init"),
            module: &jfa_shader,
            entry_point: "init",
            layout: Some(&jfa_layout),
        });

        let jfa_iter = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("jfa iter"),
            module: &jfa_shader,
            entry_point: "iter",
            layout: Some(&jfa_layout),
        });

        let jfa_finish = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("jfa finish"),
            module: &jfa_shader,
            entry_point: "finish",
            layout: Some(&jfa_layout),
        });

        Pipelines {
            cascade,
            jfa_init,
            jfa_iter,
            jfa_finish,
        }
    }

    fn create_bind_group_layouts() -> BindGroupLayouts {
        let device = crate::Renderer::device();

        let uniform_buf = |binding: u32,
                           size: usize,
                           has_dynamic_offset: bool,
                           visibility: wgpu::ShaderStages| {
            wgpu::BindGroupLayoutEntry {
                binding,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset,
                    min_binding_size: wgpu::BufferSize::new(size as u64),
                },
                visibility,
                count: None,
            }
        };

        let float_tex = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            visibility,
            count: None,
        };

        let int_tex = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Sint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            visibility,
            count: None,
        };

        let write_storage =
            |binding: u32, format: wgpu::TextureFormat| wgpu::BindGroupLayoutEntry {
                binding,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                visibility: wgpu::ShaderStages::COMPUTE,
                count: None,
            };

        use wgpu::ShaderStages as S;

        let jfa = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cascade"),
            entries: &[
                uniform_buf(0, size_of::<JumpFloodParams>(), true, S::COMPUTE),
                float_tex(1, S::COMPUTE),
                int_tex(2, S::COMPUTE),
                write_storage(3, wgpu::TextureFormat::Rg32Sint),
            ],
        });

        let light = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light texture for cascades"),
            entries: &[float_tex(0, S::COMPUTE), int_tex(1, S::COMPUTE)],
        });

        let cascade = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cascade"),
            entries: &[
                uniform_buf(0, size_of::<CascadeParams>(), true, S::COMPUTE),
                float_tex(1, S::COMPUTE),
                write_storage(2, wgpu::TextureFormat::Rgba8Unorm),
            ],
        });

        let render = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cascade render"),
            entries: &[
                uniform_buf(0, size_of::<RenderParams>(), false, S::FRAGMENT),
                float_tex(1, wgpu::ShaderStages::FRAGMENT),
            ],
        });

        BindGroupLayouts {
            jfa,
            light,
            cascade,
            render,
        }
    }

    fn create_bind_groups(
        layouts: &BindGroupLayouts,
        tex: &Textures,
        buffers: &Buffers,
    ) -> BindGroups {
        let device = crate::Renderer::device();

        let jfa = |read: &wgpu::TextureView, write: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("jfa"),
                layout: &layouts.jfa,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffers.jfa_params,
                            offset: 0,
                            size: wgpu::BufferSize::new(size_of::<JumpFloodParams>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tex.light),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(read),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(write),
                    },
                ],
            })
        };

        let light = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cascade light"),
            layout: &layouts.light,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tex.light),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex.jfa[0]),
                },
            ],
        });

        let cascade = |read: &wgpu::TextureView, write: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cascade"),
                layout: &layouts.cascade,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffers.cascade_params,
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

        let render = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cascade render"),
            layout: &layouts.render,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffers.render_params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex.cascade_0),
                },
            ],
        });

        BindGroups {
            jfa: [jfa(&tex.jfa[0], &tex.jfa[1]), jfa(&tex.jfa[1], &tex.jfa[0])],
            light,
            cascades: [
                cascade(&tex.cascades[0], &tex.cascades[1]),
                cascade(&tex.cascades[1], &tex.cascades[0]),
            ],
            // cascade 1 always writes into the first texture
            // so we only need one bind group for cascade 0
            final_cascade: cascade(&tex.cascades[0], &tex.cascade_0),
            render,
        }
    }

    fn create_resizables(target_size: (u32, u32)) -> ResizeResults {
        let device = crate::Renderer::device();

        let light_tex_size = wgpu::Extent3d {
            width: (target_size.0 as f32 * LIGHT_TEX_SCALING) as u32,
            height: (target_size.1 as f32 * LIGHT_TEX_SCALING) as u32,
            depth_or_array_layers: 1,
        };

        let light_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lights"),
            dimension: wgpu::TextureDimension::D2,
            size: light_tex_size,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
            mip_level_count: 1,
            sample_count: 1,
        });
        let light = light_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let jfa_tex = || {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("jfa"),
                dimension: wgpu::TextureDimension::D2,
                size: light_tex_size,
                format: wgpu::TextureFormat::Rg32Sint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
                mip_level_count: 1,
                sample_count: 1,
            });
            tex.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let jfa = [jfa_tex(), jfa_tex()];

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

        let cascade_tex = |scaling: u32| {
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
        let cascade_0 = cascade_tex(2);
        let cascades = [cascade_tex(1), cascade_tex(1)];

        let cascade_params = (0..cascade_count)
            .map(|level| CascadeParams::for_level(level, cascade_count, probe_count))
            .collect();

        let render_params = RenderParams {
            // this one uses the actual framebuffer so no light texture scaling
            probe_spacing: C0_PROBE_INTERVAL,
            _pad: 0,
            probe_count,
        };

        ResizeResults {
            light_tex_size: [light_tex_size.width, light_tex_size.height],
            textures: Textures {
                jfa,
                light,
                cascade_0,
                cascades,
            },
            cascade_params,
            render_params,
        }
    }

    /// Recompute needed cascade and probe counts when window size changes.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let device = crate::Renderer::device();
        let queue = crate::Renderer::queue();

        let res = Self::create_resizables(new_size.into());
        // make room in the params buffer if we need more cascades than before
        if res.cascade_params.len() > self.cascade_count {
            self.buffers.cascade_params =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cascade compute params"),
                    contents: res.cascade_params.as_bytes(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
        } else {
            let buf = &self.buffers.cascade_params;
            queue.write_buffer(buf, 0, res.cascade_params.as_bytes());
        }
        self.cascade_count = res.cascade_params.len();
        queue.write_buffer(&self.buffers.render_params, 0, res.render_params.as_bytes());

        self.probe_count = res.cascade_params[0].probe_count;
        self.bind_groups =
            Self::create_bind_groups(&self.bind_group_layouts, &res.textures, &self.buffers);
        self.textures = res.textures;
    }

    pub fn compute_sdf<'pass>(&'pass self, pass: &mut wgpu::ComputePass<'pass>) {
        let tiles_x = (self.light_tex_size[0] as f32 / TILE_SIZE as f32).ceil() as u32;
        let tiles_y = (self.light_tex_size[1] as f32 / TILE_SIZE as f32).ceil() as u32;

        pass.set_pipeline(&self.pipelines.jfa_init);
        // we want the final distance values to be in the first jfa texture
        // because that's the one we bind to the cascade step;
        // select the starting bind group accordingly
        pass.set_bind_group(0, &self.bind_groups.jfa[JFA_PASS_COUNT % 2], &[0]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);

        pass.set_pipeline(&self.pipelines.jfa_iter);
        for iter_idx in (0..JFA_PASS_COUNT).rev() {
            pass.set_bind_group(
                0,
                &self.bind_groups.jfa[iter_idx % 2],
                &[(iter_idx * size_of::<JumpFloodParams>()) as u32],
            );
            pass.dispatch_workgroups(tiles_x, tiles_y, 1);
        }

        pass.set_pipeline(&self.pipelines.jfa_finish);
        pass.set_bind_group(0, &self.bind_groups.jfa[1], &[0]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }

    pub fn compute_gi<'pass>(&'pass self, pass: &mut wgpu::ComputePass<'pass>) {
        let tiles_x = (self.probe_count[0] as f32 / TILE_SIZE as f32).ceil() as u32;
        let tiles_y = (self.probe_count[1] as f32 / TILE_SIZE as f32).ceil() as u32;
        pass.set_pipeline(&self.pipelines.cascade);
        pass.set_bind_group(0, &self.bind_groups.light, &[]);
        // cascades starting with the last
        for casc_idx in (1..self.cascade_count).rev() {
            pass.set_bind_group(
                1,
                &self.bind_groups.cascades[casc_idx % 2],
                &[(casc_idx * size_of::<CascadeParams>()) as u32],
            );
            pass.dispatch_workgroups(tiles_x, tiles_y, 1);
        }
        // final cascade drawing to the special differently sized texture
        pass.set_bind_group(1, &self.bind_groups.final_cascade, &[0]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }
}
