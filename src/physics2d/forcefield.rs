use ultraviolet as uv;

// TODO: this whole thing would be better expressed as a trait probably

/// Force fields are defined as functions from positions to force vectors
/// and applied during physics updates.
/// Many force fields can be combined by putting them in a Vec
/// and calling `from` or `into`.
pub struct ForceField {
    force: Box<dyn Fn(uv::Vec2) -> uv::Vec2>,
}

impl ForceField {
    /// Evaluate the force field at a given point in space.
    pub fn value_at(&self, point: uv::Vec2) -> uv::Vec2 {
        (self.force)(point)
    }

    /// Transform any function from a point to a vector to a force field.
    pub fn from_fn<F: Fn(uv::Vec2) -> uv::Vec2 + 'static>(f: F) -> Self {
        ForceField { force: Box::new(f) }
    }

    /// A constant force over all of space.
    pub fn gravity(f: uv::Vec2) -> Self {
        ForceField {
            force: Box::new(move |_| f),
        }
    }

    /// No force anywhere in space.
    pub fn none() -> Self {
        ForceField {
            force: Box::new(|_| uv::Vec2::zero()),
        }
    }
}

impl From<Vec<ForceField>> for ForceField {
    fn from(ff: Vec<ForceField>) -> Self {
        ForceField {
            force: Box::new(move |p| {
                let mut total = uv::Vec2::zero();
                for f in &ff {
                    total += (f.force)(p);
                }
                total
            }),
        }
    }
}
