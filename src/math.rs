//! Types, aliases and helper operations for doing math with `ultraviolet`.
use std::f64::consts::PI;
pub use ultraviolet as uv;

pub type Pose = uv::DIsometry2;
pub type Vec2 = uv::DVec2;
pub type Rotor2 = uv::DRotor2;

/// An angle in either degrees or radians.
/// Default conversion from f64 is in degrees.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum Angle {
    Rad(f64),
    Deg(f64),
}
impl Angle {
    /// Get the angle as degrees.
    #[inline]
    pub fn deg(&self) -> f64 {
        match self {
            Angle::Rad(rad) => rad * 180.0 / PI,
            Angle::Deg(deg) => *deg,
        }
    }

    /// Get the angle as radians.
    #[inline]
    pub fn rad(&self) -> f64 {
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
    #[inline]
    fn from(ang: Angle) -> Rotor2 {
        Rotor2::from_angle(ang.rad())
    }
}
impl From<Rotor2> for Angle {
    #[inline]
    fn from(rotor: Rotor2) -> Self {
        Angle::Rad(-rotor.bv.xy.atan2(rotor.s) * 2.0)
    }
}

/// A wrapper type to indicate a vector should always be normalized.
#[derive(Clone, Copy, Debug)]
pub struct Unit<T>(T);

impl Unit<Vec2> {
    pub fn new_normalize(v: Vec2) -> Self {
        Unit(v.normalized())
    }

    pub const fn new_unchecked(v: Vec2) -> Self {
        Unit(v)
    }

    pub fn unit_x() -> Self {
        Unit(Vec2::unit_x())
    }

    pub fn unit_y() -> Self {
        Unit(Vec2::unit_y())
    }
}

impl std::ops::Mul<Unit<Vec2>> for Rotor2 {
    type Output = Unit<Vec2>;

    fn mul(self, rhs: Unit<Vec2>) -> Self::Output {
        Unit(self * rhs.0)
    }
}

impl<T> std::ops::Deref for Unit<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::Neg for Unit<T>
where
    T: std::ops::Neg,
{
    type Output = Unit<<T as std::ops::Neg>::Output>;

    fn neg(self) -> Self::Output {
        Unit(-self.0)
    }
}

/// A builder useful for deserializing isometries from RON files.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PoseBuilder {
    position: [f64; 2],
    rotation: Angle,
}
impl PoseBuilder {
    pub fn new() -> Self {
        PoseBuilder {
            position: [0.0, 0.0],
            rotation: Angle::default(),
        }
    }
    #[inline]
    pub fn with_position(mut self, pos: impl Into<[f64; 2]>) -> Self {
        self.position = pos.into();
        self
    }
    #[inline]
    pub fn with_rotation(mut self, angle: Angle) -> Self {
        self.rotation = angle;
        self
    }
    #[inline]
    pub fn build(self) -> Pose {
        Pose::new(
            Vec2::new(self.position[0], self.position[1]),
            self.rotation.into(),
        )
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
impl From<[f64; 2]> for PoseBuilder {
    fn from(vec: [f64; 2]) -> Self {
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

// Vec2 utils

#[inline]
pub fn left_normal(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}
#[inline]
pub fn right_normal(v: Vec2) -> Vec2 {
    Vec2::new(v.y, -v.x)
}
#[inline]
pub fn unit_left_normal(u: Unit<Vec2>) -> Unit<Vec2> {
    Unit::new_unchecked(left_normal(*u))
}
#[inline]
pub fn unit_right_normal(u: Unit<Vec2>) -> Unit<Vec2> {
    Unit::new_unchecked(right_normal(*u))
}
