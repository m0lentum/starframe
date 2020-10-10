//! Types, aliases and helper operations for doing math with `nalgebra`.
use nalgebra as na;
use std::f32::consts::PI;

pub type Transform = na::Similarity2<f32>;
pub type Vec2 = na::Vector2<f32>;
pub type Point2 = na::Point2<f32>;
pub type Mat3 = na::Matrix3<f32>;

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
impl Into<na::UnitComplex<f32>> for Angle {
    fn into(self) -> na::UnitComplex<f32> {
        na::UnitComplex::from_angle(self.rad())
    }
}

/// An intermediate struct that makes it easier to create a Transform,
/// as well as to write a deserializable one in a RON file.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TransformBuilder {
    position: [f32; 2],
    rotation: Angle,
    scale: f32,
}
impl TransformBuilder {
    pub fn new() -> Self {
        TransformBuilder {
            position: [0.0, 0.0],
            rotation: Angle::default(),
            scale: 1.0,
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
    pub fn with_scaling(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
    pub fn build(self) -> Transform {
        Transform::from_parts(
            Vec2::new(self.position[0], self.position[1]).into(),
            self.rotation.into(),
            self.scale,
        )
    }
}
impl Default for TransformBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl Into<Transform> for TransformBuilder {
    fn into(self) -> Transform {
        self.build()
    }
}
impl From<[f32; 2]> for TransformBuilder {
    fn from(vec: [f32; 2]) -> Self {
        TransformBuilder::new().with_position(vec)
    }
}
impl From<Vec2> for TransformBuilder {
    fn from(vec: Vec2) -> Self {
        TransformBuilder::new().with_position(vec)
    }
}
impl From<Point2> for TransformBuilder {
    fn from(pt: Point2) -> Self {
        TransformBuilder::new().with_position(pt.coords)
    }
}
impl From<Angle> for TransformBuilder {
    fn from(angle: Angle) -> Self {
        TransformBuilder::new().with_rotation(angle)
    }
}

// Vec2 utils

pub fn left_normal(v: &Vec2) -> Vec2 {
    Vec2::new(-v[1], v[0])
}

pub fn right_normal(v: &Vec2) -> Vec2 {
    Vec2::new(v[1], -v[0])
}
