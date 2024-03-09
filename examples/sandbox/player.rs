use starframe as sf;

const MAX_SIMULTANEOUS_BULLETS: usize = 5;

#[derive(Clone, Debug)]
pub struct PlayerState {
    facing: Facing,
    active_bullets: Vec<(hecs::Entity, sf::ColliderKey)>,
}
impl PlayerState {
    fn new() -> Self {
        PlayerState {
            facing: Facing::Left,
            active_bullets: Vec::with_capacity(MAX_SIMULTANEOUS_BULLETS),
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Facing {
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

impl PlayerRecipe {
    pub fn spawn(&self, physics: &mut sf::PhysicsWorld, world: &mut sf::hecs::World) {
        const R: f64 = 0.1;
        const LENGTH: f64 = 0.2;

        let body = sf::Body::new_particle(1.0);
        let body_key = physics.entity_set.insert_body(body);
        let coll = sf::Collider::new_capsule(LENGTH, R).with_material(sf::PhysicsMaterial {
            static_friction_coef: None,
            dynamic_friction_coef: None,
            restitution_coef: 0.0,
        });
        let coll_key = physics.entity_set.attach_collider(body_key, coll);
        let pose = sf::PoseBuilder::new()
            .with_position(self.position)
            .with_rotation(sf::Angle::Deg(90.0))
            .build();
        let mesh = sf::Mesh::from(coll);
        let state = PlayerState::new();
        world.spawn((body_key, coll_key, pose, mesh, state));
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
        input: &sf::Input,
        physics: &mut sf::PhysicsWorld,
        world: &mut sf::hecs::World,
    ) {
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

        let mut bullet_delete_queue: Vec<hecs::Entity> = Vec::new();
        let mut bullet_to_spawn = None;

        for (player_entity, (body_key, pose, state)) in
            world.query_mut::<(&sf::BodyKey, &sf::Pose, &mut PlayerState)>()
        {
            let body = physics
                .entity_set
                .get_body_mut(*body_key)
                .expect("Player body was unexpectedly deleted");
            // move and orient

            if let Some(facing) = target_facing {
                state.facing = facing;
            }

            let move_speed = self.base_move_speed;

            let target_hvel = target_hdir * move_speed;
            let accel_needed = target_hvel - body.velocity.linear.x;
            let accel = accel_needed.min(self.max_acceleration);
            body.velocity.linear.x += accel;

            // jump

            if input.button(sf::Key::LShift.into()) {
                // TODO: only on ground, double jump, custom curve
                body.velocity.linear.y = 4.0;
            }

            // delete bullets that collided with something

            state.active_bullets.retain(|(entity, coll_key)| {
                if physics.contacts_for_collider(*coll_key).next().is_none() {
                    true
                } else {
                    bullet_delete_queue.push(*entity);
                    false
                }
            });

            // shoot

            if state.active_bullets.len() < MAX_SIMULTANEOUS_BULLETS
                && input.button(sf::Key::Z.into())
            {
                const R: f64 = 0.05;
                let player_pos = pose.translation;
                let b_pose = sf::PoseBuilder::new()
                    .with_position(player_pos + state.facing.orient_vec(sf::Vec2::new(0.2, 0.0)))
                    .build();
                let b_mesh = sf::Mesh::from(sf::ConvexMeshShape::Circle { r: R, points: 5 })
                    .without_outline();
                let b_coll = sf::Collider::new_circle(R);
                let b_body = sf::Body::new_dynamic_const_mass(b_coll.info(), 1.0).with_velocity(
                    sf::Velocity {
                        angular: 0.0,
                        linear: state.facing.orient_vec(sf::Vec2::new(20.0, 0.1)),
                    },
                );
                let b_body_key = physics.entity_set.insert_body(b_body);
                let b_coll_key = physics.entity_set.attach_collider(b_body_key, b_coll);

                bullet_to_spawn = Some((player_entity, (b_pose, b_mesh, b_body_key, b_coll_key)));
            }
        }

        for bullet in bullet_delete_queue {
            world.despawn(bullet).ok();
        }
        if let Some((player_ent, bullet)) = bullet_to_spawn {
            let bullet_ent = world.spawn(bullet);
            let bullet_coll = *world.query_one_mut::<&sf::ColliderKey>(bullet_ent).unwrap();
            let player_state = world.query_one_mut::<&mut PlayerState>(player_ent).unwrap();
            player_state.active_bullets.push((bullet_ent, bullet_coll));
        }
    }
}
