use super::gltf_animation::{AnimatedProperty, GltfAnimation, Target};
use crate::graphics::{
    manager::{AnimationId, MeshId},
    mesh::skin::Skin,
};

#[derive(Debug)]
pub struct Animator {
    pub t: f32,
    pub(crate) animation: AnimationId,
    pub(crate) target: Option<MeshId>,
}

impl Animator {
    pub fn new(animation: AnimationId) -> Self {
        Self {
            t: 0.,
            animation,
            target: None,
        }
    }

    /// Set the target of this animation to a different mesh instance from the default.
    ///
    /// Not needed if there is only one animation state associated with a mesh,
    /// as the animation will be targeted to that state by default. Use with
    /// [`GraphicsManager::new_animation_target`][crate::GraphicsManager::new_animation_target].
    pub fn with_target(mut self, mesh_id: MeshId) -> Self {
        self.target = Some(mesh_id);
        self
    }

    /// Step this animator forward `dt` seconds.
    /// Resolving the `animation` is done by the asset manager.
    pub(crate) fn step_time(&mut self, dt: f32, animation: &GltfAnimation) {
        self.t += dt;
        // loop back to the start if we're past the end
        let overshoot = self.t - animation.duration;
        if overshoot > 0.0 {
            self.t = overshoot;
        }
    }

    /// Update the skin this animation targets.
    /// Resolving the `animation` and `skin` is done by the asset manager.
    pub(crate) fn update_skin(&self, animation: &GltfAnimation, skin: &mut Skin) {
        for channel in &animation.channels {
            let Target::Joint { id, property } = channel.target;
            let joint = &mut skin.joint_set.joints[id];
            match property {
                AnimatedProperty::Translation => {
                    joint.local_pose.pos = channel.sample_vec3(self.t);
                }
                AnimatedProperty::Rotation => {
                    joint.local_pose.rot = channel.sample_rotor3(self.t);
                }
                AnimatedProperty::Scale => {
                    joint.local_pose.scale = channel.sample_vec3(self.t);
                }
            }
        }
    }
}
