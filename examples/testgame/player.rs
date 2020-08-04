use crate::MyGraph;
use starframe::{
    self as sf, graphics as gx,
    input::{Key, KeyAxisState},
    math as m, physics as phys,
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

        let tr_node = graph
            .l_transform
            .insert(self.transform.into(), &mut graph.graph);
        let shape_node = graph.l_shape.insert(
            gx::Shape::Rect {
                w: WIDTH,
                h: HEIGHT,
                color: [0.2, 0.8, 0.6, 1.0],
            },
            &mut graph.graph,
        );
        let coll = phys::Collider::new_rect(WIDTH, HEIGHT);
        let body = phys::RigidBody::new_dynamic(&coll, 3.0);
        let coll_node = graph.l_collider.insert(coll, &mut graph.graph);
        let body_node = graph.l_body.insert(body, &mut graph.graph);
        let tag_node = graph.l_player.insert(Player::new(), &mut graph.graph);
        graph.graph.connect(&tr_node, &body_node);
        graph.graph.connect(&body_node, &coll_node);
        graph.graph.connect(&tr_node, &shape_node);

        graph.graph.connect(&tag_node, &tr_node);
        graph.graph.connect(&tag_node, &body_node);
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

    pub fn tick(&mut self, g: &mut MyGraph, input: &sf::InputCache) {
        let (target_facing, target_hdir) = match input.get_key_axis_state(Key::Right, Key::Left) {
            KeyAxisState::Zero => (None, 0.0),
            KeyAxisState::Pos => (Some(Facing::Right), 1.0),
            KeyAxisState::Neg => (Some(Facing::Left), -1.0),
        };

        let mut bullet_queue: Vec<(m::Transform, phys::Velocity)> = Vec::new();
        for mut player in g.l_player.iter_mut(&g.graph) {
            let player_body = g.graph.get_neighbor_mut(&player, &mut g.l_body).unwrap();
            let mut player_tr = g
                .graph
                .get_neighbor_mut(&player, &mut g.l_transform)
                .unwrap();

            // move and orient

            if let Some(facing) = target_facing {
                player.item.facing = facing;
            }

            let move_speed = self.base_move_speed;

            let target_hvel = target_hdir * move_speed;
            let player_vel = match player_body.item.velocity_mut() {
                Some(vel) => vel,
                None => continue,
            };
            let accel_needed = target_hvel - player_vel.linear.x;
            let accel = accel_needed.min(self.max_acceleration);
            player_vel.linear.x += accel;

            // hacked up rotation locking

            player_vel.angular = 0.0;
            player_tr.item.isometry.rotation = na::UnitComplex::new(0.0);

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
                            player_tr.item.isometry.translation.vector
                                + player.item.facing.orient_vec(m::Vec2::new(0.2, 0.0)),
                        )
                        .build(),
                    phys::Velocity {
                        angular: 0.0,
                        linear: player.item.facing.orient_vec(m::Vec2::new(20.0, 0.1)),
                    },
                ));
            }
        }

        for (bullet_tr, bullet_vel) in bullet_queue {
            Self::spawn_bullet(bullet_tr, bullet_vel, g)
        }
    }

    fn spawn_bullet(tr: m::Transform, vel: phys::Velocity, g: &mut MyGraph) {
        const R: f32 = 0.05;
        let tr_node = g.l_transform.insert(tr, &mut g.graph);
        let shape_node = g.l_shape.insert(
            gx::Shape::Circle {
                r: R,
                points: 5,
                color: [1.0; 4],
            },
            &mut g.graph,
        );
        let coll = phys::Collider::new_circle(R);
        let body = phys::RigidBody::new_dynamic_const_mass(&coll, 1.0).with_velocity(vel);
        let coll_node = g.l_collider.insert(coll, &mut g.graph);
        let body_node = g.l_body.insert(body, &mut g.graph);

        let evt_sink_node = g.l_evt_sink.insert(
            sf::EventSink::new(|g: &mut MyGraph, node, evt| match evt {
                sf::Event::Contact(contact) => {
                    println!(
                        "Bullet hit with {}",
                        contact.info.impulse * *contact.info.normal
                    );
                    if let Some(checked) = node.check(&g.graph) {
                        g.graph.delete(checked);
                    }
                }
                _ => (),
            }),
            &mut g.graph,
        );

        g.graph.connect(&tr_node, &body_node);
        g.graph.connect(&body_node, &coll_node);
        g.graph.connect(&tr_node, &shape_node);
        g.graph.connect(&body_node, &evt_sink_node);
    }
}
