//! Types and procedures for reading objects from files and spawning them in the game world.
//!
//! This file has gotten quite large and unwieldy over time.
//! TODO: streamline this and bring in the Tiled editor integration from Flamegrower

use itertools::Itertools;
use starframe as sf;

use rand::{distributions as distr, distributions::Distribution, Rng};

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
        start_time: f32,
    },
    BackgroundForest {
        mesh_count: usize,
        anim_count: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
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

// TODO: these huge argument lists are awful,
// consolidate more of this stuff into one place that's easy to share
fn spawn_block(
    graphics: &mut sf::GraphicsManager,
    physics: &mut sf::PhysicsWorld,
    world: &mut sf::hecs::World,
    block: Block,
) -> sf::hecs::Entity {
    let pose = sf::Pose::from(block.pose);
    let coll = sf::Collider::new_rounded_rect(block.width, block.height, block.radius);
    let coll_key = physics.entity_set.insert_collider(coll);
    let mesh = sf::MeshParams {
        data: sf::MeshData::from(coll),
        ..Default::default()
    }
    .upload(None);
    let mesh_id = graphics.insert_mesh(mesh, None);

    let entity = world.spawn((pose, coll_key, mesh_id));

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
    material: Option<String>,
}

fn spawn_static(
    graphics: &mut sf::GraphicsManager,
    physics: &mut sf::PhysicsWorld,
    world: &mut sf::hecs::World,
    solid: Solid,
) {
    for coll in solid.colliders {
        let coll_key = physics
            .entity_set
            .insert_collider(coll.with_pose(solid.pose));
        let mesh = sf::MeshParams {
            data: sf::MeshData::from(*coll),
            offset: solid.pose * coll.pose,
            ..Default::default()
        }
        .upload(None);
        let mesh_id = graphics.insert_mesh(mesh, None);
        if let Some(mat_id) = solid
            .material
            .as_ref()
            .and_then(|mat| graphics.get_material_id(mat))
        {
            graphics.set_mesh_material(mesh_id, mat_id);
        }
        world.spawn((coll_key, mesh_id));
    }
}

fn spawn_body(
    graphics: &mut sf::GraphicsManager,
    physics: &mut sf::PhysicsWorld,
    world: &mut sf::hecs::World,
    hecs_sync: &mut sf::HecsSyncManager,
    solid: Solid,
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
        let mesh = sf::MeshParams {
            data: sf::MeshData::from(coll),
            ..Default::default()
        }
        .upload(None);
        let mesh_id = graphics.insert_mesh(mesh, None);
        if let Some(mat_id) = solid
            .material
            .as_ref()
            .and_then(|mat| graphics.get_material_id(mat))
        {
            graphics.set_mesh_material(mesh_id, mat_id);
        }
        let ent = world.spawn((solid.pose, coll_key, mesh_id));
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
        graphics: &mut sf::GraphicsManager,
    ) {
        let mut rng = rand::thread_rng();
        let mut random_palette = || {
            Some(format!(
                "palette{}",
                rng.gen_range(0..super::PALETTE_COLORS.len())
            ))
        };
        match self {
            Recipe::Player(p_rec) => p_rec.spawn(physics, graphics, world),
            Recipe::Block(block) => {
                spawn_block(graphics, physics, world, *block);
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
                    material: random_palette(),
                };
                if *is_static {
                    spawn_static(graphics, physics, world, solid);
                } else {
                    let body_key = spawn_body(graphics, physics, world, hecs_sync, solid);
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
                    material: random_palette(),
                };
                if *is_static {
                    spawn_static(graphics, physics, world, solid);
                } else {
                    spawn_body(graphics, physics, world, hecs_sync, solid);
                }
            }
            Recipe::GenericBody { pose, colliders } => {
                let solid = Solid {
                    pose: *pose,
                    colliders,
                    material: random_palette(),
                };
                spawn_body(graphics, physics, world, hecs_sync, solid);
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
                        graphics,
                        physics,
                        world,
                        hecs_sync,
                        Solid {
                            pose: sf::PoseBuilder::new()
                                .with_position(center)
                                .with_rotation(sf::Angle::Rad(orientation))
                                .into(),
                            colliders: &mut [sf::Collider::new_capsule(
                                caps_full_length - width,
                                radius,
                            )],
                            material: random_palette(),
                        },
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
                    graphics,
                    physics,
                    world,
                    Block {
                        width: 1.0,
                        height: 1.0,
                        radius: 0.0,
                        pose: sf::PoseBuilder::new().with_position(position + offset),
                        is_static: false,
                    },
                );
                let b1 = *world.query_one_mut::<&sf::BodyKey>(b1).unwrap();
                let b2 = spawn_block(
                    graphics,
                    physics,
                    world,
                    Block {
                        width: 1.0,
                        height: 1.0,
                        radius: 0.0,
                        pose: sf::PoseBuilder::new().with_position(position - offset),
                        is_static: false,
                    },
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
                let b1 = spawn_block(graphics, physics, world, *block1);
                let b1 = world.query_one_mut::<&sf::BodyKey>(b1).copied();
                let b2 = spawn_block(graphics, physics, world, *block2);
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
                // temporary visualisation with individual particle Meshes
                let mesh = sf::MeshParams {
                    data: sf::MeshData::from(sf::ConvexMeshShape::Circle {
                        r: rope.params.thickness / 2.0,
                        points: 8,
                    }),
                    ..Default::default()
                }
                .upload(None);
                let mesh_id = graphics.insert_mesh(mesh, None);
                for particle in &rope.particles {
                    let mesh_ent = world.spawn((sf::Pose::default(), mesh_id, *particle));
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
            Recipe::BackgroundTree { pose, start_time } => {
                let mesh_id = graphics.get_mesh_id("library.tree_mesh").unwrap();
                let mesh_id = graphics.new_animation_target(mesh_id);
                let mut anim =
                    sf::Animator::new(graphics.get_animation_id("library.sway").unwrap())
                        .with_target(mesh_id);
                anim.t = *start_time;
                graphics.insert_animator(anim);
                world.spawn((*pose, mesh_id));
            }
            Recipe::BackgroundForest {
                mesh_count,
                anim_count,
                left,
                right,
                top,
                bottom,
            } => {
                // spawn a ton of trees with a few shared animation states
                let anim_id = graphics.get_animation_id("library.sway").unwrap();
                let mesh_id = graphics.get_mesh_id("library.tree_mesh").unwrap();

                let targets: Vec<sf::MeshId> = std::iter::once(mesh_id)
                    .chain(std::iter::repeat_with(|| {
                        graphics.new_animation_target(mesh_id)
                    }))
                    .take(*anim_count)
                    .collect_vec();

                let time_increment = 4. / targets.len() as f32;
                for (i, target) in targets.iter().enumerate() {
                    let mut animator = sf::Animator::new(anim_id).with_target(*target);
                    animator.t = time_increment * i as f32;
                    graphics.insert_animator(animator);
                }

                for mesh_idx in 0..*mesh_count {
                    let pose = sf::PoseBuilder::new()
                        .with_position([
                            distr::Uniform::from(*left..*right).sample(&mut rng) as f64,
                            distr::Uniform::from(*bottom..*top).sample(&mut rng) as f64,
                        ])
                        .build();
                    world.spawn((pose, targets[mesh_idx % targets.len()]));
                }
            }
        }
    }
}
