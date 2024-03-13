//! Types and procedures for reading objects from files and spawning them in the game world.
//!
//! This file has gotten quite large and unwieldy over time.
//! TODO: streamline this and bring in the Tiled editor integration from Flamegrower

use starframe as sf;

use rand::{distributions as distr, distributions::Distribution};

#[derive(Clone, Debug, serde::Deserialize)]
pub enum Recipe {
    Player(crate::player::PlayerRecipe),
    Block(Block),
    Ball(Ball),
    Capsule(Capsule),
    GenericBody {
        #[serde(with = "sf::serde_pose")]
        pose: sf::Pose,
        colliders: Vec<sf::Collider>,
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
    BackgroundTree {
        #[serde(with = "sf::serde_pose")]
        pose: sf::Pose,
        depth: f32,
        start_time: f32,
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
    pub pose: sf::PoseBuilder,
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
    pub radius: f64,
    pub pose: sf::PoseBuilder,
    pub is_static: bool,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            radius: 0.0,
            pose: Default::default(),
            is_static: false,
        }
    }
}

fn spawn_block(
    block: Block,
    physics: &mut sf::PhysicsWorld,
    world: &mut sf::hecs::World,
) -> sf::hecs::Entity {
    let pose = sf::Pose::from(block.pose);
    let coll = sf::Collider::new_rounded_rect(block.width, block.height, block.radius);
    let coll_key = physics.entity_set.insert_collider(coll);
    let mesh = sf::Mesh::from(coll);

    let entity = world.spawn((pose, coll_key, mesh));

    if !block.is_static {
        let body = sf::Body::new_dynamic(coll.info(), 0.5);
        let body_key = physics.entity_set.insert_body(body);
        physics
            .entity_set
            .attach_existing_collider(body_key, coll_key);
        world.insert_one(entity, body_key).ok();
    }
    entity
}

#[derive(Debug)]
struct Solid<'a> {
    pose: sf::Pose,
    colliders: &'a [sf::Collider],
    color: [f32; 3],
}

fn spawn_static(solid: Solid, physics: &mut sf::PhysicsWorld, world: &mut sf::hecs::World) {
    for coll in solid.colliders {
        let coll_key = physics
            .entity_set
            .insert_collider(coll.with_pose(solid.pose));
        let mesh = sf::Mesh::from(*coll)
            .with_offset(solid.pose * coll.pose)
            .with_tint(solid.color);
        world.spawn((coll_key, mesh));
    }
}

fn spawn_body(
    solid: Solid,
    physics: &mut sf::PhysicsWorld,
    world: &mut sf::hecs::World,
    hecs_sync: &mut sf::HecsSyncManager,
) -> sf::BodyKey {
    let coll_setup = sf::CompoundColliderSetup::new(solid.colliders);
    let center_of_mass = coll_setup.center_of_mass();

    let body = sf::Body::new_dynamic(coll_setup.info_around_point(center_of_mass), 0.5)
        .with_pose(solid.pose);
    let body_key = physics.entity_set.insert_body(body);

    for mut coll in solid.colliders.iter().cloned() {
        coll.pose.translation -= center_of_mass;
        let coll_key = physics.entity_set.attach_collider(body_key, coll);

        // visualization with a mesh entity synced from physics
        let mesh = sf::Mesh::from(coll)
            // undo the effect of the collider offset,
            // since hecs_sync gets its global pose
            .with_offset(sf::Pose::identity())
            .with_tint(solid.color);
        let ent = world.spawn((solid.pose, coll_key, mesh));
        hecs_sync.register_collider(coll_key, ent, sf::HecsSyncOptions::physics_to_hecs_only());
    }

    body_key
}

