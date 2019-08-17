use nalgebra::{Point2, Vector2};

/// Force fields are defined as functions from positions to force vectors
/// and applied during physics updates.
/// Many force fields can be combined by putting them in a Vec
/// and calling `from` or `into`.
pub struct ForceField {
    force: Box<dyn Fn(Point2<f32>) -> Vector2<f32>>,
}

impl ForceField {
    /// Evaluate the force field at a given point in space.
    pub fn value_at(&self, point: Point2<f32>) -> Vector2<f32> {
        (self.force)(point)
    }

    /// Transform any function from a point to a vector to a force field.
    pub fn from_fn<F: Fn(Point2<f32>) -> Vector2<f32> + 'static>(f: F) -> Self {
        ForceField { force: Box::new(f) }
    }

    /// A constant force over all of space.
    pub fn gravity(f: Vector2<f32>) -> Self {
        ForceField {
            force: Box::new(move |_| f),
        }
    }
}

impl From<Vec<ForceField>> for ForceField {
    fn from(ff: Vec<ForceField>) -> Self {
        ForceField {
            force: Box::new(move |p| {
                let mut total = Vector2::zeros();
                for f in &ff {
                    total += (f.force)(p);
                }
                total
            }),
        }
    }
}
