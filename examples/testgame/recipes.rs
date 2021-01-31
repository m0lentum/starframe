use starframe::{
    self as sf, graphics as gx,
    math::{self as m, uv},
    physics as phys,
};

use rand::{distributions as distr, distributions::Distribution};

#[derive(Clone, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    StaticBlock(Block),
    DynamicBlock(Block),
    Ball {
        radius: f32,
        position: [f32; 2],
    },
    Blockchain {
        width: f32,
        spacing: f32,
        links: Vec<[f32; 2]>,
        anchored_start: bool,
        anchored_end: bool,
    },
    Oscillator {
        position: [f32; 2],
        begin_length: f32,
        target_length: f32,
        frequency: f32,
        damping: f32,
    },
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Block {
    pub width: f32,
    pub height: f32,
    pub transform: m::IsometryBuilder,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            transform: Default::default(),
        }
    }
}

fn spawn_block(
    block: Block,
    color: [f32; 4],
    is_static: bool,
    graph: &mut crate::MyGraph,
) -> sf::graph::Node<phys::RigidBody> {
    let tr_node = graph
        .l_transform
        .insert(block.transform.into(), &mut graph.graph);
    let coll = phys::Collider::new_rect(block.width, block.height);
    let body = if is_static {
        phys::RigidBody::new_static()
    } else {
        phys::RigidBody::new_dynamic(&coll, 0.5)
    };
    let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
    let body_node = graph.l_body.insert(body, &mut graph.graph);
    let shape_node = graph.l_shape.insert(
        gx::Shape::Rect {
            w: block.width,
            h: block.height,
            color,
        },
        &mut graph.graph,
    );
    graph.graph.connect(&tr_node, &body_node);
    graph.graph.connect(&body_node, &coll_node);
    graph.graph.connect(&tr_node, &shape_node);

    sf::graph::NodeRef::as_node(&body_node, &graph.graph)
}

impl Recipe {
    pub fn spawn(&self, graph: &mut crate::MyGraph, physics: &mut phys::Physics) {
        use Recipe::*;
        match self {
            Player(p_rec) => p_rec.spawn(graph),
            StaticBlock(block) => {
                spawn_block(*block, [0.5; 4], true, graph);
            }
            DynamicBlock(block) => {
                spawn_block(*block, random_color(), false, graph);
            }
            Ball { radius, position } => {
                let tr_node = graph.l_transform.insert(
                    uv::Isometry2::new(position.into(), uv::Rotor2::identity()),
                    &mut graph.graph,
                );
                let coll = phys::Collider::new_circle(*radius);
                let body = phys::RigidBody::new_dynamic(&coll, 0.5);
                let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
                let body_node = graph.l_body.insert(body, &mut graph.graph);
                let shape_node = graph.l_shape.insert(
                    gx::Shape::Circle {
                        r: *radius,
                        points: 16,
                        color: random_color(),
                    },
                    &mut graph.graph,
                );
                graph.graph.connect(&tr_node, &body_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&tr_node, &shape_node);
            }
            Blockchain {
                width,
                spacing,
                links,
                anchored_start,
                anchored_end,
            } => {
                if links.len() < 2 {
                    println!("Too few links in a chain");
                    return;
                }

                let mut links_iter = links.iter().map(|p| uv::Vec2::new(p[0], p[1])).peekable();

                // to connect another block to it
                let mut prev_block: Option<(sf::graph::Node<phys::RigidBody>, f32)> = None;
                while let (Some(link1), Some(link2)) = (links_iter.next(), links_iter.peek()) {
                    let distance = *link2 - link1;
                    let dist_norm = distance.mag();
                    let center = (link1 + *link2) / 2.0;
                    let orientation = (distance[0] / dist_norm).acos() * distance[1].signum();

                    let block_length = dist_norm - spacing;
                    let block = spawn_block(
                        Block {
                            width: block_length,
                            height: *width, // a bit weird but makes sense with the orientation calculations
                            transform: m::IsometryBuilder::new()
                                .with_position(center)
                                .with_rotation(m::Angle::Rad(orientation)),
                        },
                        random_color(),
                        false,
                        graph,
                    );
                    let block_length_half = block_length / 2.0;
                    if let Some((prev_block, prev_block_offset)) = prev_block {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(block)
                                .with_target(prev_block)
                                .with_origin(uv::Vec2::new(-block_length_half, 0.0))
                                .with_target_origin(uv::Vec2::new(prev_block_offset, 0.0))
                                .inequality_lt()
                                .soft(phys::OscillatorParams {
                                    frequency: 20.0,
                                    damping: 1.0,
                                })
                                .build_distance(*spacing),
                        );
                    } else if *anchored_start {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(block)
                                .with_origin(uv::Vec2::new(
                                    -block_length_half - (spacing / 2.0),
                                    0.0,
                                ))
                                .with_target_origin(link1)
                                .build_distance(0.0),
                        );
                    }
                    prev_block = Some((block, block_length_half));
                }

                if *anchored_end {
                    let (prev_block, prev_block_offset) = prev_block.unwrap();
                    physics.add_constraint(
                        phys::ConstraintBuilder::new(prev_block)
                            .with_origin(uv::Vec2::new(prev_block_offset + (spacing / 2.0), 0.0))
                            .with_target_origin(
                                links
                                    .iter()
                                    .map(|p| uv::Vec2::new(p[0], p[1]))
                                    .last()
                                    .unwrap(),
                            )
                            .build_distance(0.0),
                    );
                }
            }
            Oscillator {
                position,
                begin_length,
                target_length,
                frequency,
                damping,
            } => {
                let position: uv::Vec2 = position.into();
                let offset = uv::Vec2::new(begin_length / 2.0, 0.0);
                let b1 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        transform: m::IsometryBuilder::new().with_position(position + offset),
                    },
                    random_color(),
                    false,
                    graph,
                );
                let b2 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        transform: m::IsometryBuilder::new().with_position(position - offset),
                    },
                    random_color(),
                    false,
                    graph,
                );
                physics.add_constraint(
                    phys::ConstraintBuilder::new(b1)
                        .with_target(b2)
                        .soft(phys::OscillatorParams {
                            frequency: *frequency,
                            damping: *damping,
                        })
                        .build_distance(*target_length),
                );
            }
        }
    }
}

fn random_color() -> [f32; 4] {
    let mut rng = rand::thread_rng();
    [
        distr::Uniform::from(0.4..1.0).sample(&mut rng),
        distr::Uniform::from(0.4..1.0).sample(&mut rng),
        distr::Uniform::from(0.4..1.0).sample(&mut rng),
        1.0,
    ]
}
