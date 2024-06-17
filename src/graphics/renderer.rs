use super::{
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

pub const SWAPCHAIN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    window_scale_factor: f64,

    depth_tex: wgpu::Texture,
    depth_view: wgpu::TextureView,
    light_bufs: light::LightBuffers,

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
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits {
                        // TODO we won't need 5 bind groups eventually,
                        // reduce this back to the default
                        max_bind_groups: 5,
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

        let depth_tex = Self::create_depth_texture(window_size);
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let light_bufs = light::LightBuffers::new();
        let mesh_renderer = MeshRenderer::new(&light_bufs);
        let skin_pl = SkinPipeline::new();

        Ok(Renderer {
            surface,
            surface_config,
            window_scale_factor,
            depth_tex,
            depth_view,
            light_bufs,
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
        self.depth_tex = Self::create_depth_texture(new_size);
        self.depth_view = self
            .depth_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
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
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
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
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("deferred"),
        });

        Frame {
            renderer: self,
            encoder: Some(encoder),
            surface: Some(surface),
            view,
            clear_color: Some(wgpu::Color::BLACK),
            main_light: light::MainLight::AmbientOnly([0., 0., 0.]),
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
    main_light: light::MainLight,
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

    /// Set the directional light of the scene.
    ///
    /// Only one of these can be active at a given time.
    pub fn set_directional_light(&mut self, light: crate::DirectionalLight) {
        self.main_light = light::MainLight::Directional(light);
    }

    /// Set the scene to be fully lit with a uniformly colored light
    /// without any directional shading.
    ///
    /// This removes any directional light that was set before.
    pub fn set_ambient_light(&mut self, light_color: [f32; 3]) {
        self.main_light = light::MainLight::AmbientOnly(light_color);
    }

    /// Add a point light.
    pub fn push_point_light(&mut self, light: crate::PointLight) {
        self.point_lights.push(light::GpuPointLight::from(light));
    }

    /// Add point lights from an iterator.
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
        self.renderer.light_bufs.write_main_light(self.main_light);
        let point_lights = std::mem::take(&mut self.point_lights);
        self.renderer.light_bufs.write_point_lights(point_lights);

        let encoder = self.encoder.as_mut().unwrap();

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("skin"),
                timestamp_writes: None,
            });

            self.renderer.skin_pl.compute_skins(&mut cpass, manager);
        }

        {
            let mut rpass = Self::_pass(
                encoder,
                &self.view,
                Some(&self.renderer.depth_view),
                self.clear_color.take(),
            );

            let mesh_rend = &mut self.renderer.mesh_renderer;
            mesh_rend.draw(
                &mut rpass,
                manager,
                world,
                camera,
                &self.renderer.light_bufs,
            );
        }
    }

    /// Draw a collection of line strips with the line renderer.
    pub fn draw_lines(
        &mut self,
        manager: &crate::GraphicsManager,
        camera: &crate::Camera,
        lines: &[super::line_renderer::LineStrip],
    ) {
        let line_rend = self
            .renderer
            .line_renderer
            .get_or_insert_with(LineRenderer::new);

        let mut pass = Self::_pass(
            self.encoder.as_mut().unwrap(),
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
            &self.view,
            Some(&self.renderer.depth_view),
            self.clear_color.take(),
        )
    }

    // these arguments need to be partially borrowed from a &mut Self
    // to avoid lifetime trouble
    fn _pass<'enc, 'view: 'enc>(
        encoder: &'enc mut wgpu::CommandEncoder,
        target_view: &'view wgpu::TextureView,
        depth_view: Option<&'view wgpu::TextureView>,
        clear_color: Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'enc> {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
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
                        load: if clear_color.is_some() {
                            wgpu::LoadOp::Clear(1.)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        pass
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
