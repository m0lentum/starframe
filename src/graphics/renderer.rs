use super::{
    gi::GlobalIlluminationPipeline,
    light,
    line_renderer::LineRenderer,
    mesh::{skin::SkinPipeline, MeshRenderer},
};
use std::sync::OnceLock;

// there is only ever one wgpu context,
// and since the device and queue are frequently needed to create resources,
// we store those globally here
// so that the user doesn't have to ferry them around constantly

static DEVICE: OnceLock<wgpu::Device> = OnceLock::new();
static QUEUE: OnceLock<wgpu::Queue> = OnceLock::new();
static WINDOW: OnceLock<winit::window::Window> = OnceLock::new();
// bind group layout in a global for convenience.
// these are a bit scattered right now with dependencies in many places
// and the globals are a little ugly,
// might want to refactor bind group layouts into one place
static DEPTH_BIND_GROUP_LAYOUT: OnceLock<wgpu::BindGroupLayout> = OnceLock::new();

pub const SWAPCHAIN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;
pub const MSAA_SAMPLES: u32 = 4;
pub const DEFAULT_MULTISAMPLE_STATE: wgpu::MultisampleState = wgpu::MultisampleState {
    count: MSAA_SAMPLES,
    mask: !0,
    alpha_to_coverage_enabled: false,
};

/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    window_scale_factor: f64,

    msaa_view: wgpu::TextureView,
    // textures and bind group for depth and lights to use in GI
    depth_tex: wgpu::Texture,
    depth_view: wgpu::TextureView,
    emissive_tex: wgpu::Texture,
    emissive_view: wgpu::TextureView,

    light_man: light::LightManager,
    gi_pipeline: GlobalIlluminationPipeline,

    mesh_renderer: MeshRenderer,
    skin_pl: SkinPipeline,
    // rendering subsystems that aren't always used in lazily initialized Options
    // so we can have a unified API to call them through `Frame`
    // but don't pay for them if the user doesn't use them
    line_renderer: Option<LineRenderer>,
}

/// An error that occurred during renderer initialization.
#[derive(thiserror::Error, Debug)]
pub enum RendererInitError {
    #[error("Failed to create surface")]
    CreateSurfaceError(#[from] wgpu::CreateSurfaceError),
    #[error("Adapter request failed")]
    RequestAdapterError,
    #[error("Device request failed")]
    RequestDeviceError(#[from] wgpu::RequestDeviceError),
    #[error("Another Renderer already existed")]
    AlreadyInitialized,
}

impl Renderer {
    /// Create a Renderer.
    /// The [`Game`][crate::game::Game] API does this automatically.
    pub(crate) async fn init(window: winit::window::Window) -> Result<Self, RendererInitError> {
        let instance = wgpu::Instance::default();
        let surface = unsafe { instance.create_surface(&window) }?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .ok_or(RendererInitError::RequestAdapterError)?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::default(),
                    limits: wgpu::Limits {
                        min_uniform_buffer_offset_alignment: 64,
                        ..Default::default()
                    },
                    label: None,
                },
                None,
            )
            .await?;

        let window_size = window.inner_size();

        let swapchain_capabilities = surface.get_capabilities(&adapter);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: SWAPCHAIN_FORMAT,
            width: window_size.width,
            height: window_size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let window_scale_factor = window.scale_factor();

        DEVICE
            .set(device)
            .map_err(|_| RendererInitError::AlreadyInitialized)?;
        QUEUE
            .set(queue)
            .map_err(|_| RendererInitError::AlreadyInitialized)?;
        WINDOW
            .set(window)
            .map_err(|_| RendererInitError::AlreadyInitialized)?;

        let msaa_tex = Self::create_msaa_texture(window_size);
        let msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = Self::create_depth_texture(window_size);
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let emissive_tex = Self::create_emissive_texture(window_size);
        let emissive_view = emissive_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let gi_pipeline = GlobalIlluminationPipeline::new();
        let light_man = light::LightManager::new();
        let mesh_renderer = MeshRenderer::new(&gi_pipeline);
        let skin_pl = SkinPipeline::new();

        Ok(Renderer {
            surface,
            surface_config,
            window_scale_factor,
            msaa_view,
            depth_tex,
            depth_view,
            emissive_tex,
            emissive_view,
            gi_pipeline,
            light_man,
            mesh_renderer,
            skin_pl,
            line_renderer: None,
        })
    }

    /// Get a reference to the the window the game draws to.
    /// # Panics
    /// This function panics if the renderer hasn't been initialized yet,
    /// i.e. if [`Game::run`][crate::Game::run] hasn't been called yet.
    pub fn window<'a>() -> &'a winit::window::Window {
        WINDOW.get().expect("Renderer has not been initialized yet")
    }

