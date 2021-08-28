/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    swapchain_format: wgpu::TextureFormat,
    window_scale_factor: f64,
}

impl Renderer {
    /// Create a Renderer.
    ///
    /// Most users won't need to create one of these manually;
    /// the `Game`/`GameLoop` API handles it for you.
    pub async fn init(window: &winit::window::Window) -> Self {
        let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
        let surface = unsafe { instance.create_surface(window) };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Renderer init failed: failed to create adapter");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .expect("Failed to create wgpu device");

        let window_size = window.inner_size();
        let swapchain_format = surface
            .get_preferred_format(&adapter)
            .expect("Failed to get swapchain format");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: window_size.width,
            height: window_size.height,
            present_mode: wgpu::PresentMode::Mailbox,
        };
        surface.configure(&device, &surface_config);

        Renderer {
            device,
            queue,
            surface,
            surface_config,
            swapchain_format,
            window_scale_factor: window.scale_factor(),
        }
    }

    #[inline]
    pub fn swapchain_format(&self) -> wgpu::TextureFormat {
        self.swapchain_format
    }

    /// Get the size of the window this Renderer draws to.
    #[inline]
    pub fn window_size(&self) -> winit::dpi::PhysicalSize<u32> {
        winit::dpi::PhysicalSize::new(self.surface_config.width, self.surface_config.height)
    }

    /// Get the scale factor of the window this Renderer draws to.
    /// Useful e.g. when rendering text.
    #[inline]
    pub fn window_scale_factor(&self) -> f64 {
        self.window_scale_factor
    }

    /// Change the size of the frame `draw_to_window` draws into.
    /// This is called automatically by the gameloop when the window size changes.
    #[inline]
    pub fn resize_swap_chain(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Begin drawing directly into the game window.
    pub fn draw_to_window(&mut self) -> RenderContext {
        let frame = self
            .surface
            .get_current_frame()
            .expect("Failed to get next swap chain texture")
            .output;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let target_size = self.window_size().into();
        let queue = &mut self.queue;

        RenderContext {
            view,
            _frame: frame,
            encoder,
            device: &self.device,
            queue,
            target_size,
        }
    }
}

/// An interface that lets you send draw instructions to the GPU.
///
/// TODOC: example
pub struct RenderContext<'a> {
    view: wgpu::TextureView,
    // frame is not accessed but needs to be stored,
    // otherwise it gets dropped and the texture goes with it
    // (specifically AFTER view so it gets dropped after it)
    _frame: wgpu::SurfaceTexture,
    pub encoder: wgpu::CommandEncoder,
    pub device: &'a wgpu::Device,
    pub queue: &'a mut wgpu::Queue,
    pub target_size: (u32, u32),
}

impl<'a> RenderContext<'a> {
    /// Fill the render target with a flat color.
    pub fn clear(&mut self, color: wgpu::Color) {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });
        // drop the pass immediately, causing the clear instruction
        // to be written to the encoder
    }

    /// Begin a render pass.
    pub fn pass(&mut self) -> wgpu::RenderPass {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        })
    }

    /// Submit the commands made through this context to the GPU.
    pub fn submit(self) {
        self.queue.submit(Some(self.encoder.finish()));
    }
}
