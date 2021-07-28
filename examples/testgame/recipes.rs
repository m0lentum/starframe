use starframe::{
    self as sf, graphics as gx, math as m,
    physics::{self as phys, Material, Velocity},
};

use rand::{distributions as distr, distributions::Distribution};

#[derive(Clone, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    Block(Block),
    Ball(Ball),
    Capsule {
        length: f64,
        radius: f64,
        pose: m::PoseBuilder,
    },
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
    RopeConnectedBlocks {
        block1: Block,
        offset1: [f64; 2],
        block2: Block,
        offset2: [f64; 2],
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
    pub pose: m::PoseBuilder,
    pub is_static: bool,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            pose: Default::default(),
            is_static: false,
        }
    }
}

fn spawn_block(
    block: Block,
    color: [f32; 4],
    g: &mut crate::MyGraph,
) -> Option<sf::graph::Node<phys::Body>> {
    let pose_node = g.l_pose.insert(block.pose.into(), &mut g.graph);
    let coll = phys::Collider::new_rect(block.width, block.height);
    let coll_node = g.l_collider.insert(coll, &mut g.graph);
    let shape_node = g.l_shape.insert(
        gx::Shape::Rect {
            w: block.width,
            h: block.height,
            color,
        },
        &mut g.graph,
    );
    g.graph.connect(&pose_node, &coll_node);
    g.graph.connect(&pose_node, &shape_node);

    if !block.is_static {
        let body = phys::Body::new_dynamic(&coll, 0.5);
        let body_node = g.l_body.insert(body, &mut g.graph);
        g.graph.connect(&body_node, &coll_node);
        g.graph.connect(&pose_node, &body_node);
        Some(sf::graph::NodeRef::as_node(&body_node, &g.graph))
    } else {
        None
    }
}

fn spawn_body(
    pose: m::Pose,
    coll: phys::Collider,
    color: [f32; 4],
    g: &mut crate::MyGraph,
) -> sf::graph::Node<phys::Body> {
    let pose_node = g.l_pose.insert(pose, &mut g.graph);
    let coll_node = g.l_collider.insert(coll, &mut g.graph);
    let shape_node = g
        .l_shape
        .insert(gx::Shape::from_collider(&coll, color), &mut g.graph);
    let body_node = g
        .l_body
        .insert(phys::Body::new_dynamic(&coll, 0.5), &mut g.graph);

    g.graph.connect(&pose_node, &coll_node);
    g.graph.connect(&pose_node, &shape_node);
    g.graph.connect(&body_node, &coll_node);
    g.graph.connect(&pose_node, &body_node);

    sf::graph::NodeRef::as_node(&body_node, &g.graph)
}

