use starframe::{
    self as sf, graphics as gx, math as m,
    physics::{self as phys, rigidbody::SurfaceMaterial, Velocity},
};

use rand::{distributions as distr, distributions::Distribution};

#[derive(Clone, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    StaticBlock(Block),
    DynamicBlock(Block),
    Ball(Ball),
    Blockchain {
        width: f64,
        spacing: f64,
        links: Vec<[f64; 2]>,
        anchored_start: bool,
        anchored_end: bool,
    },
    Oscillator {
        position: [f64; 2],
        begin_length: f64,
        target_length: f64,
        compliance: f64,
    },
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Ball {
    pub radius: f64,
    pub position: [f64; 2],
    pub restitution: f64,
    pub start_velocity: [f64; 2],
}

impl Default for Ball {
    fn default() -> Self {
        Self {
            radius: 1.0,
            position: [0.0, 0.0],
            restitution: 0.0,
            start_velocity: [0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Block {
    pub width: f64,
    pub height: f64,
    pub pose: m::IsometryBuilder,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            pose: Default::default(),
        }
    }
}

fn spawn_block(
    block: Block,
    color: [f32; 4],
    is_static: bool,
    graph: &mut crate::MyGraph,
) -> sf::graph::Node<phys::RigidBody> {
    let pose_node = graph.l_pose.insert(block.pose.into(), &mut graph.graph);
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
    graph.graph.connect(&pose_node, &body_node);
    graph.graph.connect(&body_node, &coll_node);
    graph.graph.connect(&pose_node, &shape_node);

    sf::graph::NodeRef::as_node(&body_node, &graph.graph)
}

impl Recipe {
    pub fn spawn(&self, graph: &mut crate::MyGraph, physics: &mut phys::Physics) {
        match self {
            Recipe::Player(p_rec) => p_rec.spawn(graph),
            Recipe::StaticBlock(block) => {
                spawn_block(*block, [0.5; 4], true, graph);
            }
            Recipe::DynamicBlock(block) => {
                spawn_block(*block, random_color(), false, graph);
            }
            Recipe::Ball(Ball {
                radius,
                position,
                restitution,
                start_velocity,
            }) => {
                let pose_node = graph.l_pose.insert(
                    m::Pose::new(position.into(), m::Rotor2::identity()),
                    &mut graph.graph,
                );
                let coll = phys::Collider::new_circle(*radius);
                let body = phys::RigidBody::new_dynamic(&coll, 0.5)
                    .with_material(SurfaceMaterial {
                        restitution_coef: *restitution,
                        ..Default::default()
                    })
                    .with_velocity(Velocity {
                        linear: start_velocity.into(),
                        angular: 0.0,
                    });
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
                graph.graph.connect(&pose_node, &body_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&pose_node, &shape_node);
            }
            Recipe::Blockchain {
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

                let half_spacing = spacing / 2.0;

                let mut links_iter = links.iter().map(|p| m::Vec2::new(p[0], p[1])).peekable();

                // to connect another block to it
                let mut prev_block: Option<(sf::graph::Node<phys::RigidBody>, f64)> = None;
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
                            pose: m::IsometryBuilder::new()
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
                                .with_origin(m::Vec2::new(-block_length_half - half_spacing, 0.0))
                                .with_target_origin(m::Vec2::new(prev_block_offset, 0.0))
                                .with_compliance(0.015)
                                .build_attachment(),
                        );
                    } else if *anchored_start {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(block)
                                .with_origin(m::Vec2::new(-block_length_half - half_spacing, 0.0))
                                .with_target_origin(link1)
                                .build_attachment(),
                        );
                    }
                    prev_block = Some((block, block_length_half + half_spacing));
                }

                if *anchored_end {
                    let (prev_block, prev_block_offset) = prev_block.unwrap();
                    physics.add_constraint(
                        phys::ConstraintBuilder::new(prev_block)
                            .with_origin(m::Vec2::new(prev_block_offset + (spacing / 2.0), 0.0))
                            .with_target_origin(
                                links
                                    .iter()
                                    .map(|p| m::Vec2::new(p[0], p[1]))
                                    .last()
                                    .unwrap(),
                            )
                            .build_attachment(),
                    );
                }
            }
            Recipe::Oscillator {
                position,
                begin_length,
                target_length,
                compliance,
            } => {
                let position: m::Vec2 = position.into();
                let offset = m::Vec2::new(begin_length / 2.0, 0.0);
                let b1 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        pose: m::IsometryBuilder::new().with_position(position + offset),
                    },
                    random_color(),
                    false,
                    graph,
                );
                let b2 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        pose: m::IsometryBuilder::new().with_position(position - offset),
                    },
                    random_color(),
                    false,
                    graph,
                );
                physics.add_constraint(
                    phys::ConstraintBuilder::new(b1)
                        .with_target(b2)
                        .with_compliance(*compliance)
                        .build_distance(*target_length, phys::ConstraintLimit::Eq),
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
