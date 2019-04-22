pub mod shape;

pub mod shaders;

//

pub type Color = [f32; 4];

#[derive(Copy, Clone, Default)]
pub struct Vertex2D {
    v_position: [f32; 2],
}

glium::implement_vertex!(Vertex2D, v_position);
