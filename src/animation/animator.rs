use super::gltf_animation::{AnimatedProperty, GltfAnimation, Target};
use crate::{
    graph,
    graphics::mesh::{self, skin},
    math::uv,
};

/// Component that handles animating a skinned mesh.
pub struct MeshAnimator {
    animations: Vec<GltfAnimation>,
    active_anim_idx: Option<usize>,
    anim_time: f32,
}

/// Error type indicating that an animation with a given name does not exist.
#[derive(Debug, Clone, Copy)]
pub struct AnimationNotFoundError;

impl MeshAnimator {
    pub fn new(animations: Vec<GltfAnimation>) -> Self {
        Self {
            animations,
            active_anim_idx: None,
            anim_time: 0.0,
        }
    }

    /// Activate the animation with the given name, if it exists.
    /// Returns an error if an animation with the given name doesn't exist.
    pub fn activate_animation(&mut self, name: &str) -> Result<(), AnimationNotFoundError> {
        match self
            .animations
            .iter()
            .enumerate()
            .find(|(_, anim)| anim.name.as_deref() == Some(name))
        {
            Some((idx, _)) => self.active_anim_idx = Some(idx),
            None => return Err(AnimationNotFoundError),
        }
        Ok(())
    }
}

//
// systems
//

/// Step all animators forward in time.
pub fn step_time(dt: f32, mut l_anim: graph::LayerViewMut<MeshAnimator>) {
    for animator in l_anim.iter_mut() {
        if let Some(anim_idx) = animator.c.active_anim_idx {
            animator.c.anim_time += dt;
            // loop back to the start if we're past the end
            let overshoot = animator.c.anim_time - animator.c.animations[anim_idx].duration;
            if overshoot > 0.0 {
                animator.c.anim_time = overshoot;
            }
        }
    }
}

pub fn update_joints(
    (l_anim, mut l_skin): (
        graph::LayerView<MeshAnimator>,
        graph::LayerViewMut<mesh::Skin>,
    ),
) {
    // cache global poses so we only compute each of them once
    // (rather than every time a child joint is computed)
    let mut global_poses: Vec<Option<uv::Mat4>> = Vec::new();

    for anim in l_anim.iter() {
        let Some(skin) = anim.get_neighbor_mut(&mut l_skin) else { continue; };

        // sample animations

        if let Some(anim_idx) = anim.c.active_anim_idx {
            let active_anim = &anim.c.animations[anim_idx];
            for channel in &active_anim.channels {
                let Target::Joint { id, property } = channel.target;
                let joint = &mut skin.c.joints[id];
                match property {
                    AnimatedProperty::Translation => {
                        joint.local_pose.pos = channel.sample_vec3(anim.c.anim_time);
                    }
                    AnimatedProperty::Rotation => {
                        joint.local_pose.rot = channel.sample_rotor3(anim.c.anim_time);
                    }
                    AnimatedProperty::Scale => {
                        joint.local_pose.scale = channel.sample_vec3(anim.c.anim_time);
                    }
                }
            }
        }

        // recompute joint matrices

        global_poses.clear();
        global_poses.extend(std::iter::repeat(None).take(skin.c.joints.len()));
        for joint_idx in 0..skin.c.joints.len() {
            // traverse recursively until an already computed global parent transform is found
            fn populate_parents(
                joint_idx: usize,
                joints: &[skin::Joint],
                global_poses: &mut [Option<uv::Mat4>],
            ) {
                if let Some(parent_idx) = joints[joint_idx].parent_idx {
                    if global_poses[parent_idx].is_none() {
                        populate_parents(parent_idx, joints, global_poses);
                    }
                    global_poses[joint_idx] = Some(
                        // global pose is guaranteed to exist because we just called
                        // populate_parents if it didn't
                        global_poses[parent_idx].unwrap()
                            * joints[joint_idx].local_pose.as_matrix(),
                    );
                } else {
                    global_poses[joint_idx] = Some(joints[joint_idx].local_pose.as_matrix());
                }
            }
            populate_parents(joint_idx, &skin.c.joints, &mut global_poses);
            let joint_pose = global_poses[joint_idx].unwrap();

            skin.c.joints[joint_idx].joint_matrix =
                skin.c.root_transform * joint_pose * skin.c.joints[joint_idx].inv_bind_matrix;
        }
    }
}
