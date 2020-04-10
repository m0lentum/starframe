pub struct Renderer {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    swap_chain: wgpu::SwapChain,
    swap_chain_descriptor: wgpu::SwapChainDescriptor,
}

impl Renderer {
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
        .expect("Failed to create adapter");

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
            swap_chain,
            swap_chain_descriptor,
        }
    }

    pub fn draw_frame(
        &mut self,
        draw: impl FnOnce(&wgpu::SwapChainOutput, &wgpu::Device) -> wgpu::CommandBuffer,
    ) {
        let frame = self
            .swap_chain
            .get_next_texture()
            .expect("Failed to get next swap chain texture");

        let commands = draw(&frame, &self.device);
        self.queue.submit(&[commands]);
    }

    pub fn draw_to_window(&mut self) -> RenderContext {
        let frame = self
            .swap_chain
            .get_next_texture()
            .expect("Failed to get next swap chain texture");
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let queue = &mut self.queue;

        RenderContext {
            target: RenderTarget::Window(frame),
            encoder,
            queue,
        }
    }
}

enum RenderTarget {
    Window(wgpu::SwapChainOutput),
    _Texture(wgpu::TextureView),
}
impl RenderTarget {
    fn view(&self) -> &wgpu::TextureView {
        match self {
            RenderTarget::Window(frame) => &frame.view,
            RenderTarget::_Texture(view) => view,
        }
    }
}

pub struct RenderContext<'a> {
    target: RenderTarget,
    pub encoder: wgpu::CommandEncoder,
    queue: &'a mut wgpu::Queue,
}

impl<'a> RenderContext<'a> {
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

    pub fn submit(self) {
        let commands = self.encoder.finish();
        self.queue.submit(&[commands]);
    }
}
