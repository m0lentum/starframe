/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    swap_chain: wgpu::SwapChain,
    swap_chain_descriptor: wgpu::SwapChainDescriptor,
    window_scale_factor: f64,
}

impl Renderer {
    /// Create a Renderer.
    ///
    /// Most users won't need to create one of these manually;
    /// the `Game`/`GameLoop` API handles it for you.
    pub async fn init(window: &winit::window::Window) -> Self {
        let surface = wgpu::Surface::create(window);

        let adapter = wgpu::Adapter::request(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::Default,
                compatible_surface: Some(&surface),
            },
            wgpu::BackendBit::PRIMARY,
        )
        .await
        .expect("Renderer init failed: failed to create adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                extensions: wgpu::Extensions {
                    anisotropic_filtering: false,
                },
                limits: wgpu::Limits::default(),
            })
            .await;

        let window_size = window.inner_size();
        let swap_chain_descriptor = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: window_size.width,
            height: window_size.height,
            present_mode: wgpu::PresentMode::Mailbox,
        };
        let swap_chain = device.create_swap_chain(&surface, &swap_chain_descriptor);

        Renderer {
            device,
            queue,
            surface,
            swap_chain,
            swap_chain_descriptor,
            window_scale_factor: window.scale_factor(),
        }
    }

    /// Get the size of the window this Renderer draws to.
    pub fn window_size(&self) -> winit::dpi::PhysicalSize<u32> {
        winit::dpi::PhysicalSize::new(
            self.swap_chain_descriptor.width,
            self.swap_chain_descriptor.height,
        )
    }

    /// Get the scale factor of the window this Renderer draws to.
    /// Useful e.g. when rendering text.
    pub fn window_scale_factor(&self) -> f64 {
        self.window_scale_factor
    }

    /// Change the size of the frame `draw_to_window` draws into.
    /// This is called automatically by the gameloop when the window size changes.
    pub fn resize_swap_chain(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.swap_chain_descriptor.width = new_size.width;
        self.swap_chain_descriptor.height = new_size.height;
        self.swap_chain = self
            .device
            .create_swap_chain(&self.surface, &self.swap_chain_descriptor);
    }

    /// Begin drawing directly into the game window.
    pub fn draw_to_window(&mut self) -> RenderContext {
        let frame = self
            .swap_chain
            .get_next_texture()
            .expect("Failed to get next swap chain texture");
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let target_size = self.window_size().into();
        let queue = &mut self.queue;

        RenderContext {
            target: RenderTarget::Window(frame),
            encoder,
            device: &self.device,
            queue,
            target_size,
        }
    }
}

enum RenderTarget {
    Window(wgpu::SwapChainOutput),
    Texture(wgpu::TextureView),
}
impl RenderTarget {
    fn view(&self) -> &wgpu::TextureView {
        match self {
            RenderTarget::Window(frame) => &frame.view,
            RenderTarget::Texture(view) => view,
        }
    }
}

/// An interface that lets you send draw instructions to the GPU.
///
/// TODOC: example
pub struct RenderContext<'a> {
    target: RenderTarget,
    pub encoder: wgpu::CommandEncoder,
    pub device: &'a wgpu::Device,
    queue: &'a mut wgpu::Queue,
    pub target_size: (u32, u32),
}

impl<'a> RenderContext<'a> {
    /// Fill the render target with a flat color.
    pub fn clear(&mut self, color: wgpu::Color) {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                attachment: self.target.view(),
                resolve_target: None,
                load_op: wgpu::LoadOp::Clear,
                store_op: wgpu::StoreOp::Store,
                clear_color: color,
            }],
            depth_stencil_attachment: None,
        });
        // drop the pass immediately, causing the clear instruction
        // to be written to the encoder
    }

    /// Begin a render pass.
    pub fn pass(&mut self) -> wgpu::RenderPass {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                attachment: self.target.view(),
                resolve_target: None,
                load_op: wgpu::LoadOp::Load,
                store_op: wgpu::StoreOp::Store,
                clear_color: wgpu::Color::TRANSPARENT,
            }],
            depth_stencil_attachment: None,
        })
    }

    /// Submit the commands made through this context to the GPU.
    pub fn submit(self) {
        let commands = self.encoder.finish();
        self.queue.submit(&[commands]);
    }
}
