/// A Renderer manages resources needed to draw graphics to the screen.
pub struct Renderer {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    surface_config: wgpu::SurfaceConfiguration,
    swapchain_format: wgpu::TextureFormat,
    window_scale_factor: f64,

    /// Depth buffer automatically kept in sync with the swapchain size.
    pub window_depth_buffer: super::DepthBuffer,

    msaa_samples: u32,
    // texture to store multisampling results
    msaa_texture: wgpu::Texture,
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
    msaa_view: wgpu::TextureView,
}

impl Renderer {
    /// Create a Renderer.
    /// The [`Game`][crate::game::Game] API does this automatically.
    pub(crate) async fn init(window: &winit::window::Window) -> Self {
        let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
        let surface = unsafe { instance.create_surface(window) };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
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

        // surface.get_preferred_format gives a non-SRGB format on wasm
        // which screws up colors. not sure if setting it to a constant
        // is the correct solution but it works on my machines :v)
        let swapchain_format = wgpu::TextureFormat::Bgra8UnormSrgb;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: window_size.width,
            height: window_size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        surface.configure(&device, &surface_config);

        // constant number of samples for now,
        // TODO: make this configurable
        const MSAA_SAMPLES: u32 = 4;
        let msaa_texture =
            Self::create_msaa_texture(&device, swapchain_format, MSAA_SAMPLES, window_size);

        let window_depth_buffer = super::DepthBuffer::new(
            &device,
            window_size.into(),
            MSAA_SAMPLES,
            Some("global depth buffer made on init"),
        );

        Renderer {
            device,
            queue,
            surface,
            surface_config,
            swapchain_format,
            window_scale_factor: window.scale_factor(),
            window_depth_buffer,
            generation: 0,
            msaa_samples: MSAA_SAMPLES,
            msaa_texture,
            active_frame: None,
        }
    }

    fn create_msaa_texture(
        device: &wgpu::Device,
        swapchain_format: wgpu::TextureFormat,
        msaa_samples: u32,
        window_size: winit::dpi::PhysicalSize<u32>,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen multisample"),
            size: wgpu::Extent3d {
                width: window_size.width,
                height: window_size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: msaa_samples,
            dimension: wgpu::TextureDimension::D2,
            format: swapchain_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        })
    }

    /// Change the size of the frame `draw_to_window` draws into.
    /// This is called automatically by the gameloop when the window size changes.
    pub(crate) fn resize_swap_chain(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size == self.window_size() {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
        self.msaa_texture = Self::create_msaa_texture(
            &self.device,
            self.swapchain_format,
            self.msaa_samples,
            new_size,
        );
        self.window_depth_buffer = super::DepthBuffer::new(
            &self.device,
            new_size.into(),
            self.msaa_samples,
            Some("global depth buffer made on resize"),
        );
        self.generation += 1;
    }

    #[inline]
    pub fn swapchain_format(&self) -> wgpu::TextureFormat {
        self.swapchain_format
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

    /// Begin drawing directly into the game window.
    pub fn draw_to_window(&mut self) -> RenderContext<'_> {
        // start a new frame if this is the first time we're drawing to the window
        // since last present
        if self.active_frame.is_none() {
            let surface = self
                .surface
                .get_current_texture()
                .expect("Failed to get next swap chain texture");
            let view = surface
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let msaa_view = self
                .msaa_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            self.active_frame = Some(Frame {
                surface,
                view,
                msaa_view,
            });
        }
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let target_size = self.window_size().into();
        let queue = &mut self.queue;

        // active frame was just set so unwrap is safe
        let active_frame = self.active_frame.as_ref().unwrap();
        let target = RenderTarget {
            view: &active_frame.msaa_view,
            resolve_target: Some(&active_frame.view),
            depth: Some(&self.window_depth_buffer.view),
        };

        RenderContext {
            target,
            encoder: CommandEncoder(encoder),
            device: &self.device,
            queue,
            target_size,
            submit_check: SubmitCheck::new(),
        }
    }

    /// Begin drawing to a non-screen texture, optionally with a self-provided depth texture.
    ///
    /// If you need the depth texture from the window, use
    /// [`draw_to_texture_window_depth`][Self::draw_to_texture_window_depth]
    pub fn draw_to_texture<'s, 'v: 's>(
        &'s mut self,
        view: &'v wgpu::TextureView,
        depth_target: Option<&'v wgpu::TextureView>,
        target_size: (u32, u32),
    ) -> RenderContext<'s> {
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let queue = &mut self.queue;

        RenderContext {
            target: RenderTarget {
                view,
                resolve_target: None,
                depth: depth_target,
            },
            encoder: CommandEncoder(encoder),
            device: &self.device,
            queue,
            target_size,
            submit_check: SubmitCheck::new(),
        }
    }

    /// Display everything drawn to the window since the last `present_frame` call.
    /// Must be called at the end of every frame.
    pub fn present_frame(&mut self) {
        if let Some(frame) = self.active_frame.take() {
            frame.surface.present();
        }
    }
}

