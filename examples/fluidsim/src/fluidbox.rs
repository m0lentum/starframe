use graphics::Transformed;
use image::{ImageBuffer, Rgba};
use nalgebra::Vector2;
use piston::input::Event;
use piston_window::{G2dTexture, PistonWindow, Texture, TextureSettings};

type Pixel = Rgba<u8>;

pub struct FluidBox {
    width: u32,
    height: u32,
    densities: Vec<f32>,
    velocities: Vec<Vector2<f32>>,
}

impl FluidBox {
    pub fn new(width: u32, height: u32) -> Self {
        // additional invisible boundary layer around the whole thing
        let num_cells = ((width + 2) * (height + 2)) as usize;
        FluidBox {
            width,
            height,
            densities: vec![0.0; num_cells],
            velocities: vec![Vector2::zeros(); num_cells],
        }
    }

    pub fn draw_density(&self, scale: f64, evt: &Event, window: &mut PistonWindow) {
        // grayscale where alpha of each square is determined by density
        let mut canvas = ImageBuffer::from_pixel(
            self.width,
            self.height,
            Pixel {
                data: [255, 255, 255, 0],
            },
        );

        for (x, y, pixel) in canvas.enumerate_pixels_mut() {
            pixel.data[3] = (self.densities[(y * self.width + x) as usize] * 255.0) as u8;
        }

        let texture: G2dTexture =
            Texture::from_image(&mut window.factory, &canvas, &TextureSettings::new()).unwrap();

        window.draw_2d(evt, |ctx, gfx| {
            graphics::image(&texture, ctx.transform.scale(scale, scale), gfx);
        });
    }
}
