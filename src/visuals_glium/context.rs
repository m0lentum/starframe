use super::Shaders;

static mut CONTEXT_SINGLETON: Option<Context> = None;

/// A Context is responsible for holding the Glium game window, shaders, and event loop.
/// It is a globally accessible singleton that must be initialized before being used.
pub struct Context {
    pub display: glium::Display,
    pub shaders: Shaders,
}

impl<'a> Context {
    /// Create a Context singleton and return the associated EventsLoop.
    /// This is because polling the EventsLoop requires mutable access,
    /// but only immutable access to the Context singleton is safe.
    /// # Safety
    /// This is unsafe because it uses a mutable static internally and can therefore cause data races.
    /// A Context is typically very rarely mutated (only at startup and after changing graphics settings),
    /// so you probably don't need to worry about this much.
    pub unsafe fn init() -> glutin::EventsLoop {
        let events = glutin::EventsLoop::new();
        // TODO: get this stuff from settings
        let window = glutin::WindowBuilder::new()
            .with_title("MoleEngine")
            .with_dimensions(glutin::dpi::LogicalSize::new(800.0, 600.0));
        let context = glutin::ContextBuilder::new();
        let display =
            glium::Display::new(window, context, &events).expect("Failed to create display");

        let shaders = Shaders::compile(&display).expect("Failed to compile shader");

        CONTEXT_SINGLETON = Some(Context { display, shaders });

        events
    }

    /// Get an immutable reference to the global Context object.
    /// # Panics
    /// Panics if the Context has not been initialized.
    pub fn get() -> &'a Self {
        unsafe {
            if let Some(ctx) = CONTEXT_SINGLETON.as_ref() {
                &ctx
            } else {
                panic!("Access of uninitialized rendering context");
            }
        }
    }
}
