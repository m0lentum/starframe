use nalgebra::geometry::UnitComplex;
use nalgebra::{Point2, Similarity2, Translation2, Vector2};
use std::f32::consts::PI;

/// A wrapper on top of a nalgebra::Similarity2<f32> that adds some useful methods.
/// All Similarity2 methods and members can be accessed from a Transform reference thanks to shrinkwraprs.
/// See https://www.nalgebra.org/rustdoc/nalgebra/geometry/struct.Similarity.html
#[derive(Clone, Copy, shrinkwraprs::Shrinkwrap)]
#[shrinkwrap(mutable)]
pub struct Transform(pub Similarity2<f32>);

impl Transform {
    /// The identity transform, i.e. no translation, rotation or scaling.
    pub fn identity() -> Self {
        Transform(Similarity2::identity())
    }

    /// Create a new Transform with an initial position, rotation and scale.
    pub fn new(translation: Vector2<f32>, rotation: f32, scale: f32) -> Self {
        Transform(Similarity2::new(translation, rotation, scale))
    }

    /// Create a transform with just a position.
    pub fn from_position(pos: Point2<f32>) -> Self {
        Self::new(pos.coords, 0.0, 1.0)
    }

    /// Like `from_position`, but with the position expressed as a Vector2.
    pub fn from_translation(vec: Vector2<f32>) -> Self {
        Self::new(vec, 0.0, 1.0)
    }

    /// Like `from_position`, bbut with the position expressed as two floats.
    pub fn from_coords(x: f32, y: f32) -> Self {
        Self::new(Vector2::new(x, y), 0.0, 1.0)
    }

    /// Create a transform with just a rotation, expressed in radians.
    pub fn from_rotation_rad(angle: f32) -> Self {
        Self::new(Vector2::zeros(), angle, 1.0)
    }

    /// Create a transform with just a rotation, expressed in degrees.
    pub fn from_rotation_deg(angle: f32) -> Self {
        Self::from_rotation_rad(angle * PI / 180.0)
    }

    /// Create a transform with just a scaling.
    pub fn from_scaling(s: f32) -> Self {
        Transform(Similarity2::from_scaling(s))
    }

    pub fn translate(&mut self, amount: Vector2<f32>) {
        self.isometry
            .append_translation_mut(&Translation2::from(amount));
    }

    pub fn set_translation(&mut self, pos: Vector2<f32>) {
        self.isometry.translation = nalgebra::Translation2::from(pos);
    }

    /// Position as a Vector2.
    pub fn translation(&self) -> Vector2<f32> {
        self.isometry.translation.vector
    }

    /// Position as a Point2.
    pub fn position(&self) -> Point2<f32> {
        Point2::from(self.translation())
    }

    pub fn rotate_rad(&mut self, angle: f32) {
        self.isometry
            .append_rotation_wrt_center_mut(&UnitComplex::new(angle));
    }

    pub fn rotate_deg(&mut self, angle: f32) {
        self.rotate_rad(angle * PI / 180.0);
    }

    pub fn rotation_rad(&self) -> f32 {
        self.isometry.rotation.angle()
    }

    pub fn rotation_deg(&self) -> f32 {
        self.isometry.rotation.angle() * 180.0 / PI
    }

    pub fn set_rotation_rad(&mut self, angle: f32) {
        self.isometry.rotation = UnitComplex::new(angle);
    }

    pub fn set_rotation_deg(&mut self, angle: f32) {
        self.set_rotation_rad(angle * PI / 180.0);
    }

    pub fn scale(&mut self, factor: f32) {
        self.append_scaling(factor);
    }

    pub fn set_scale(&mut self, s: f32) {
        self.set_scaling(s);
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}
