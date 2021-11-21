use starframe::{
    self as sf,
    graph::{LayerViewMut, NodeKey},
    graphics as gx,
    input::{Key, KeyAxisState},
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

type Layers<'a> = (
    LayerViewMut<'a, m::Pose>,
    LayerViewMut<'a, phys::Collider>,
    LayerViewMut<'a, phys::Body>,
    LayerViewMut<'a, gx::Shape>,
    LayerViewMut<'a, Player>,
);

impl PlayerRecipe {
    pub fn spawn(&self, layers: Layers) {
        let (mut l_pose, mut l_collider, mut l_body, mut l_shape, mut l_player) = layers;
        const R: f64 = 0.1;
        const LENGTH: f64 = 0.2;

        let coll = phys::Collider::new_capsule(LENGTH, R).with_material(phys::Material {
            static_friction_coef: None,
            dynamic_friction_coef: None,
            restitution_coef: 0.0,
        });

        let mut pose_node = l_pose.insert(
            m::PoseBuilder::new()
                .with_position(self.position)
                .with_rotation(m::Angle::Deg(90.0))
                .build(),
        );
        let mut shape_node = l_shape.insert(gx::Shape::from_collider(&coll, [0.2, 0.8, 0.6, 1.0]));
        let mut coll_node = l_collider.insert(coll);
        let mut body_node = l_body.insert(phys::Body::new_particle(1.0));
        let mut tag_node = l_player.insert(Player::new());
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

    pub fn tick(
        &mut self,
        input: &sf::InputCache,
        physics: &phys::Physics,
        graph: &mut super::MyGraph,
    ) {
        let mut layers = graph.get_layer_bundle::<Layers>();
        let (l_pose, l_collider, l_body, l_shape, l_player) = &mut layers;

        let (target_facing, target_hdir) = match input.get_key_axis_state(Key::Right, Key::Left) {
            KeyAxisState::Zero => (None, 0.0),
            KeyAxisState::Pos => (Some(Facing::Right), 1.0),
            KeyAxisState::Neg => (Some(Facing::Left), -1.0),
        };

        let mut bullet_delete_queue: Vec<sf::graph::NodeKey<phys::Collider>> = Vec::new();

        for mut player in l_player.iter_mut() {
            let mut player_body = player.get_neighbor_mut(l_body).unwrap();
            let player_tr = player.get_neighbor_mut(l_pose).unwrap();

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

            if input.is_key_pressed(Key::LShift, Some(0)) {
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
                && input.is_key_pressed(Key::Z, Some(0))
            {
                player.c.active_bullets.push(Self::spawn_bullet(
                    m::PoseBuilder::new()
                        .with_position(
                            player_tr.c.translation
                                + player.c.facing.orient_vec(m::Vec2::new(0.2, 0.0)),
                        )
                        .build(),
                    phys::Velocity {
                        angular: 0.0,
                        linear: player.c.facing.orient_vec(m::Vec2::new(20.0, 0.1)),
                    },
                    (l_pose, l_collider, l_body, l_shape),
                ));
            }
        }

        drop(layers);
        for bullet in bullet_delete_queue {
            graph.delete(bullet);
        }
    }

    fn spawn_bullet(
        pose: m::Pose,
        vel: phys::Velocity,
        (l_pose, l_collider, l_body, l_shape): (
            &mut LayerViewMut<m::Pose>,
            &mut LayerViewMut<phys::Collider>,
            &mut LayerViewMut<phys::Body>,
            &mut LayerViewMut<gx::Shape>,
        ),
    ) -> NodeKey<phys::Collider> {
        const R: f64 = 0.05;
        let mut pose_node = l_pose.insert(pose);
        let mut shape_node = l_shape.insert(gx::Shape::Circle {
            r: R,
            points: 5,
            color: [1.0; 4],
        });
        let coll = phys::Collider::new_circle(R);
        let body = phys::Body::new_dynamic_const_mass(&coll, 1.0).with_velocity(vel);
        let mut coll_node = l_collider.insert(coll);
        let mut body_node = l_body.insert(body);

        pose_node.connect(&mut body_node);
        pose_node.connect(&mut coll_node);
        body_node.connect(&mut coll_node);
        pose_node.connect(&mut shape_node);

        coll_node.key()
    }
}
