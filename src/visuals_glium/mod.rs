pub mod shape;
pub use shape::{Shape, ShapeStyle, ShapeRenderer};

pub mod shaders;
pub use shaders::Shaders;

mod context;
pub use context::Context;

#[cfg(feature = "debug_visuals")]
pub mod debug;

//

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

impl From<nalgebra::Vector2<f32>> for Vertex2D {
    fn from(pos: nalgebra::Vector2<f32>) -> Self {
        Vertex2D {
            v_position: [pos[0], pos[1]],
        }
    }
}

impl From<nalgebra::Point2<f32>> for Vertex2D {
    fn from(pos: nalgebra::Point2<f32>) -> Self {
        Vertex2D {
            v_position: [pos[0], pos[1]],
        }
    }
}

glium::implement_vertex!(Vertex2D, v_position);

