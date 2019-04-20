use glium::{index::PrimitiveType, Display, IndexBuffer, Surface, VertexBuffer};
use nalgebra::Vector2;

use super::Color;
use crate::ecs::system::*;
use crate::physics2d::collider::{Collider, COLLIDER_MAX_VERTS};
use crate::util::Transform;

#[derive(Copy, Clone, Default)]
struct Vertex {
    position: [f32; 2],
}

glium::implement_vertex!(Vertex, position);

impl From<Vector2<f32>> for Vertex {
    fn from(vector: Vector2<f32>) -> Self {
        Vertex {
            position: [vector[0], vector[1]],
        }
    }
}

pub struct ColliderVisualizer {
    vb: VertexBuffer<Vertex>,
    ib: IndexBuffer<u16>,
    capacity: usize,
    style: RenderStyle,
    color: Color,
}

impl ColliderVisualizer {
    pub fn new(display: &Display, style: RenderStyle, color: Color, capacity: usize) -> Self {
        let vb = VertexBuffer::empty_dynamic(display, capacity * COLLIDER_MAX_VERTS)
            .expect("Failed to create vertex buffer");
        let ib = IndexBuffer::empty_dynamic(
            display,
            match style {
                RenderStyle::Lines => PrimitiveType::LineStrip,
                RenderStyle::Fill => PrimitiveType::TriangleFan,
            },
            capacity * (COLLIDER_MAX_VERTS + 1),
        )
        .expect("Failed to create index buffer");

        ColliderVisualizer {
            vb,
            ib,
            capacity,
            style,
            color,
        }
    }

    pub fn draw_space(&mut self, surface: impl Surface, shader: glium::Program, space: &Space) {
        space.run_filter(|items: &mut [ColliderFilter]| {
            let mut verts: Vec<Vertex> = Vec::with_capacity(self.capacity * COLLIDER_MAX_VERTS);
            let mut indices: Vec<u16> =
                Vec::with_capacity(self.capacity * (COLLIDER_MAX_VERTS + 1));
            for item in items {
                match item.coll {
                    Collider::Circle { r } => {
                        for vert in CIRCLE_POINTS.iter() {
                            verts.push(Vertex::from(item.tr.0 * (*r * vert)));
                            indices.push(verts.len() as u16);
                        }

                        if let RenderStyle::Lines = self.style {
                            indices.push((verts.len() - CIRCLE_POINTS.len()) as u16);
                        }
                    }
                    Collider::Rect { hw, hh } => {
                        let corners = vec![
                            Vector2::new(-hw, -hh),
                            Vector2::new(*hw, -hh),
                            Vector2::new(*hw, *hh),
                            Vector2::new(-hw, *hh),
                        ];
                        for corner in corners {
                            indices.push(verts.len() as u16);
                            verts.push(Vertex::from(item.tr.0 * corner));
                        }

                        if let RenderStyle::Lines = self.style {
                            indices.push((verts.len() - 4) as u16);
                        }
                    }
                }
                indices.push(std::u16::MAX); // primitive restart
            }

            self.vb.write(&verts);
            self.ib.write(&indices);
            // TODO: shader & uniforms
        });
    }
}

pub enum RenderStyle {
    Lines,
    Fill,
}

#[derive(ComponentFilter)]
pub struct ColliderFilter<'a> {
    coll: &'a Collider,
    tr: &'a Transform,
}

lazy_static::lazy_static! {
    /// All circles are the same so we can precalculate their vertices
    static ref CIRCLE_POINTS: Vec<Vector2<f32>> = {
        let angle_incr = 2.0 * std::f32::consts::PI / COLLIDER_MAX_VERTS as f32;
        (0..COLLIDER_MAX_VERTS).map(|i| {
            let angle = angle_incr * i as f32;
            Vector2::new(angle.cos(), angle.sin())
        }).collect()
    };
}
