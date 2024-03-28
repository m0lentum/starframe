use crate::math::uv;

use itertools::izip;
use std::rc::Rc;

/// A hierarchy of joints used for deforming and animating meshes.
#[derive(Debug, Clone)]
pub struct Skin {
    pub(crate) root_transform: uv::Mat4,
    pub(crate) joint_set: JointSet,
    /// inverse bind matrices shared between clones of the skin
    pub(crate) inv_bind_matrices: Rc<[uv::Mat4]>,
}

impl Skin {
    pub fn update_global_poses(&mut self) {
        self.joint_set.update_global_poses();
    }

    /// Update the joint matrices in this skin
    /// based on values of the joints' local poses.
    pub fn evaluate_joint_matrices(&self) -> Vec<uv::Mat4> {
        self.joint_set
            .evaluate_joint_matrices(&self.root_transform, &self.inv_bind_matrices)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JointSet {
    pub joints: Vec<Joint>,
}

#[derive(Debug, Clone)]
pub struct Joint {
    pub name: Option<String>,
    /// index of the joint's parent joint in the skin's `joints` array
    pub parent_idx: Option<usize>,
    /// pose relative to the parent joint, updated by animations
    pub local_pose: TransformDecomp,
    /// pose in model space, as a matrix because nonuniform scalings are allowed
    /// and these can't be composed nicely outside of matrix form.
    pub global_pose: uv::Mat4,
}

impl JointSet {
    pub(crate) fn update_global_poses(&mut self) {
        // joints are given in breadth-first order, so we don't need recursion here,
        // the order automatically makes it so parents are evaluated first
        for joint_idx in 0..self.joints.len() {
            let local = self.joints[joint_idx].local_pose.as_matrix();
            if let Some(parent_idx) = self.joints[joint_idx].parent_idx {
                // assert to make sure the aforementioned order holds,
                // this should always be correct at least with Blender
                assert!(
                    parent_idx < joint_idx,
                    "Joints must be given in breadth-first order"
                );
                self.joints[joint_idx].global_pose = self.joints[parent_idx].global_pose * local;
            } else {
                self.joints[joint_idx].global_pose = local;
            }
        }
    }

    /// Compute joint matrices from the global poses,
    /// which should have been updated beforehand.
    ///
    /// `root_transform` and `inv_bind_matrices`
    /// should be those of the skin owning this joint set.
    pub(crate) fn evaluate_joint_matrices(
        &self,
        root_transform: &uv::Mat4,
        inv_bind_matrices: &[uv::Mat4],
    ) -> Vec<uv::Mat4> {
        izip!(self.joints.iter().map(|j| j.global_pose), inv_bind_matrices)
            .map(|(global_pose, inv_bind)| *root_transform * global_pose * *inv_bind)
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransformDecomp {
    pub pos: uv::Vec3,
    pub rot: uv::Rotor3,
    pub scale: uv::Vec3,
}

impl TransformDecomp {
    pub fn from_parts((pos, rot_quat, scale): ([f32; 3], [f32; 4], [f32; 3])) -> Self {
        Self {
            pos: pos.into(),
            rot: uv::Rotor3::from_quaternion_array(rot_quat),
            scale: scale.into(),
        }
    }

    pub fn as_matrix(&self) -> uv::Mat4 {
        uv::Mat4::from_translation(self.pos)
            * self.rot.into_matrix().into_homogeneous()
            * uv::Mat4::from_nonuniform_scale(self.scale)
    }
}
