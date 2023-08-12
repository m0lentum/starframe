use starframe as sf;

#[derive(Clone, Copy, Debug)]
pub struct MouseGrabber {
    constraint: Option<sf::ConstraintKey>,
}

impl MouseGrabber {
    pub fn new() -> Self {
        Self { constraint: None }
    }

    pub fn update(
        &mut self,
        input: &sf::Input,
        camera: &sf::Camera,
        physics: &mut sf::PhysicsWorld,
    ) {
        if input.button(sf::ButtonQuery::mouse(sf::MouseButton::Left).held()) {
            let target_point = input.cursor_position_world(camera);
            match self.constraint {
                Some(handle) => {
                    if let Some(constr) = physics.constraint_set.get_mut(handle) {
                        constr.offsets[1] = target_point;
                    }
                }
                None => {
                    let Some(body_key) = physics.query_point(target_point).find_map(|(_, b)| b) else {return; };
                    let Some(body) = physics.entity_set.get_body(body_key) else { return; };
                    let constr = sf::ConstraintBuilder::new(body_key)
                        .with_origin(body.pose.inversed() * target_point)
                        .with_target_origin(target_point)
                        .with_compliance(0.01)
                        .with_linear_damping(10.0)
                        .with_angular_damping(0.5)
                        .disable_sleeping()
                        .build_attachment();
                    self.constraint = Some(physics.constraint_set.insert(constr));
                }
            }
        } else if let Some(key) = self.constraint {
            physics.constraint_set.remove(key);
            self.constraint = None;
        }
    }
}