impl Recipe {
    pub fn spawn(&self, graph: &mut crate::MyGraph, physics: &mut phys::Physics) {
        match self {
            Recipe::Player(p_rec) => p_rec.spawn(graph),
            Recipe::Block(block) => {
                spawn_block(
                    *block,
                    if block.is_static {
                        [0.5; 4]
                    } else {
                        random_color()
                    },
                    graph,
                );
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
                let coll = phys::Collider::new_circle(*radius).with_material(Material {
                    restitution_coef: *restitution,
                    ..Default::default()
                });
                let body = phys::Body::new_dynamic(&coll, 0.5).with_velocity(Velocity {
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
                graph.graph.connect(&pose_node, &coll_node);
                graph.graph.connect(&body_node, &coll_node);
                graph.graph.connect(&pose_node, &shape_node);
            }
            Recipe::Capsule {
                length,
                radius,
                pose,
            } => {
                spawn_body(
                    (*pose).into(),
                    phys::Collider::new_capsule(*length, *radius),
                    random_color(),
                    graph,
                );
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
                let radius = width / 2.0;

                let mut links_iter = links.iter().map(|p| m::Vec2::new(p[0], p[1])).peekable();

                // to connect another block to it
                let mut prev_block: Option<(sf::graph::Node<phys::Body>, f64)> = None;
                while let (Some(link1), Some(link2)) = (links_iter.next(), links_iter.peek()) {
                    let distance = *link2 - link1;
                    let dist_norm = distance.mag();
                    let center = (link1 + *link2) / 2.0;
                    let orientation = (distance[0] / dist_norm).acos() * distance[1].signum();

                    let caps_full_length = dist_norm - spacing;
                    let capsule = spawn_body(
                        m::PoseBuilder::new()
                            .with_position(center)
                            .with_rotation(m::Angle::Rad(orientation))
                            .into(),
                        phys::Collider::new_capsule(caps_full_length - width, radius),
                        random_color(),
                        graph,
                    );
                    let caps_length_half = caps_full_length / 2.0;
                    if let Some((prev_block, prev_block_offset)) = prev_block {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(capsule)
                                .with_target(prev_block)
                                .with_origin(m::Vec2::new(-caps_length_half - half_spacing, 0.0))
                                .with_target_origin(m::Vec2::new(prev_block_offset, 0.0))
                                .with_compliance(0.015)
                                .build_attachment(),
                        );
                    } else if *anchored_start {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(capsule)
                                .with_origin(m::Vec2::new(-caps_length_half - half_spacing, 0.0))
                                .with_target_origin(link1)
                                .build_attachment(),
                        );
                    }
                    prev_block = Some((capsule, caps_length_half + half_spacing));
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
                        pose: m::PoseBuilder::new().with_position(position + offset),
                        is_static: false,
                    },
                    random_color(),
                    graph,
                )
                .unwrap();
                let b2 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        pose: m::PoseBuilder::new().with_position(position - offset),
                        is_static: false,
                    },
                    random_color(),
                    graph,
                )
                .unwrap();
                physics.add_constraint(
                    phys::ConstraintBuilder::new(b1)
                        .with_target(b2)
                        .with_compliance(*compliance)
                        .with_linear_damping(0.0)
                        .build_distance(*target_length),
                );
            }
            Recipe::RopeConnectedBlocks {
                block1,
                offset1,
                block2,
                offset2,
            } => {
                let b1 = spawn_block(*block1, random_color(), graph);
                let b2 = spawn_block(*block2, random_color(), graph);
                let rope_end_1 = block1.pose.build() * m::Vec2::from(offset1);
                let rope_end_2 = block2.pose.build() * m::Vec2::from(offset2);
                let rope = phys::spawn_rope_line(
                    phys::Rope {
                        spacing: 0.05,
                        thickness: 0.1,
                        compliance: 0.0001,
                        bending: Some(phys::RopeBendingResistance {
                            max_angle: m::Angle::Deg(45.0),
                            compliance: 0.1,
                        }),
                        damping: 10.0,
                        material: phys::Material {
                            static_friction_coef: 0.0,
                            dynamic_friction_coef: 0.0,
                            restitution_coef: 0.0,
                        },
                    },
                    rope_end_1,
                    rope_end_2,
                    0.02,
                    &mut graph.l_body,
                    &mut graph.l_pose,
                    &mut graph.l_collider,
                    &mut graph.l_rope,
                    &mut graph.l_shape,
                    &mut graph.graph,
                );
                match b1 {
                    Some(b1) => {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(rope.first_particle)
                                .with_target(b1)
                                .with_target_origin(offset1.into())
                                .build_attachment(),
                        );
                    }
                    None => {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(rope.first_particle)
                                .with_target_origin(rope_end_1)
                                .build_attachment(),
                        );
                    }
                }
                match b2 {
                    Some(b2) => {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(rope.last_particle)
                                .with_target(b2)
                                .with_target_origin(offset2.into())
                                .build_attachment(),
                        );
                    }
                    None => {
                        physics.add_constraint(
                            phys::ConstraintBuilder::new(rope.last_particle)
                                .with_target_origin(rope_end_2)
                                .build_attachment(),
                        );
                    }
                }
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
