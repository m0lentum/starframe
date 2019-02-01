use graphics::math::Matrix2d;
use nalgebra::geometry::UnitComplex;
use nalgebra::{Similarity2, Translation2, Vector2};
use std::f32::consts::PI;

/// A wrapper on top of a nalgebra::Similarity2<f32> that adds some useful methods.
/// The wrapped Similarity is public so its methods can be used directly.
/// # MoleEngineSpace format
/// `Px,y&Rr&Ss` where T, R and S are character literals (standing for Position, Rotation
/// and Scale respectively) and x, y, r, and s are f32-parseable strings.
/// Rotation is expressed in degrees.
/// Any of these can be omitted, which will leave them at the default value (no transformation).
#[derive(Clone, Copy)]
pub struct Transform(pub Similarity2<f32>);

impl Transform {
    /// Create a new Transform with an initial position, rotation and scale.
    /// This is simply a slightly more concise syntax for Similarity2::new (with [f32;2] instead of Vector2<f32>).
    pub fn new(position: [f32; 2], rotation: f32, scale: f32) -> Self {
        Transform(Similarity2::new(Vector2::from(position), rotation, scale))
    }

    /// Create a transform with just a position.
    pub fn from_position(pos: [f32; 2]) -> Self {
        Self::new(pos, 0.0, 1.0)
    }

    /// Like `from_position`, but with the position expressed as a Vector2.
    pub fn from_translation(vec: Vector2<f32>) -> Self {
        Transform(Similarity2::new(vec, 0.0, 1.0))
    }

    /// Create a transform with just a rotation, expressed in radians.
    pub fn from_rotation_rad(angle: f32) -> Self {
        Self::new([0.0, 0.0], angle, 1.0)
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
        self.0
            .isometry
            .append_translation_mut(&Translation2::from(amount));
    }

    pub fn set_position(&mut self, pos: Vector2<f32>) {
        self.0.isometry.translation = nalgebra::Translation2::from(pos);
    }

    pub fn rotate_rad(&mut self, angle: f32) {
        self.0
            .isometry
            .append_rotation_mut(&UnitComplex::new(angle));
    }

    pub fn rotate_deg(&mut self, angle: f32) {
        self.rotate_rad(angle * PI / 180.0);
    }

    pub fn set_rotation_rad(&mut self, angle: f32) {
        self.0.isometry.rotation = UnitComplex::new(angle);
    }

    pub fn set_rotation_deg(&mut self, angle: f32) {
        self.set_rotation_rad(angle * PI / 180.0);
    }

    pub fn scale(&mut self, factor: f32) {
        self.0.append_scaling(factor);
    }

    pub fn set_scale(&mut self, s: f32) {
        self.0.set_scaling(s);
    }

    /// Maps the wrapped Similarity into the less sophisticated graphics::Matrix2d
    /// for rendering with piston-graphics.
    pub fn for_gfx(&self) -> Matrix2d {
        // Matrix2d == [[f32;3];2]
        let h = self.0.to_homogeneous().map(f64::from);
        [[h[0], h[3], h[6]], [h[1], h[4], h[7]]]
    }
}

impl std::str::FromStr for Transform {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut pos = [0.0, 0.0];
        let mut rot = 0.0;
        let mut scale = 1.0;
        for part in s.split('&') {
            let (sym, val) = part.split_at(1);
            match sym {
                "P" => pos = parse_point(val)?,
                "R" => rot = val.parse().map_err(|_| ())?,
                "S" => scale = val.parse().map_err(|_| ())?,
                _ => return Err(()),
            }
        }

        Ok(Transform::new(pos, rot, scale))
    }
}

/// Parses a string representing two comma-separated float values into a [f32;2]
fn parse_point(s: &str) -> Result<[f32; 2], ()> {
    let mut parts = s.split(',').map(|p| p.parse::<f32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(x)), Some(Ok(y))) => Ok([x, y]),
        _ => Err(()),
    }
}