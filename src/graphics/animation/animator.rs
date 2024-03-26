use super::gltf_animation::{AnimatedProperty, GltfAnimation, Target};
use crate::graphics::{
    manager::{AnimationId, SkinId},
    mesh::skin::Skin,
};

#[derive(Debug)]
pub struct Animator {
    pub t: f32,
    pub(crate) animation: AnimationId,
    pub(crate) target_override: Option<SkinId>,
}

impl Animator {
    pub fn new(animation: AnimationId) -> Self {
        Self {
            t: 0.,
            animation,
            target_override: None,
        }
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
