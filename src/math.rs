//! Types, aliases and helper operations for doing math with `ultraviolet`.

use std::f32::consts::PI;

// public APIs using 3D single-length floats.
// physics uses f64 math internally, but we don't expose that to the user
// to keep physics and graphics APIs compatible

pub use ultraviolet as uv;
pub use uv::{Bivec2, Bivec3, DBivec2, DRotor2, DVec2, Rotor2, Rotor3, Vec2, Vec3};

pub type PhysicsPose = uv::DIsometry2;

/// The position, rotation and scaling of an entity,
/// also known as a transform, in 3D space.
///
/// This pose type should be used in the ECS world with your entities.
/// [`PhysicsPose`] is used internally by the physics engine.
///
/// # Physics compatibility
///
/// This type is a superset of the poses used in Starframe's physics engine.
/// The physics uses 2D isometries, meaning transformations
/// with only a translation and rotation in the xy plane.
/// The z component of position, rotations in the xz and yz planes,
/// and scaling are ignored in physics and only relevant to graphics.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose(pub uv::Similarity3);

impl AsRef<uv::Similarity3> for Pose {
    fn as_ref(&self) -> &uv::Similarity3 {
        &self.0
    }
}

impl std::borrow::Borrow<uv::Similarity3> for Pose {
    fn borrow(&self) -> &uv::Similarity3 {
        &self.0
    }
}

impl std::ops::Deref for Pose {
    type Target = uv::Similarity3;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Pose {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl std::ops::Mul<Pose> for Pose {
    type Output = Self;

    fn mul(self, rhs: Pose) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl std::ops::Mul<Vec3> for Pose {
    type Output = Vec3;

    fn mul(self, rhs: Vec3) -> Self::Output {
        self.0 * rhs
    }
}

impl std::ops::Mul<Vec2> for Pose {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        (self.0 * Vec3::new(rhs.x, rhs.y, 0.)).xy()
    }
}

impl Pose {
    /// Create a Pose from a 2D position and angle.
    ///
    /// The z coordinate and rotations in planes other than the xy plane
    /// are set to zero.
    #[inline]
    pub fn new(pos: Vec2, angle: Angle) -> Self {
        Self(uv::Similarity3::new(
            Vec3::new(pos.x, pos.y, 0.),
            Rotor3::from_rotation_xy(angle.rad()),
            1.,
        ))
    }

    /// Create an identity pose (position at the origin, no rotation).
    #[inline]
    pub fn identity() -> Self {
        Self(uv::Similarity3::identity())
    }

    /// Set the z coordinate of this pose in a builder-style fashion.
    ///
    /// If you need to mutate an existing pose,
    /// set `pose.translation.z` directly
    #[inline]
    pub fn with_depth(mut self, depth: f32) -> Self {
        self.translation.z = depth;
        self
    }

    /// Get the 2D part of this pose's position.
    #[inline]
    pub fn position_2d(&self) -> Vec2 {
        self.translation.xy()
    }

    /// Get the 2D part of this pose's rotation.
    #[inline]
    pub fn angle_2d(&self) -> Angle {
        Angle::Rad(-self.rotation.bv.xy.atan2(self.rotation.s) * 2.0)
    }

    /// Set the 2D parts of this pose to the ones defined by a physics pose.
    #[inline]
    pub fn sync_from_physics(&mut self, pose: PhysicsPose) {
        self.translation.x = pose.translation.x as f32;
        self.translation.y = pose.translation.y as f32;
        self.rotation.s = pose.rotation.s as f32;
        self.rotation.bv.xy = pose.rotation.bv.xy as f32;
    }
}

impl From<Pose> for PhysicsPose {
    fn from(pose: Pose) -> Self {
        let pos = pose.position_2d();
        PhysicsPose::new(
            uv::DVec2::new(pos.x as f64, pos.y as f64),
            uv::DRotor2::new(
                pose.rotation.s as f64,
                uv::DBivec2::new(pose.rotation.bv.xy as f64),
            ),
        )
    }
}

impl From<PhysicsPose> for Pose {
    fn from(pose: PhysicsPose) -> Self {
        Pose(uv::Similarity3::new(
            Vec3::new(pose.translation.x as f32, pose.translation.y as f32, 0.),
            Rotor3::new(
                pose.rotation.s as f32,
                Bivec3::new(pose.rotation.bv.xy as f32, 0., 0.),
            ),
            1.,
        ))
    }
}

/// An angle in either degrees or radians.
/// Default conversion from f64 is in degrees.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum Angle {
    Rad(f32),
    Deg(f32),
}
impl Angle {
    /// Get the angle as degrees.
    #[inline]
    pub fn deg(&self) -> f32 {
        match self {
            Angle::Rad(rad) => rad * 180.0 / PI,
            Angle::Deg(deg) => *deg,
        }
    }

