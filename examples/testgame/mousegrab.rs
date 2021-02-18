use sf::{
    graphics::camera::Camera,
    physics::{Constraint, ConstraintBuilder, ConstraintHandle, ConstraintType},
};
use starframe as sf;

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
        graph: &crate::MyGraph,
    ) {
        if input.is_mouse_button_pressed(sf::input::MouseButton::Left, None) {
            let target_point =
                camera.point_screen_to_world(viewport_size, input.cursor_position().into());
            match self.constraint {
                Some(handle) => {
                    if let Some(Constraint {
                        ty: ConstraintType::Distance { offsets, .. },
                        ..
                    }) = physics.get_constraint_mut(handle)
                    {
                        offsets[1] = target_point;
                    }
                }
                None => {
                    if let Some((pose, _, rb)) = physics.query_point_body(
                        &graph.graph,
                        &graph.l_pose,
                        &graph.l_collider,
                        &graph.l_body,
                        target_point,
                    ) {
                        let constr =
                            ConstraintBuilder::new(sf::graph::NodeRef::as_node(&rb, &graph.graph))
                                .with_origin(pose.inversed() * target_point)
                                .with_target_origin(target_point)
                                .with_compliance(0.05)
                                .build_attachment();
                        self.constraint = Some(physics.add_constraint(constr));
                    }
                }
            }
        } else {
            if let Some(handle) = self.constraint {
                physics.remove_constraint(handle);
                self.constraint = None;
            }
        }
    }
}
