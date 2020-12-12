//! Types, aliases and helper operations for doing math with `ultraviolet`.
use std::f32::consts::PI;
pub use ultraviolet as uv;

/// An angle in either degrees or radians.
/// Default conversion from f32 is in degrees.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum Angle {
    Rad(f32),
    Deg(f32),
}
impl Angle {
    /// Get the angle as degrees.
    pub fn deg(&self) -> f32 {
        match self {
            Angle::Rad(rad) => rad * 180.0 / PI,
            Angle::Deg(deg) => *deg,
        }
    }

    /// Get the angle as radians.
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
impl Into<uv::Rotor2> for Angle {
    fn into(self) -> uv::Rotor2 {
        uv::Rotor2::from_angle(self.rad())
    }
}

/// A wrapper type to indicate a vector should always be normalized.
#[derive(Clone, Copy, Debug)]
pub struct Unit<T>(T);

impl Unit<uv::Vec2> {
    pub fn new_normalize(v: uv::Vec2) -> Self {
        Unit(v.normalized())
    }

    pub fn new_unchecked(v: uv::Vec2) -> Self {
        Unit(v)
    }

    pub fn unit_x() -> Self {
        Unit(uv::Vec2::unit_x())
    }

    pub fn unit_y() -> Self {
        Unit(uv::Vec2::unit_y())
    }
}

impl std::ops::Mul<Unit<uv::Vec2>> for uv::Rotor2 {
    type Output = Unit<uv::Vec2>;

    fn mul(self, rhs: Unit<uv::Vec2>) -> Self::Output {
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
pub struct IsometryBuilder {
    position: [f32; 2],
    rotation: Angle,
}
impl IsometryBuilder {
    pub fn new() -> Self {
        IsometryBuilder {
            position: [0.0, 0.0],
            rotation: Angle::default(),
        }
    }
    pub fn with_position(mut self, pos: impl Into<[f32; 2]>) -> Self {
        self.position = pos.into();
        self
    }
    pub fn with_rotation(mut self, angle: Angle) -> Self {
        self.rotation = angle;
        self
    }
    pub fn build(self) -> uv::Isometry2 {
        uv::Isometry2::new(
            uv::Vec2::new(self.position[0], self.position[1]),
            self.rotation.into(),
        )
    }
}
impl Default for IsometryBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl Into<uv::Isometry2> for IsometryBuilder {
    fn into(self) -> uv::Isometry2 {
        self.build()
    }
}
impl From<[f32; 2]> for IsometryBuilder {
    fn from(vec: [f32; 2]) -> Self {
        IsometryBuilder::new().with_position(vec)
    }
}
impl From<uv::Vec2> for IsometryBuilder {
    fn from(vec: uv::Vec2) -> Self {
        IsometryBuilder::new().with_position(vec)
    }
}
impl From<Angle> for IsometryBuilder {
    fn from(angle: Angle) -> Self {
        IsometryBuilder::new().with_rotation(angle)
    }
}

// Vec2 utils

pub fn left_normal(v: uv::Vec2) -> uv::Vec2 {
    uv::Vec2::new(-v.y, v.x)
}

pub fn right_normal(v: uv::Vec2) -> uv::Vec2 {
    uv::Vec2::new(v.y, -v.x)
}