pub struct RenderTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub resolve_target: Option<&'a wgpu::TextureView>,
    pub depth: Option<&'a wgpu::TextureView>,
}

/// An interface that lets you send draw instructions to the GPU.
///
/// You **must** call [`submit`](Self::submit) when you drop the context.
/// Not doing so would result in a memory leak, so
/// `RenderContext` will panic on drop if you do this.
pub struct RenderContext<'a> {
    pub target: RenderTarget<'a>,
    pub encoder: CommandEncoder,
    pub device: &'a wgpu::Device,
    pub queue: &'a mut wgpu::Queue,
    pub target_size: (u32, u32),
    // this is just used to warn if a context was dropped without submitting
    submit_check: SubmitCheck,
}

impl<'a> RenderContext<'a> {
    /// Fill the render target with a flat color.
    ///
    /// If you need access to other fields of the RenderContext, this method also exists on the
    /// `encoder` so you can partial borrow when needed.
    #[inline]
    pub fn clear(&mut self, color: wgpu::Color) {
        self.encoder.clear(&self.target, color)
    }

    /// Begin a render pass that draws on top of what's already in the target
    /// and uses the depth buffer.
    ///
    /// If you need access to other fields of the RenderContext, this method also exists on the
    /// `encoder` so you can partial borrow when needed.
    #[inline]
    pub fn pass(&mut self, label: Option<&'static str>) -> wgpu::RenderPass {
        self.encoder.pass(&self.target, label)
    }

    /// Begin a render pass that draws on top of what's already in the target
    /// and ignores (i.e. doesn't bind at all) the depth buffer.
    ///
    /// If you need access to other fields of the RenderContext, this method also exists on the
    /// `encoder` so you can partial borrow when needed.
    #[inline]
    pub fn pass_without_depth(&mut self, label: Option<&'static str>) -> wgpu::RenderPass {
        self.encoder.pass_without_depth(&self.target, label)
    }

    /// Submit the commands made through this context to the GPU.
    /// Must be called or nothing is actually executed!
    pub fn submit(mut self) {
        self.queue.submit(Some(self.encoder.0.finish()));
        self.submit_check.0 = true;
    }
}

/// A wrapper around [`wgpu::CommandEncoder`][wgpu::CommandEncoder]
/// to facilitate creation of render passes with default parameters
/// while also partial borrowing other fields from [`RenderContext`][self::RenderContext].
pub struct CommandEncoder(pub wgpu::CommandEncoder);

impl CommandEncoder {
    /// Fill the render target with a flat color.
    pub fn clear(&mut self, target: &RenderTarget, color: wgpu::Color) {
        self.0.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: true,
                },
            })],
            depth_stencil_attachment: target.depth.map(|depth| {
                wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: true,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: true,
                    }),
                }
            }),
        });
        // drop the pass immediately, causing the clear instruction
        // to be written to the encoder
    }

    /// Begin a render pass that draws on top of what's already in the target
    /// and uses the depth buffer.
    #[inline]
    pub fn pass<'s, 't: 's>(
        &'s mut self,
        target: &'s RenderTarget<'t>,
        label: Option<&'static str>,
    ) -> wgpu::RenderPass {
        self._pass(target, true, label)
    }

    /// Begin a render pass that draws on top of what's already in the target
    /// and ignores (i.e. doesn't bind at all) the depth buffer.
    #[inline]
    pub fn pass_without_depth<'s, 't: 's>(
        &'s mut self,
        target: &'s RenderTarget<'t>,
        label: Option<&'static str>,
    ) -> wgpu::RenderPass {
        self._pass(target, false, label)
    }

    fn _pass<'s, 't: 's>(
        &'s mut self,
        target: &'s RenderTarget<'t>,
        use_depth: bool,
        label: Option<&'static str>,
    ) -> wgpu::RenderPass {
        self.0.begin_render_pass(&wgpu::RenderPassDescriptor {
            label,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                resolve_target: target.resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: true,
                },
            })],
            depth_stencil_attachment: if !use_depth {
                None
            } else {
                target
                    .depth
                    .map(|depth| wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: true,
                        }),
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: true,
                        }),
                    })
            },
        })
    }
}

/// `RenderContext::submit` requires taking ownership and destructuring,
/// which makes submitting on drop too annoying.
/// Instead, use this to panic if the user drops a context wrong.
struct SubmitCheck(bool);
impl SubmitCheck {
    fn new() -> Self {
        Self(false)
    }
}
impl Drop for SubmitCheck {
    fn drop(&mut self) {
        assert!(self.0, "Dropped a RenderContext without submitting");
    }
}
