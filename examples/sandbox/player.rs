use starframe::{
    self as sf,
    graph::{named_layer_bundle, Graph},
    graphics as gx,
    input::{AxisQuery, Key},
    math as m, physics as phys,
};

const MAX_SIMULTANEOUS_BULLETS: usize = 5;

#[derive(Clone, Debug)]
pub struct Player {
    facing: Facing,
    active_bullets: Vec<sf::graph::NodeKey<phys::Collider>>,
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
    fn orient_vec(&self, vel: m::Vec2) -> m::Vec2 {
        match self {
            Facing::Right => vel,
            Facing::Left => m::Vec2::new(-vel.x, vel.y),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct PlayerRecipe {
    pub position: [f64; 2],
}

named_layer_bundle! {
    pub struct PlayerLayers<'a> {
        pose: w m::Pose,
        collider: w phys::Collider,
        body: w phys::Body,
        mesh: w gx::Mesh,
        player: w Player,
    }
}

impl PlayerRecipe {
    pub fn spawn(&self, mut l: PlayerLayers) {
        const R: f64 = 0.1;
        const LENGTH: f64 = 0.2;

        let coll = phys::Collider::new_capsule(LENGTH, R).with_material(phys::Material {
            static_friction_coef: None,
            dynamic_friction_coef: None,
            restitution_coef: 0.0,
        });

        let mut pose_node = l.pose.insert(
            m::PoseBuilder::new()
                .with_position(self.position)
                .with_rotation(m::Angle::Deg(90.0))
                .build(),
        );
        let mut shape_node = l
            .mesh
            .insert(gx::Mesh::from(coll).with_color([0.2, 0.8, 0.6, 1.0]));
        let mut coll_node = l.collider.insert(coll);
        let mut body_node = l.body.insert(phys::Body::new_particle(1.0));
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

    pub fn tick(&mut self, input: &sf::InputCache, physics: &phys::Physics, graph: &mut Graph) {
        let mut l = graph.get_layer_bundle::<PlayerLayers>();

        let move_axis = input.axis(AxisQuery {
            pos_btn: Key::Right.into(),
            neg_btn: Key::Left.into(),
        });
        let (target_facing, target_hdir) = if move_axis == 0.0 {
            (None, 0.0)
        } else if move_axis.is_sign_positive() {
            (Some(Facing::Right), 1.0)
        } else {
            (Some(Facing::Left), -1.0)
        };

        let mut bullet_delete_queue: Vec<sf::graph::NodeKey<phys::Collider>> = Vec::new();

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

            if input.button(Key::LShift.into()) {
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
                && input.button(Key::Z.into())
            {
                const R: f64 = 0.05;
                let player_pos = player_pose.c.translation;
                let mut b_pose = l.pose.insert(
                    m::PoseBuilder::new()
                        .with_position(
                            player_pos + player.c.facing.orient_vec(m::Vec2::new(0.2, 0.0)),
                        )
                        .build(),
                );
                let mut b_mesh = l
                    .mesh
                    .insert(gx::Mesh::from(gx::MeshShape::Circle { r: R, points: 5 }));
                let mut b_coll = l.collider.insert(phys::Collider::new_circle(R));
                let mut b_body = l.body.insert(
                    phys::Body::new_dynamic_const_mass(b_coll.c.info(), 1.0).with_velocity(
                        phys::Velocity {
                            angular: 0.0,
                            linear: player.c.facing.orient_vec(m::Vec2::new(20.0, 0.1)),
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
