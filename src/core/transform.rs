use std::f32::consts::PI;
use ultraviolet as uv;

/// An angle in either degrees or radians.
/// Default conversion from f32 is in degrees.
pub enum Angle {
    Radians(f32),
    Degrees(f32),
}

impl Angle {
    /// Get the angle as degrees.
    pub fn deg(&self) -> f32 {
        match self {
            Angle::Radians(rad) => rad * 180.0 / PI,
            Angle::Degrees(deg) => *deg,
        }
    }

    /// Get the angle as radians.
    pub fn rad(&self) -> f32 {
        match self {
            Angle::Radians(rad) => *rad,
            Angle::Degrees(deg) => deg * PI / 180.0,
        }
    }
}

impl From<f32> for Angle {
    fn from(n: f32) -> Self {
        Angle::Degrees(n)
    }
}

impl Into<uv::Rotor2> for Angle {
    fn into(self) -> uv::Rotor2 {
        uv::Rotor2::from_angle(self.rad())
    }
}

impl Default for Angle {
    fn default() -> Self {
        Angle::Radians(0.0)
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(from = "SerializeIntermediary")]
#[serde(into = "SerializeIntermediary")]
pub struct Transform(pub uv::Similarity2);

impl crate::ecs::DefaultStorage for Transform {
    type DefaultStorage = crate::ecs::storage::VecStorage<Self>;
}

impl Transform {
    pub fn identity() -> Self {
        Transform(uv::Similarity2::identity())
    }

    pub fn new(pos: uv::Vec2, angle: Angle, scaling: f32) -> Self {
        Transform(uv::Similarity2::new(pos, angle.into(), scaling))
    }

    pub fn from_position(pos: uv::Vec2) -> Self {
        Transform(uv::Similarity2::new(pos, uv::Rotor2::default(), 1.0))
    }

    pub fn from_angle(angle: Angle) -> Self {
        Transform(uv::Similarity2::new(
            uv::Vec2::new(0.0, 0.0),
            angle.into(),
            1.0,
        ))
    }

    pub fn from_scaling(scaling: f32) -> Self {
        Transform(uv::Similarity2::new(
            uv::Vec2::new(0.0, 0.0),
            uv::Rotor2::default(),
            scaling,
        ))
    }

    pub fn angle(&self) -> Angle {
        Angle::Radians((self.0.rotation * uv::Vec2::unit_x()).x.acos())
    }
}

use crate::core::{storage, Container};
pub type TransformFeature = Container<storage::VecStorage<Transform>>;

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct SerializeIntermediary {
    position: [f32; 2],
    rotation: f32,
    scaling: f32,
}

impl Default for SerializeIntermediary {
    fn default() -> Self {
        SerializeIntermediary {
            position: [0.0; 2],
            rotation: 0.0,
            scaling: 1.0,
        }
    }
}

impl From<Transform> for SerializeIntermediary {
    fn from(tr: Transform) -> Self {
        SerializeIntermediary {
            position: [tr.0.translation.x, tr.0.translation.y],
            rotation: tr.angle().deg(),
            scaling: tr.0.scale,
        }
    }
}

impl From<SerializeIntermediary> for Transform {
    fn from(s: SerializeIntermediary) -> Self {
        Transform(uv::Similarity2::new(
            s.position.into(),
            Angle::Degrees(s.rotation).into(),
            s.scaling,
        ))
    }
}
