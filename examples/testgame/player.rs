use crate::MyGraph;
use starframe::{
    core::{
        self, graph,
        inputcache::{Key, KeyAxisState},
        math as m,
    },
    graphics as gx, physics as phys,
};

use nalgebra as na;

pub struct Tag;

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
        let tag_node = graph.l_playertag.push(Tag);
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

    pub fn tick(
        &mut self,
        graph: &graph::Graph,
        l_transform: &mut graph::Layer<m::Transform>,
        l_body: &mut graph::Layer<phys::RigidBody>,
        l_tag: &graph::Layer<Tag>,
        input: &core::InputCache,
    ) {
        let target_hdir = match input.get_key_axis_state(Key::Right, Key::Left) {
            KeyAxisState::Zero => 0.0,
            KeyAxisState::Pos => 1.0,
            KeyAxisState::Neg => -1.0,
        };

        for tag in l_tag.iter() {
            let mut player_body = graph.get_neighbor_mut(&tag, l_body).unwrap();
            let mut player_tr = graph.get_neighbor_mut(&tag, l_transform).unwrap();

            // move

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
                player_vel.linear.y = 8.0;
            }
        }
    }
}