impl Recipe {
    pub fn spawn(
        &self,
        physics: &mut sf::PhysicsWorld,
        world: &mut sf::hecs::World,
        hecs_sync: &mut sf::HecsSyncManager,
        renderer: &sf::Renderer,
    ) {
        match self {
            Recipe::Player(p_rec) => p_rec.spawn(physics, world),
            Recipe::Block(block) => {
                spawn_block(*block, physics, world);
            }
            Recipe::Ball(Ball {
                radius,
                position,
                restitution,
                start_velocity,
                is_static,
            }) => {
                let pose = sf::Pose::new(position.into(), sf::Rotor2::identity());
                let coll = sf::Collider::new_circle(*radius).with_material(sf::PhysicsMaterial {
                    restitution_coef: *restitution,
                    ..Default::default()
                });
                let solid = Solid {
                    pose,
                    colliders: &mut [coll],
                    color: random_color(),
                };
                if *is_static {
                    spawn_static(solid, physics, world);
                } else {
                    let body_key = spawn_body(solid, physics, world, hecs_sync);
                    let body = physics.entity_set.get_body_mut(body_key).unwrap();
                    body.velocity.linear = start_velocity.into();
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
                    colliders: &mut [sf::Collider::new_capsule(*length, *radius)],
                    color: random_color(),
                };
                if *is_static {
                    spawn_static(solid, physics, world);
                } else {
                    spawn_body(solid, physics, world, hecs_sync);
                }
            }
            Recipe::GenericBody { pose, colliders } => {
                let solid = Solid {
                    pose: *pose,
                    colliders,
                    color: random_color(),
                };
                spawn_body(solid, physics, world, hecs_sync);
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

                let mut links_iter = links.iter().map(|p| sf::Vec2::new(p[0], p[1])).peekable();

                // to connect another block to it
                let mut prev_block: Option<(sf::BodyKey, f64)> = None;
                while let (Some(link1), Some(link2)) = (links_iter.next(), links_iter.peek()) {
                    let distance = *link2 - link1;
                    let dist_norm = distance.mag();
                    let center = (link1 + *link2) / 2.0;
                    let orientation = (distance[0] / dist_norm).acos() * distance[1].signum();

                    let caps_full_length = dist_norm - spacing;
                    let capsule = spawn_body(
                        Solid {
                            pose: sf::PoseBuilder::new()
                                .with_position(center)
                                .with_rotation(sf::Angle::Rad(orientation))
                                .into(),
                            colliders: &mut [sf::Collider::new_capsule(
                                caps_full_length - width,
                                radius,
                            )],
                            color: random_color(),
                        },
                        physics,
                        world,
                        hecs_sync,
                    );
                    let caps_length_half = caps_full_length / 2.0;
                    if let Some((prev_block, prev_block_offset)) = prev_block {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(capsule)
                                .with_target(prev_block)
                                .with_origin(sf::Vec2::new(-caps_length_half - half_spacing, 0.0))
                                .with_target_origin(sf::Vec2::new(prev_block_offset, 0.0))
                                .with_compliance(0.015)
                                .build_attachment(),
                        );
                    } else if *anchored_start {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(capsule)
                                .with_origin(sf::Vec2::new(-caps_length_half - half_spacing, 0.0))
                                .with_target_origin(link1)
                                .build_attachment(),
                        );
                    }
                    prev_block = Some((capsule, caps_length_half + half_spacing));
                }

                if *anchored_end {
                    let (prev_block, prev_block_offset) = prev_block.unwrap();
                    physics.constraint_set.insert(
                        sf::ConstraintBuilder::new(prev_block)
                            .with_origin(sf::Vec2::new(prev_block_offset + (spacing / 2.0), 0.0))
                            .with_target_origin(
                                links
                                    .iter()
                                    .map(|p| sf::Vec2::new(p[0], p[1]))
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
                let position: sf::Vec2 = position.into();
                let offset = sf::Vec2::new(begin_length / 2.0, 0.0);
                let b1 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        radius: 0.0,
                        pose: sf::PoseBuilder::new().with_position(position + offset),
                        is_static: false,
                    },
                    physics,
                    world,
                );
                let b1 = *world.query_one_mut::<&sf::BodyKey>(b1).unwrap();
                let b2 = spawn_block(
                    Block {
                        width: 1.0,
                        height: 1.0,
                        radius: 0.0,
                        pose: sf::PoseBuilder::new().with_position(position - offset),
                        is_static: false,
                    },
                    physics,
                    world,
                );
                let b2 = *world.query_one_mut::<&sf::BodyKey>(b2).unwrap();
                physics.constraint_set.insert(
                    sf::ConstraintBuilder::new(b1)
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
                let b1 = spawn_block(*block1, physics, world);
                let b1 = world.query_one_mut::<&sf::BodyKey>(b1).copied();
                let b2 = spawn_block(*block2, physics, world);
                let b2 = world.query_one_mut::<&sf::BodyKey>(b2).copied();
                let rope_end_1 = block1.pose.build() * sf::Vec2::from(offset1);
                let rope_end_2 = block2.pose.build() * sf::Vec2::from(offset2);
                let rope = sf::Rope::spawn_line(
                    sf::RopeParameters {
                        ..Default::default()
                    },
                    rope_end_1,
                    rope_end_2,
                    &mut physics.entity_set,
                );
                for particle in &rope.particles {
                    // temporary visualisation with individual particle Meshes
                    let mesh = sf::Mesh::from(sf::ConvexMeshShape::Circle {
                        r: rope.params.thickness / 2.0,
                        points: 8,
                    });
                    let mesh_ent = world.spawn((sf::Pose::default(), mesh, *particle));
                    hecs_sync.register_body(
                        particle.body,
                        mesh_ent,
                        sf::HecsSyncOptions::physics_to_hecs_only(),
                    );
                }
                let first_particle = *rope.particles.first().expect("No particles in rope");
                let last_particle = *rope.particles.iter().last().expect("No particles in rope");
                match b1 {
                    Ok(b1) => {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(first_particle.body)
                                .with_target(b1)
                                .with_target_origin(offset1.into())
                                .build_attachment(),
                        );
                    }
                    Err(_) => {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(first_particle.body)
                                .with_target_origin(rope_end_1)
                                .build_attachment(),
                        );
                    }
                }
                match b2 {
                    Ok(b2) => {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(last_particle.body)
                                .with_target(b2)
                                .with_target_origin(offset2.into())
                                .build_attachment(),
                        );
                    }
                    Err(_) => {
                        physics.constraint_set.insert(
                            sf::ConstraintBuilder::new(last_particle.body)
                                .with_target_origin(rope_end_2)
                                .build_attachment(),
                        );
                    }
                }
                physics.rope_set.insert(rope);
            }
            Recipe::BackgroundTree {
                pose,
                depth,
                start_time,
            } => {
                //
            }
        }
    }
}

fn random_color() -> [f32; 3] {
    let mut rng = rand::thread_rng();
    [
        distr::Uniform::from(0.6..1.0).sample(&mut rng),
        distr::Uniform::from(0.6..1.0).sample(&mut rng),
        distr::Uniform::from(0.6..1.0).sample(&mut rng),
    ]
}
