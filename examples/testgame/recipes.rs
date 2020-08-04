use starframe::{graphics as gx, math as m, physics as phys};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    StaticBlock(Block),
    DynamicBlock(Block),
    Ball { radius: f32, position: [f32; 2] },
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct Block {
    pub width: f32,
    pub height: f32,
    pub transform: m::TransformBuilder,
}

impl Recipe {
    pub fn spawn(&self, graph: &mut crate::MyGraph) {
        use Recipe::*;
        match self {
            Player(p_rec) => p_rec.spawn(graph),
            StaticBlock(block) => {
                let tr_node = graph
                    .l_transform
                    .insert(block.transform.into(), &mut graph.graph);
                let coll = phys::Collider::new_rect(block.width, block.height);
                let body = phys::RigidBody::new_static();
                let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
                let body_node = graph.l_body.insert(body, &mut graph.graph);
                let shape_node = graph.l_shape.insert(
                    gx::Shape::Rect {
                        w: block.width,
                        h: block.height,
                        color: [0.5; 4],
                    },
                    &mut graph.graph,
                );
                // TODO: helper to create this graph pattern in the starframe::physics module
                graph.graph.connect(&tr_node, &body_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&tr_node, &shape_node);
            }
            DynamicBlock(block) => {
                let tr_node = graph
                    .l_transform
                    .insert(block.transform.into(), &mut graph.graph);
                let coll = phys::Collider::new_rect(block.width, block.height);
                let body = phys::RigidBody::new_dynamic(&coll, 1.0);
                let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
                let body_node = graph.l_body.insert(body, &mut graph.graph);
                let shape_node = graph.l_shape.insert(
                    gx::Shape::Rect {
                        w: block.width,
                        h: block.height,
                        color: [1.0; 4],
                    },
                    &mut graph.graph,
                );
                graph.graph.connect(&tr_node, &body_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&tr_node, &shape_node);
            }
            Ball { radius, position } => {
                let tr_node = graph.l_transform.insert(
                    m::TransformBuilder::from(*position).into(),
                    &mut graph.graph,
                );
                let coll = phys::Collider::new_circle(*radius);
                let body = phys::RigidBody::new_dynamic(&coll, 0.5);
                let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
                let body_node = graph.l_body.insert(body, &mut graph.graph);
                let shape_node = graph.l_shape.insert(
                    gx::Shape::Circle {
                        r: *radius,
                        points: 24,
                        color: [1.0; 4],
                    },
                    &mut graph.graph,
                );
                graph.graph.connect(&tr_node, &body_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&tr_node, &shape_node);
            }
        }
    }

    pub fn read_from_file(
        file: std::fs::File,
        graph: &mut crate::MyGraph,
    ) -> Result<(), ron::de::Error> {
        use serde::Deserialize;
        use std::io::Read;

        let mut reader = std::io::BufReader::new(file);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;

        let mut deser = ron::de::Deserializer::from_bytes(bytes.as_slice())?;
        let file_content = Vec::<Recipe>::deserialize(&mut deser)?;
        for recipe in file_content {
            recipe.spawn(graph);
        }

        Ok(())
    }
}