    /// Get a reference to the the global device instance.
    /// # Panics
    /// This function panics if the renderer hasn't been initialized yet,
    /// i.e. if [`Game::run`][crate::Game::run] hasn't been called yet.
    #[inline]
    pub fn device<'a>() -> &'a wgpu::Device {
        DEVICE.get().expect("Renderer has not been initialized yet")
    }

    /// Get a reference to the the global queue instance.
    /// # Panics
    /// This function panics if the renderer hasn't been initialized yet,
    /// i.e. if [`Game::run`][crate::Game::run] hasn't been called yet.
    #[inline]
    pub fn queue<'a>() -> &'a wgpu::Queue {
        QUEUE.get().expect("Renderer has not been initialized yet")
    }

    /// Change the size of the frame `draw_to_window` draws into.
    /// This is called automatically by the gameloop when the window size changes.
    pub(crate) fn resize_swap_chain(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size == self.window_size() {
            return;
        }
        let device = Self::device();
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(device, &self.surface_config);
        let msaa_tex = Self::create_msaa_texture(new_size);
        self.msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth_tex = Self::create_depth_texture(new_size);
        self.depth_view = self
            .depth_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.emissive_tex = Self::create_emissive_texture(new_size);
        self.emissive_view = self
            .emissive_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.gi_pipeline.resize(new_size);
        self.light_man.recreate_light_bins(new_size.into());
    }

    fn create_msaa_texture(size: winit::dpi::PhysicalSize<u32>) -> wgpu::Texture {
        let device = Self::device();
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: MSAA_SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: SWAPCHAIN_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    fn create_depth_texture(size: winit::dpi::PhysicalSize<u32>) -> wgpu::Texture {
        let device = Self::device();
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: MSAA_SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    fn create_emissive_texture(size: winit::dpi::PhysicalSize<u32>) -> wgpu::Texture {
        let device = Self::device();
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("emissive"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    pub(crate) fn depth_bind_group_layout<'a>() -> &'a wgpu::BindGroupLayout {
        DEPTH_BIND_GROUP_LAYOUT.get_or_init(|| {
            let device = Self::device();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("depth"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: true,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            })
        })
    }

    #[inline]
    pub fn swapchain_format(&self) -> wgpu::TextureFormat {
        SWAPCHAIN_FORMAT
    }

    #[inline]
    pub fn depth_format(&self) -> wgpu::TextureFormat {
        DEPTH_FORMAT
    }

    /// Get the size of the window this Renderer draws to in pixels.
    #[inline]
    pub fn window_size(&self) -> winit::dpi::PhysicalSize<u32> {
        winit::dpi::PhysicalSize::new(self.surface_config.width, self.surface_config.height)
    }

    /// Get the scale factor of the window this Renderer draws to.
    #[inline]
    pub fn window_scale_factor(&self) -> f64 {
        self.window_scale_factor
    }

    /// Depth-stencil state that uses the same depth format as the window depth buffer
    /// and writes depths to the buffer.
    #[inline]
    pub fn default_depth_stencil_state(&self) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: self.depth_tex.format(),
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// Start drawing a frame.
    #[inline]
    pub fn begin_frame(&mut self) -> Frame<'_> {
        let surface = self
            .surface
            .get_current_texture()
            .expect("Failed to get next swap chain texture");
        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let device = Self::device();
        let encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        Frame {
            renderer: self,
            encoder: Some(encoder),
            surface: Some(surface),
            view,
            clear_color: Some(wgpu::Color::BLACK),
            ambient_color: [0.; 3],
            dir_lights: Vec::new(),
            point_lights: Vec::new(),
        }
    }
}

pub struct Frame<'a> {
    renderer: &'a mut Renderer,
    // encoder and surface in Options
    // so that we can take them out on drop without using unsafe
    encoder: Option<wgpu::CommandEncoder>,
    surface: Option<wgpu::SurfaceTexture>,
    view: wgpu::TextureView,
    clear_color: Option<wgpu::Color>,
    // lighting state
    ambient_color: [f32; 3],
    dir_lights: Vec<light::DirectionalLight>,
    point_lights: Vec<light::GpuPointLight>,
}

impl<'a> Frame<'a> {
    /// Set the color the framebuffer will be cleared with
    /// when the shading is executed (i.e. on [`finish`][Self::finish]).
    /// Black by default.
    pub fn set_clear_color(&mut self, color: [f32; 4]) {
        self.clear_color = Some(wgpu::Color {
            r: color[0] as f64,
            g: color[1] as f64,
            b: color[2] as f64,
            a: color[3] as f64,
        });
    }

    /// Set the ambient light color of the scene.
    /// This is light that is applied everywhere regardless of surface direction.
    #[inline]
    pub fn set_ambient_light(&mut self, light_color: [f32; 3]) {
        self.ambient_color = light_color;
    }

