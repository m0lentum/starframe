use graphics::Transformed;
use image::{ImageBuffer, Rgba};
use nalgebra::Vector2;
use piston::input::Event;
use piston_window::{G2dTexture, PistonWindow, Texture, TextureSettings};

type Pixel = Rgba<u8>;

pub struct FluidBox {
    width: u32,
    height: u32,
    scaling: f64,
    densities: Vec<f64>,
    velocities: Vec<Vector2<f64>>,
}

impl FluidBox {
    pub fn new(width: u32, height: u32, scaling: f64) -> Self {
        // additional invisible boundary layer around the whole thing
        let num_cells = ((width + 2) * (height + 2)) as usize;
        FluidBox {
            width,
            height,
            scaling,
            densities: vec![0.0; num_cells],
            velocities: vec![Vector2::new(3.0, 3.0); num_cells],
        }
    }

    fn index_at(&self, x: u32, y: u32) -> usize {
        (y * (self.width + 2) + x + 1) as usize
    }

    pub fn _add_sources(&mut self, src: &[f64]) {
        assert!(
            src.len() as u32 == (self.width + 2) * (self.height + 2),
            "Wrong source size"
        );
        for (d, s) in self.densities.iter_mut().zip(src.iter()) {
            *d = *d + *s;
        }
    }

    pub fn add_source_at(&mut self, x: u32, y: u32, amount: f64) {
        let i = self.index_at(x, y);
        self.densities[i] += amount;
    }

    pub fn diffuse(&mut self) {
        //
    }

    pub fn draw_density(&self, evt: &Event, window: &mut PistonWindow) {
        // grayscale where alpha of each square is determined by density
        let mut canvas = ImageBuffer::from_pixel(
            self.width,
            self.height,
            Pixel {
                data: [255, 255, 255, 0],
            },
        );

        for (x, y, pixel) in canvas.enumerate_pixels_mut() {
            pixel.data[3] = (self.densities[self.index_at(x, y)] * 255.0) as u8;
        }

        let texture: G2dTexture =
            Texture::from_image(&mut window.factory, &canvas, &TextureSettings::new()).unwrap();

        window.draw_2d(evt, |ctx, gfx| {
            graphics::image(
                &texture,
                ctx.transform.scale(self.scaling, self.scaling),
                gfx,
            );
        });
    }

    pub fn draw_velocity(&self, evt: &Event, window: &mut PistonWindow) {
        window.draw_2d(evt, |ctx, gfx| {
            for (i, vel) in self.velocities.iter().enumerate() {
                let x = i as u32 % self.width;
                let y = i as u32 / self.width;
                let pos = [x as f64 * self.scaling, y as f64 * self.scaling];
                graphics::line(
                    [1.0, 0.2, 0.1, 0.5],
                    0.5,
                    [pos[0], pos[1], pos[0] + vel.x, pos[1] + vel.y],
                    ctx.transform,
                    gfx,
                );
            }
        });
    }
}
