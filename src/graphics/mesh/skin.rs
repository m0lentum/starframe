use crate::math::uv;

/// A hierarchy of joints used for deforming and animating meshes.
#[derive(Debug, Clone)]
pub struct Skin {
    pub root_transform: uv::Mat4,
    pub joints: Vec<Joint>,
}

#[derive(Debug, Clone)]
pub struct Joint {
    pub name: Option<String>,
    /// index of the joint's parent joint in the skin's `joints` array
    pub parent_idx: Option<usize>,
    /// inverse bind matrix, stays constant
    pub inv_bind_matrix: uv::Mat4,
    /// pose relative to the parent joint, updated by animations
    pub local_pose: TransformDecomp,
    /// the final joint transform for use in rendering, also updated by animations
    pub joint_matrix: uv::Mat4,
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
