use starframe::{
    self as sf,
    graph::LayerViewMut,
    graphics as gx, math as m,
    physics::{self as phys, rope, Material},
};

use rand::{distributions as distr, distributions::Distribution};

#[derive(Clone, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    Block(Block),
    Ball(Ball),
    Capsule(Capsule),
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
    pub is_static: bool,
}

impl Default for Ball {
    fn default() -> Self {
        Self {
            radius: 1.0,
            position: [0.0, 0.0],
            restitution: 0.0,
            start_velocity: [0.0, 0.0],
            is_static: false,
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Capsule {
    pub length: f64,
    pub radius: f64,
    pub pose: m::PoseBuilder,
    pub is_static: bool,
}

impl Default for Capsule {
    fn default() -> Self {
        Self {
            length: 1.0,
            radius: 0.5,
            pose: Default::default(),
            is_static: false,
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

type Layers<'a> = (
    LayerViewMut<'a, m::Pose>,
    LayerViewMut<'a, phys::Collider>,
    LayerViewMut<'a, phys::Body>,
    LayerViewMut<'a, gx::Shape>,
);

fn spawn_block(block: Block, layers: Layers) -> Option<sf::graph::NodeKey<phys::Body>> {
    let (mut l_pose, mut l_collider, mut l_body, mut l_shape) = layers;

    let mut pose_node = l_pose.insert(block.pose.into());
    let coll = phys::Collider::new_rect(block.width, block.height);
    let mut coll_node = l_collider.insert(coll);
    let mut shape_node = l_shape.insert(gx::Shape::Rect {
        w: block.width,
        h: block.height,
        color: if block.is_static {
            [0.7; 4]
        } else {
            random_color()
        },
    });
    pose_node.connect(&mut coll_node);
    pose_node.connect(&mut shape_node);

    if !block.is_static {
        let body = phys::Body::new_dynamic(&coll, 0.5);
        let mut body_node = l_body.insert(body);
        body_node.connect(&mut coll_node);
        pose_node.connect(&mut body_node);
        Some(body_node.key())
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug)]
struct Solid {
    pose: m::Pose,
    coll: phys::Collider,
    color: [f32; 4],
}

fn spawn_static(solid: Solid, layers: Layers) {
    let (mut l_pose, mut l_collider, _, mut l_shape) = layers;

    let mut pose_node = l_pose.insert(solid.pose);
    let mut coll_node = l_collider.insert(solid.coll);
    let mut shape_node = l_shape.insert(gx::Shape::from_collider(&solid.coll, solid.color));
    pose_node.connect(&mut coll_node);
    pose_node.connect(&mut shape_node);
}

fn spawn_body(solid: Solid, layers: Layers) -> sf::graph::NodeKey<phys::Body> {
    let (mut l_pose, mut l_collider, mut l_body, mut l_shape) = layers;

    let mut pose_node = l_pose.insert(solid.pose);
    let mut coll_node = l_collider.insert(solid.coll);
    let mut shape_node = l_shape.insert(gx::Shape::from_collider(&solid.coll, solid.color));
    let mut body_node = l_body.insert(phys::Body::new_dynamic(&solid.coll, 0.5));

    pose_node.connect(&mut coll_node);
    pose_node.connect(&mut shape_node);
    body_node.connect(&mut coll_node);
    pose_node.connect(&mut body_node);

    body_node.key()
}

impl Recipe {
    pub fn spawn(&self, physics: &mut phys::Physics, graph: &super::MyGraph) {
        match self {
            Recipe::Player(p_rec) => p_rec.spawn(graph.get_layer_bundle()),
            Recipe::Block(block) => {
                spawn_block(*block, graph.get_layer_bundle());
            }
            Recipe::Ball(Ball {
                radius,
                position,
                restitution,
                start_velocity,
                is_static,
            }) => {
                let pose = m::Pose::new(position.into(), m::Rotor2::identity());
                let coll = phys::Collider::new_circle(*radius).with_material(Material {
                    restitution_coef: *restitution,
                    ..Default::default()
                });
                let solid = Solid {
                    pose,
                    coll,
                    color: random_color(),
                };
                if *is_static {
                    spawn_static(solid, graph.get_layer_bundle());
                } else {
                    let body = spawn_body(solid, graph.get_layer_bundle());
                    graph
                        .get_layer_mut::<phys::Body>()
                        .get_mut(body)
                        .unwrap()
                        .c
                        .velocity
                        .linear = start_velocity.into();
                }
            }
            Recipe::Capsule(Capsule {
                length,
                radius,
                pose,
                is_static,
            }) => {
                let solid = Solid {
                    pose: (*pose).into(),
                    coll: phys::Collider::new_capsule(*length, *radius),
                    color: random_color(),
                };
                if *is_static {
                    spawn_static(solid, graph.get_layer_bundle());
                } else {
                    spawn_body(solid, graph.get_layer_bundle());
                }
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
                let mut prev_block: Option<(sf::graph::NodeKey<phys::Body>, f64)> = None;
                while let (Some(link1), Some(link2)) = (links_iter.next(), links_iter.peek()) {
                    let distance = *link2 - link1;
                    let dist_norm = distance.mag();
                    let center = (link1 + *link2) / 2.0;
                    let orientation = (distance[0] / dist_norm).acos() * distance[1].signum();

                    let caps_full_length = dist_norm - spacing;
                    let capsule = spawn_body(
                        Solid {
                            pose: m::PoseBuilder::new()
                                .with_position(center)
                                .with_rotation(m::Angle::Rad(orientation))
                                .into(),
                            coll: phys::Collider::new_capsule(caps_full_length - width, radius),
                            color: random_color(),
                        },
                        graph.get_layer_bundle(),
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
                    graph.get_layer_bundle(),
                )
                .unwrap();
                let b2 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        pose: m::PoseBuilder::new().with_position(position - offset),
                        is_static: false,
                    },
                    graph.get_layer_bundle(),
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
                let b1 = spawn_block(*block1, graph.get_layer_bundle());
                let b2 = spawn_block(*block2, graph.get_layer_bundle());
                let rope_end_1 = block1.pose.build() * m::Vec2::from(offset1);
                let rope_end_2 = block2.pose.build() * m::Vec2::from(offset2);
                let rope = rope::spawn_line(
                    rope::Rope {
                        ..Default::default()
                    },
                    rope_end_1,
                    rope_end_2,
                    graph.get_layer_bundle(),
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
