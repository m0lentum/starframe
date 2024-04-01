use std::sync::OnceLock;

mod deferred;
pub use deferred::{DeferredContext, DeferredPass, GBuffer, GBuffers, PostShadeContext};

mod shading;
use shading::ShadingPipeline;
pub use shading::{DirectionalLight, PointLight};

// there is only ever one wgpu context,
// and since the device and queue are frequently needed to create resources,
// we store those globally here
// so that the user doesn't have to ferry them around constantly

static DEVICE: OnceLock<wgpu::Device> = OnceLock::new();
static QUEUE: OnceLock<wgpu::Queue> = OnceLock::new();
static WINDOW: OnceLock<winit::window::Window> = OnceLock::new();

pub const SWAPCHAIN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
// constant number of samples for now,
// TODO: make this configurable
const MSAA_SAMPLES: u32 = 4;

/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    window_scale_factor: f64,

    /// GBuffers for deferred shading.
    pub gbufs: GBuffers,
    deferred_shading_pl: ShadingPipeline,

    msaa_samples: u32,
    // MSAA texture for drawing that happens after deferred shading
    msaa_view: wgpu::TextureView,
    // current active frame stored here instead of in RenderContext
    // so that we can interleave drawing to window and drawing to textures
    active_frame: Option<Frame>,

    /// Index incremented whenever the window is resized,
    /// used to notify rendering subsystems to update their internal textures.
    pub generation: usize,
}

struct Frame {
    surface: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
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
                    limits: wgpu::Limits::default(),
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

        let gbufs = GBuffers::new(window_size.into(), MSAA_SAMPLES);
        let deferred_shading_pl = ShadingPipeline::new(&gbufs, MSAA_SAMPLES);

        let msaa_view = Self::create_msaa_texture((surface_config.width, surface_config.height));

        Ok(Renderer {
            surface,
            surface_config,
            window_scale_factor,
            gbufs,
            deferred_shading_pl,
            generation: 0,
            msaa_samples: MSAA_SAMPLES,
            msaa_view,
            active_frame: None,
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

    fn create_msaa_texture(dimensions: (u32, u32)) -> wgpu::TextureView {
        let tex = deferred::create_texture(
            dimensions,
            MSAA_SAMPLES,
            SWAPCHAIN_FORMAT,
            Some("window msaa"),
        );
        tex.create_view(&wgpu::TextureViewDescriptor::default())
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
        self.gbufs = GBuffers::new(new_size.into(), self.msaa_samples);
        self.deferred_shading_pl
            .update_gbufs_bind_group(&self.gbufs);
        self.msaa_view = Self::create_msaa_texture(new_size.into());
        self.generation += 1;
    }

    #[inline]
    pub fn swapchain_format(&self) -> wgpu::TextureFormat {
        SWAPCHAIN_FORMAT
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

    #[inline]
    pub fn msaa_samples(&self) -> u32 {
        self.msaa_samples
    }

    #[inline]
    pub fn multisample_state(&self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.msaa_samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        }
    }

    /// Depth-stencil state that uses the same depth format as the window depth buffer
    /// and writes depths to the buffer.
    #[inline]
    pub fn default_depth_stencil_state(&self) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: self.gbufs.depth_tex.format(),
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    }

    #[inline]
    pub fn geometry_pass_targets(&self) -> [Option<wgpu::ColorTargetState>; 3] {
        [
            Some(self.gbufs.position.texture.format().into()),
            Some(self.gbufs.normal.texture.format().into()),
            Some(self.gbufs.albedo.texture.format().into()),
        ]
    }

    /// Start drawing a frame.
    ///
    /// The first step of a frame is deferred shading pipeline,
    /// which can be accessed through the returned [`DeferredContext`].
    /// Typically in this phase you would render meshes with
    /// [`MeshRenderer`][crate::MeshRenderer].
    /// To end the deferred phase, call [`DeferredContext::shade`],
    /// which performs shading, draws it into the framebuffer,
    /// and returns a [`PostShadeContext`]
    /// which can be used to draw on top of the framebuffer.
    /// To finish drawing the frame, simply drop the [`PostShadeContext`].
    #[inline]
    pub fn begin_frame(&mut self) -> DeferredContext<'_> {
        assert!(
            self.active_frame.is_none(),
            "Started a frame twice without presenting"
        );
        let surface = self
            .surface
            .get_current_texture()
            .expect("Failed to get next swap chain texture");
        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.active_frame = Some(Frame { surface, view });

        DeferredContext::new(self)
    }

    /// Display everything drawn to the window since the last `present_frame` call.
    /// Called automatically at the end of the frame by [`Game`][crate::Game].
    pub(crate) fn present_frame(&mut self) {
        if let Some(frame) = self.active_frame.take() {
            frame.surface.present();
        }
    }
}