    /// Get the angle as radians.
    #[inline]
    pub fn rad(&self) -> f32 {
        match self {
            Angle::Rad(rad) => *rad,
            Angle::Deg(deg) => deg * PI / 180.0,
        }
    }
}
impl Default for Angle {
    fn default() -> Self {
        Angle::Rad(0.0)
    }
}
impl From<Angle> for Rotor2 {
    fn from(ang: Angle) -> Rotor2 {
        Rotor2::from_angle(ang.rad())
    }
}
impl From<Angle> for uv::DRotor2 {
    fn from(ang: Angle) -> uv::DRotor2 {
        uv::DRotor2::from_angle(ang.rad() as f64)
    }
}
impl From<Rotor2> for Angle {
    #[inline]
    fn from(rotor: Rotor2) -> Self {
        Angle::Rad(-rotor.bv.xy.atan2(rotor.s) * 2.0)
    }
}
impl From<Angle> for f32 {
    fn from(ang: Angle) -> Self {
        ang.rad()
    }
}
impl From<Angle> for f64 {
    fn from(ang: Angle) -> Self {
        ang.rad() as f64
    }
}

/// A 2D unit vector. Used in physics.
#[derive(Clone, Copy, Debug)]
pub struct UnitDVec2(uv::DVec2);

impl UnitDVec2 {
    pub fn new_normalize(v: uv::DVec2) -> Self {
        Self(v.normalized())
    }

    pub const fn new_unchecked(v: uv::DVec2) -> Self {
        Self(v)
    }

    pub fn unit_x() -> Self {
        Self(uv::DVec2::unit_x())
    }

    pub fn unit_y() -> Self {
        Self(uv::DVec2::unit_y())
    }
}

impl std::ops::Mul<UnitDVec2> for uv::DRotor2 {
    type Output = UnitDVec2;

    fn mul(self, rhs: UnitDVec2) -> Self::Output {
        UnitDVec2(self * *rhs)
    }
}

impl std::ops::Deref for UnitDVec2 {
    type Target = uv::DVec2;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Neg for UnitDVec2 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

//
// pose builder
//

/// A builder to create [`Pose`][self::Pose]s.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PoseBuilder {
    pub position: [f32; 2],
    pub depth: f32,
    pub rotation: Angle,
}
impl PoseBuilder {
    pub fn new() -> Self {
        PoseBuilder {
            position: [0., 0.],
            depth: 0.,
            rotation: Angle::default(),
        }
    }

    #[inline]
    pub fn with_position(mut self, pos: impl Into<[f32; 2]>) -> Self {
        self.position = pos.into();
        self
    }

    #[inline]
    pub fn with_depth(mut self, depth: f32) -> Self {
        self.depth = depth;
        self
    }

    #[inline]
    pub fn with_rotation(mut self, angle: Angle) -> Self {
        self.rotation = angle;
        self
    }

    #[inline]
    pub fn build(self) -> Pose {
        Pose::new(Vec2::new(self.position[0], self.position[1]), self.rotation)
            .with_depth(self.depth)
    }
}
impl Default for PoseBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl From<PoseBuilder> for Pose {
    fn from(iso: PoseBuilder) -> Pose {
        iso.build()
    }
}
impl From<[f32; 2]> for PoseBuilder {
    fn from(vec: [f32; 2]) -> Self {
        PoseBuilder::new().with_position(vec)
    }
}
impl From<Vec2> for PoseBuilder {
    fn from(vec: Vec2) -> Self {
        PoseBuilder::new().with_position(vec)
    }
}
impl From<Angle> for PoseBuilder {
    fn from(angle: Angle) -> Self {
        PoseBuilder::new().with_rotation(angle)
    }
}
impl From<Pose> for PoseBuilder {
    fn from(pose: Pose) -> Self {
        let pos = pose.position_2d();
        Self {
            position: [pos.x, pos.y],
            depth: 0.,
            rotation: pose.angle_2d(),
        }
    }
}

/// Module to deserialize `Pose`s from `PoseBuilder` format without manually converting,
/// using the serde attribute `#[serde(with = "serde_pose")]`.
#[cfg(feature = "serde-types")]
pub mod serde_pose {
    use super::*;

    pub fn serialize<S>(pose: &Pose, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::Serialize;
        PoseBuilder::from(*pose).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pose, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        PoseBuilder::deserialize(deserializer).map(|p| p.build())
    }
}

/// Module to deserialize `PhysicsPose`s from `PoseBuilder` format without manually converting,
/// using the serde attribute `#[serde(with = "serde_physics_pose")]`.
#[cfg(feature = "serde-types")]
pub mod serde_physics_pose {
    use super::*;

    pub fn serialize<S>(pose: &PhysicsPose, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::Serialize;
        PoseBuilder::from(Pose::from(*pose)).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PhysicsPose, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        PoseBuilder::deserialize(deserializer).map(|p| p.build().into())
    }
}

//
// Vec2 utils
//

#[inline]
pub fn left_normal(v: uv::DVec2) -> uv::DVec2 {
    uv::DVec2::new(-v.y, v.x)
}
#[inline]
pub fn right_normal(v: uv::DVec2) -> uv::DVec2 {
    uv::DVec2::new(v.y, -v.x)
}
#[inline]
pub fn unit_left_normal(u: UnitDVec2) -> UnitDVec2 {
    UnitDVec2::new_unchecked(left_normal(*u))
}
#[inline]
pub fn unit_right_normal(u: UnitDVec2) -> UnitDVec2 {
    UnitDVec2::new_unchecked(right_normal(*u))
}

/// Trait facilitating conversions between f64 types (for physics)
/// and f32 types (for everything else).
pub trait ConvertPrecision {
    type Target;

    fn conv_p(&self) -> Self::Target;
}

impl ConvertPrecision for uv::DVec2 {
    type Target = uv::Vec2;

    fn conv_p(&self) -> Self::Target {
        uv::Vec2::new(self.x as f32, self.y as f32)
    }
}
impl ConvertPrecision for uv::Vec2 {
    type Target = uv::DVec2;

    fn conv_p(&self) -> Self::Target {
        uv::DVec2::new(self.x as f64, self.y as f64)
    }
}
