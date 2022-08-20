use starframe as sf;

const MAX_SIMULTANEOUS_BULLETS: usize = 5;

#[derive(Clone, Debug)]
pub struct Player {
    facing: Facing,
    active_bullets: Vec<sf::NodeKey<sf::Collider>>,
}
impl Player {
    fn new() -> Self {
        Player {
            facing: Facing::Left,
            active_bullets: Vec::with_capacity(MAX_SIMULTANEOUS_BULLETS),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(self) enum Facing {
    Right,
    Left,
}
impl Facing {
    fn orient_vec(&self, vel: sf::Vec2) -> sf::Vec2 {
        match self {
            Facing::Right => vel,
            Facing::Left => sf::Vec2::new(-vel.x, vel.y),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct PlayerRecipe {
    pub position: [f64; 2],
}

sf::named_layer_bundle! {
    pub struct PlayerLayers<'a> {
        pose: w sf::Pose,
        collider: w sf::Collider,
        body: w sf::Body,
        mesh: w sf::Mesh,
        player: w Player,
    }
}

impl PlayerRecipe {
    pub fn spawn(&self, mut l: PlayerLayers) {
        const R: f64 = 0.1;
        const LENGTH: f64 = 0.2;

        let coll = sf::Collider::new_capsule(LENGTH, R).with_material(sf::PhysicsMaterial {
            static_friction_coef: None,
            dynamic_friction_coef: None,
            restitution_coef: 0.0,
        });

        let mut pose_node = l.pose.insert(
            sf::PoseBuilder::new()
                .with_position(self.position)
                .with_rotation(sf::Angle::Deg(90.0))
                .build(),
        );
        let mut shape_node = l
            .mesh
            .insert(sf::Mesh::from(coll).with_color([0.2, 0.8, 0.6, 1.0]));
        let mut coll_node = l.collider.insert(coll);
        let mut body_node = l.body.insert(sf::Body::new_particle(1.0));
        let mut tag_node = l.player.insert(Player::new());
        pose_node.connect(&mut body_node);
        pose_node.connect(&mut coll_node);
        body_node.connect(&mut coll_node);
        pose_node.connect(&mut shape_node);

        tag_node.connect(&mut pose_node);
        tag_node.connect(&mut body_node);
    }
}

pub struct PlayerController {
    base_move_speed: f64,
    max_acceleration: f64,
}
impl PlayerController {
    pub fn new() -> Self {
        PlayerController {
            base_move_speed: 4.0,
            max_acceleration: 8.0,
        }
    }

    pub fn tick(&mut self, input: &sf::Input, physics: &sf::Physics, graph: &mut sf::Graph) {
        let mut l = graph.get_layer_bundle::<PlayerLayers>();

        let move_axis = input.axis(sf::AxisQuery {
            pos_btn: sf::Key::Right.into(),
            neg_btn: sf::Key::Left.into(),
        });
        let (target_facing, target_hdir) = if move_axis == 0.0 {
            (None, 0.0)
        } else if move_axis.is_sign_positive() {
            (Some(Facing::Right), 1.0)
        } else {
            (Some(Facing::Left), -1.0)
        };

        let mut bullet_delete_queue: Vec<sf::graph::NodeKey<sf::Collider>> = Vec::new();

        for mut player in l.player.iter_mut() {
            let mut player_body = player.get_neighbor_mut(&mut l.body).unwrap();
            let player_pose = player.get_neighbor_mut(&mut l.pose).unwrap();

            // move and orient

            if let Some(facing) = target_facing {
                player.c.facing = facing;
            }

            let move_speed = self.base_move_speed;

            let target_hvel = target_hdir * move_speed;
            let accel_needed = target_hvel - player_body.c.velocity.linear.x;
            let accel = accel_needed.min(self.max_acceleration);
            player_body.c.velocity.linear.x += accel;

            // jump

            if input.button(sf::Key::LShift.into()) {
                // TODO: only on ground, double jump, custom curve
                player_body.c.velocity.linear.y = 4.0;
            }

            // delete bullets that collided with something

            player.c.active_bullets.retain(|&bullet| {
                if physics.contacts_for_collider(bullet).next().is_none() {
                    true
                } else {
                    bullet_delete_queue.push(bullet);
                    false
                }
            });

            // shoot

            if player.c.active_bullets.len() < MAX_SIMULTANEOUS_BULLETS
                && input.button(sf::Key::Z.into())
            {
                const R: f64 = 0.05;
                let player_pos = player_pose.c.translation;
                let mut b_pose = l.pose.insert(
                    sf::PoseBuilder::new()
                        .with_position(
                            player_pos + player.c.facing.orient_vec(sf::Vec2::new(0.2, 0.0)),
                        )
                        .build(),
                );
                let mut b_mesh = l.mesh.insert(
                    sf::Mesh::from(sf::ConvexMeshShape::Circle { r: R, points: 5 })
                        .without_outline(),
                );
                let mut b_coll = l.collider.insert(sf::Collider::new_circle(R));
                let mut b_body = l.body.insert(
                    sf::Body::new_dynamic_const_mass(b_coll.c.info(), 1.0).with_velocity(
                        sf::Velocity {
                            angular: 0.0,
                            linear: player.c.facing.orient_vec(sf::Vec2::new(20.0, 0.1)),
                        },
                    ),
                );

                b_pose.connect(&mut b_body);
                b_pose.connect(&mut b_coll);
                b_body.connect(&mut b_coll);
                b_pose.connect(&mut b_mesh);

                player.c.active_bullets.push(b_coll.key());
            }
        }

        drop(l);
        for bullet in bullet_delete_queue {
            graph.gather(bullet).delete();
        }
    }
}