    /// Add a directional light.
    #[inline]
    pub fn push_directional_light(&mut self, light: crate::DirectionalLight) {
        self.dir_lights.push(light);
    }

    /// Add directional lights from an iterator.
    #[inline]
    pub fn extend_directional_lights(
        &mut self,
        lights: impl Iterator<Item = crate::DirectionalLight>,
    ) {
        self.dir_lights.extend(lights);
    }

    /// Add a point light.
    #[inline]
    pub fn push_point_light(&mut self, light: crate::PointLight) {
        self.point_lights.push(light::GpuPointLight::from(light));
    }

    /// Add point lights from an iterator.
    #[inline]
    pub fn extend_point_lights(&mut self, lights: impl Iterator<Item = crate::PointLight>) {
        self.point_lights
            .extend(lights.map(light::GpuPointLight::from));
    }

    /// Draw all meshes in the world.
    pub fn draw_meshes(
        &mut self,
        manager: &mut crate::GraphicsManager,
        world: &mut hecs::World,
        camera: &crate::Camera,
    ) {
        // upload lights

        let main_lights = light::MainLights {
            ambient_color: self.ambient_color,
            dir_lights: std::mem::take(&mut self.dir_lights),
        };
        let point_lights = std::mem::take(&mut self.point_lights);
        self.renderer.light_man.write_main_lights(main_lights);
        self.renderer.light_man.write_point_lights(point_lights);

        // compute skins

        let encoder = self.encoder.as_mut().unwrap();
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            self.renderer.skin_pl.compute_skins(&mut cpass, manager);
        }

        // upload mesh data

        self.renderer.mesh_renderer.prepare(manager, world);

        // render depth

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.renderer.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            let mesh_rend = &mut self.renderer.mesh_renderer;
            mesh_rend.depth_pass(&mut rpass, manager, camera);
        }

        // render light emitters and occluders

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lights"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.renderer.gi_pipeline.textures.light,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            let mesh_rend = &mut self.renderer.mesh_renderer;
            mesh_rend.emissive_pass(&mut rpass, manager, camera);
        }

        // compute global illumination

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("global illumination"),
                timestamp_writes: None,
            });

            self.renderer.gi_pipeline.compute_gi(&mut cpass);
        }

        // final render

        {
            let mut rpass = Self::_pass(
                encoder,
                &self.renderer.msaa_view,
                &self.view,
                Some(&self.renderer.depth_view),
                self.clear_color.take(),
            );

            let mesh_rend = &mut self.renderer.mesh_renderer;
            mesh_rend.draw_pass(&mut rpass, manager, camera, &self.renderer.gi_pipeline);
        }
    }

    /// Draw a collection of line strips with the line renderer.
    pub fn draw_lines<'s>(
        &mut self,
        manager: &crate::GraphicsManager,
        camera: &crate::Camera,
        lines: impl IntoIterator<Item = &'s super::line_renderer::LineStrip>,
    ) {
        let line_rend = self
            .renderer
            .line_renderer
            .get_or_insert_with(LineRenderer::new);

        let mut pass = Self::_pass(
            self.encoder.as_mut().unwrap(),
            &self.renderer.msaa_view,
            &self.view,
            Some(&self.renderer.depth_view),
            self.clear_color.take(),
        );
        for line in lines {
            line_rend.draw(&mut pass, manager, camera, line);
        }
    }

    /// Begin a render pass with default parameters.
    pub fn pass(&mut self) -> wgpu::RenderPass<'_> {
        Self::_pass(
            self.encoder.as_mut().unwrap(),
            &self.renderer.msaa_view,
            &self.view,
            Some(&self.renderer.depth_view),
            self.clear_color.take(),
        )
    }

    // these arguments need to be partially borrowed from a &mut Self
    // to avoid lifetime trouble
    fn _pass<'enc, 'view: 'enc>(
        encoder: &'enc mut wgpu::CommandEncoder,
        msaa_view: &'view wgpu::TextureView,
        target_view: &'view wgpu::TextureView,
        depth_view: Option<&'view wgpu::TextureView>,
        clear_color: Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'enc> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: msaa_view,
                resolve_target: Some(target_view),
                ops: wgpu::Operations {
                    load: if let Some(color) = clear_color {
                        wgpu::LoadOp::Clear(color)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_view.map(|depth_view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        })
    }

    /// Access the command encoder recording commands for this frame.
    #[inline]
    pub fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder.as_mut().unwrap()
    }
}

impl<'a> Drop for Frame<'a> {
    fn drop(&mut self) {
        let queue = Renderer::queue();
        queue.submit(Some(self.encoder.take().unwrap().finish()));
        self.surface.take().unwrap().present();
    }
}
