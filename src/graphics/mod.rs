pub mod camera;

pub mod shape;
pub use shape::{Shape, ShapeFeature, ShapeRenderer, ShapeStyle};

pub mod shaders;
pub use shaders::Shaders;

mod context;
pub use context::Context;

pub mod debug;

//

use ultraviolet as uv;

pub type Color = [f32; 4];

#[derive(Copy, Clone, Default)]
pub struct Vertex2D {
    v_position: [f32; 2],
}

impl From<[f32; 2]> for Vertex2D {
    fn from(v_position: [f32; 2]) -> Self {
        Vertex2D { v_position }
    }
}

impl From<uv::Vec2> for Vertex2D {
    fn from(pos: uv::Vec2) -> Self {
        Vertex2D {
            v_position: [pos.x, pos.y],
        }
    }
}

glium::implement_vertex!(Vertex2D, v_position);
