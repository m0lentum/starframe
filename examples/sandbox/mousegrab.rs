use starframe::{
    self as sf,
    graph::Graph,
    graphics::camera::Camera,
    input::{ButtonQuery, MouseButton},
    physics::{ConstraintBuilder, ConstraintHandle},
};

#[derive(Clone, Copy, Debug)]
pub struct MouseGrabber {
    constraint: Option<ConstraintHandle>,
}

impl MouseGrabber {
    pub fn new() -> Self {
        Self { constraint: None }
    }

    pub fn update(
        &mut self,
        input: &sf::InputCache,
        camera: &impl Camera,
        viewport_size: (u32, u32),
        physics: &mut sf::Physics,
        graph: &Graph,
    ) {
        if input.button(ButtonQuery::mouse(MouseButton::Left).held()) {
            let target_point =
                camera.point_screen_to_world(viewport_size, input.cursor_position().into());
            match self.constraint {
                Some(handle) => {
                    if let Some(constr) = physics.get_constraint_mut(handle) {
                        constr.offsets[1] = target_point;
                    }
                }
                None => {
                    let layers = graph.get_layer_bundle();
                    let body = physics.query_point_body(target_point, &layers).next();
                    if let Some((pose, _, rb)) = body {
                        let constr = ConstraintBuilder::new(rb.key())
                            .with_origin(pose.c.inversed() * target_point)
                            .with_target_origin(target_point)
                            .with_compliance(0.01)
                            .with_linear_damping(10.0)
                            .with_angular_damping(0.5)
                            .disable_sleeping()
                            .build_attachment();
                        self.constraint = Some(physics.add_constraint(constr));
                    }
                }
            }
        } else if let Some(handle) = self.constraint {
            physics.remove_constraint(handle);
            self.constraint = None;
        }
    }
}
