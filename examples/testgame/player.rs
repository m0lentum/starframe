use crate::MyGraph;
use starframe::{
    core::{
        self,
        inputcache::{Key, KeyAxisState},
        math as m,
    },
    graphics as gx, physics as phys,
};

use nalgebra as na;

#[derive(Clone, Copy, Debug)]
pub struct Player {
    facing: Facing,
}
impl Player {
    fn new() -> Self {
        Player {
            facing: Facing::Left,
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
    pub transform: m::TransformBuilder,
}

impl PlayerRecipe {
    pub fn spawn(&self, graph: &mut MyGraph) {
        const WIDTH: f32 = 0.2;
        const HEIGHT: f32 = 0.4;

        let tr_node = graph.l_transform.push(self.transform.into());
        let shape_node = graph.l_shape.push(gx::Shape::Rect {
            w: WIDTH,
            h: HEIGHT,
            color: [0.2, 0.8, 0.6, 1.0],
        });
        let coll = phys::Collider::new_rect(WIDTH, HEIGHT);
        let body = phys::RigidBody::new_dynamic(&coll, 3.0);
        let coll_node = graph.l_collider.push(coll);
        let body_node = graph.l_body.push(body);
        let tag_node = graph.l_player.push(Player::new());
        graph.graph.connect(tr_node, body_node);
        graph.graph.connect(body_node, coll_node);
        graph.graph.connect(tr_node, shape_node);

        graph.graph.connect(tag_node, tr_node);
        graph.graph.connect(tag_node, body_node);
    }
}

pub struct PlayerController {
    base_move_speed: f32,
    max_acceleration: f32,
}
impl PlayerController {
    pub fn new() -> Self {
        PlayerController {
            base_move_speed: 4.0,
            max_acceleration: 8.0,
        }
    }

    pub fn tick(&mut self, g: &mut MyGraph, input: &core::InputCache) {
        let (target_facing, target_hdir) = match input.get_key_axis_state(Key::Right, Key::Left) {
            KeyAxisState::Zero => (None, 0.0),
            KeyAxisState::Pos => (Some(Facing::Right), 1.0),
            KeyAxisState::Neg => (Some(Facing::Left), -1.0),
        };

        let mut bullet_queue: Vec<(m::Transform, phys::Velocity)> = Vec::new();
        for mut player in g.l_player.iter_mut() {
            let mut player_body = g.graph.get_neighbor_mut(&player, &mut g.l_body).unwrap();
            let mut player_tr = g
                .graph
                .get_neighbor_mut(&player, &mut g.l_transform)
                .unwrap();

            // move and orient

            if let Some(facing) = target_facing {
                player.facing = facing;
            }

            let move_speed = self.base_move_speed;

            let target_hvel = target_hdir * move_speed;
            let player_vel = match player_body.velocity_mut() {
                Some(vel) => vel,
                None => continue,
            };
            let accel_needed = target_hvel - player_vel.linear.x;
            let accel = accel_needed.min(self.max_acceleration);
            player_vel.linear.x += accel;

            // hacked up rotation locking

            player_vel.angular = 0.0;
            player_tr.isometry.rotation = na::UnitComplex::new(0.0);

            // jump

            if input.is_key_pressed(Key::LShift, Some(0)) {
                // TODO: only on ground, double jump, custom curve
                player_vel.linear.y = 4.0;
            }

            // shoot

            if input.is_key_pressed(Key::Z, Some(0)) {
                bullet_queue.push((
                    m::TransformBuilder::new()
                        .with_position(
                            player_tr.isometry.translation.vector
                                + player.facing.orient_vec(m::Vec2::new(0.15, 0.0)),
                        )
                        .build(),
                    phys::Velocity {
                        angular: 0.0,
                        linear: player.facing.orient_vec(m::Vec2::new(15.0, 0.0)),
                    },
                ));
            }
        }

        for (bullet_tr, bullet_vel) in bullet_queue {
            Self::spawn_bullet(bullet_tr, bullet_vel, g)
        }
    }

    fn spawn_bullet(tr: m::Transform, vel: phys::Velocity, graph: &mut MyGraph) {
        const R: f32 = 0.05;
        let tr_node = graph.l_transform.push(tr);
        let shape_node = graph.l_shape.push(gx::Shape::Circle {
            r: R,
            points: 5,
            color: [1.0; 4],
        });
        let coll = phys::Collider::new_circle(R);
        let body = phys::RigidBody::new_dynamic_const_mass(&coll, 1.0).with_velocity(vel);
        let coll_node = graph.l_collider.push(coll);
        let body_node = graph.l_body.push(body);

        graph.graph.connect(tr_node, body_node);
        graph.graph.connect(body_node, coll_node);
        graph.graph.connect(tr_node, shape_node);
    }
}
