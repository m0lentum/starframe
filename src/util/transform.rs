use nalgebra::geometry::UnitComplex;
use nalgebra::{Point2, Similarity2, Translation2, Vector2};
use std::f32::consts::PI;

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

impl From<UnitComplex<f32>> for Angle {
    fn from(uc: UnitComplex<f32>) -> Self {
        Angle::Radians(uc.angle())
    }
}

impl Default for Angle {
    fn default() -> Self {
        Angle::Radians(0.0)
    }
}

/// A wrapper on top of a nalgebra::Similarity2<f32> that adds some useful methods.
/// All Similarity2 methods and members can be accessed from a Transform reference thanks to shrinkwraprs.
/// See https://www.nalgebra.org/rustdoc/nalgebra/geometry/struct.Similarity.html
#[derive(Clone, Copy, Debug, shrinkwraprs::Shrinkwrap)]
#[shrinkwrap(mutable)]
#[cfg_attr(feature = "ron-recipes", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "ron-recipes", serde(from = "SerializeIntermediary"))]
#[cfg_attr(feature = "ron-recipes", serde(into = "SerializeIntermediary"))]
pub struct Transform(pub Similarity2<f32>);

impl Transform {
    /// The identity transform, i.e. no translation, rotation or scaling.
    pub fn identity() -> Self {
        Transform(Similarity2::identity())
    }

    /// Create a new Transform with an initial position, rotation and scale.
    pub fn new(position: impl Into<Vector2<f32>>, rotation: impl Into<Angle>, scale: f32) -> Self {
        Transform(Similarity2::new(
            position.into(),
            rotation.into().rad(),
            scale,
        ))
    }

    /// Create a transform with just a position.
    pub fn from_position(pos: impl Into<Vector2<f32>>) -> Self {
        Self::new(pos.into(), Angle::default(), 1.0)
    }

    /// Create a transform with just a rotation.
    pub fn from_rotation(angle: impl Into<Angle>) -> Self {
        Self::new(Vector2::zeros(), angle, 1.0)
    }

    /// Create a transform with just a scaling.
    pub fn from_scaling(s: f32) -> Self {
        Transform(Similarity2::from_scaling(s))
    }

    pub fn translate(&mut self, amount: impl Into<Vector2<f32>>) {
        self.isometry
            .append_translation_mut(&Translation2::from(amount.into()));
    }

    pub fn set_position(&mut self, pos: impl Into<Vector2<f32>>) {
        self.isometry.translation = nalgebra::Translation2::from(pos.into());
    }

    /// Position as a Vector2.
    pub fn translation(&self) -> Vector2<f32> {
        self.isometry.translation.vector
    }

    /// Position as a Point2.
    pub fn position(&self) -> Point2<f32> {
        Point2::from(self.translation())
    }

    pub fn rotate(&mut self, angle: impl Into<Angle>) {
        self.isometry
            .append_rotation_wrt_center_mut(&UnitComplex::new(angle.into().rad()));
    }

    pub fn rotation(&self) -> Angle {
        Angle::Radians(self.isometry.rotation.angle())
    }

    pub fn set_rotation(&mut self, angle: impl Into<Angle>) {
        self.isometry.rotation = UnitComplex::new(angle.into().rad());
    }

    pub fn scaling(&self) -> f32 {
        self.0.scaling()
    }

    pub fn multiply_scaling(&mut self, factor: f32) {
        self.append_scaling(factor);
    }

    pub fn set_scaling(&mut self, s: f32) {
        self.0.set_scaling(s);
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl crate::ecs::DefaultStorage for Transform {
    type DefaultStorage = crate::ecs::storage::VecStorage<Self>;
}

#[cfg(feature = "ron-recipes")]
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct SerializeIntermediary {
    position: [f32; 2],
    rotation: f32,
    scaling: f32,
}

impl Default for SerializeIntermediary {
    fn default() -> Self {
        Transform::default().into()
    }
}

impl From<Transform> for SerializeIntermediary {
    fn from(tr: Transform) -> Self {
        SerializeIntermediary {
            position: tr.position().coords.into(),
            rotation: tr.rotation().deg(),
            scaling: tr.scaling(),
        }
    }
}

impl From<SerializeIntermediary> for Transform {
    fn from(s: SerializeIntermediary) -> Self {
        Transform::new(
            Vector2::from(s.position),
            Angle::Degrees(s.rotation),
            s.scaling,
        )
    }
}
