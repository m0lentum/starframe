//! Types of physical constraints.

use super::RigidBody;
use crate::{graph, math as m};

/// A constraint restricts the relative motion of two bodies,
/// or the motion of a single body in the world.
///
/// [`ConstraintBuilder`][self::ConstraintBuilder] is the preferred
/// way to create these, but the fields are public to allow in-place editing
/// for advanced users.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    /// The body that owns this constraint.
    pub owner: graph::Node<RigidBody>,
    /// The body this constraint is attached to.
    /// `None` represents ground.
    pub target: Option<graph::Node<RigidBody>>,
    /// Inverse of stiffness, or how much the constraint resists violation.
    pub compliance: f64,
    /// Type of the constraint.
    pub ty: ConstraintType,
}

/// Type-specific variables for constraints.
#[derive(Clone, Copy, Debug)]
pub enum ConstraintType {
    /// A distance constraint enforces a specific distance between two points.
    Distance {
        /// The desired distance.
        distance: f64,
        /// Offsets from each body's center of mass (or world origin).
        offsets: [m::Vec2; 2],
        /// Which directions to enforce the constraint in.
        limit: ConstraintLimit,
    },
}

/// Some constraints can be set to only work in one direction,
/// to e.g. set a maximum distance while allowing shorter distances.
#[derive(Clone, Copy, Debug)]
pub enum ConstraintLimit {
    /// Always apply a correction to the constraint.
    Eq,
    /// Only apply a correction if the constraint value is less than the target.
    Lt,
    /// Only apply a correction if the constraint value is greater than the target.
    Gt,
}

/// A builder that allows ergonomic construction of different constraints.
#[derive(Clone, Copy, Debug)]
pub struct ConstraintBuilder {
    owner: graph::Node<RigidBody>,
    target: Option<graph::Node<RigidBody>>,
    offsets: [m::Vec2; 2],
    compliance: f64,
}

impl ConstraintBuilder {
    /// Start building a constraint.
    ///
    /// An owning body is required.
    /// If you don't connect the constraint to another body with
    /// `with_target`, it will be connected to ground, i.e. the world origin.
    pub fn new(owner: graph::Node<RigidBody>) -> Self {
        Self {
            owner,
            target: None,
            offsets: [m::Vec2::zero(); 2],
            compliance: 0.0,
        }
    }

    /// Attach the constraint to another body.
    pub fn with_target(mut self, target: graph::Node<RigidBody>) -> Self {
        self.target = Some(target);
        self
    }

    /// Set the origin point of the constraint on the owning body
    /// relative to the center of mass.
    ///
    /// This has no effect on angular-only constraints.
    pub fn with_origin(mut self, point: m::Vec2) -> Self {
        self.offsets[0] = point;
        self
    }

    /// Set the origin point of the constraint on the target body
    /// relative to the center of mass,
    /// or in the world if the target is None.
    ///
    /// This has no effect on angular-only constraints.
    pub fn with_target_origin(mut self, point: m::Vec2) -> Self {
        self.offsets[1] = point;
        self
    }

    /// Add compliance (inverse stiffness) to the constraint.
    /// This makes it behave like a spring instead of a hard limit.
    ///
    /// Units of compliance are m/N.
    pub fn with_compliance(mut self, compliance: f64) -> Self {
        self.compliance = compliance;
        self
    }

    /// Build a distance constraint.
    pub fn build_distance(self, distance: f64, dir: ConstraintLimit) -> Constraint {
        let ty = ConstraintType::Distance {
            distance,
            offsets: self.offsets,
            limit: dir,
        };
        self.build(ty)
    }

    /// Build an attachment constraint, i.e. a distance constraint of zero,
    /// forcing the origin points to overlap.
    pub fn build_attachment(self) -> Constraint {
        let ty = ConstraintType::Distance {
            distance: 0.0,
            offsets: self.offsets,
            limit: ConstraintLimit::Eq,
        };
        self.build(ty)
    }

    fn build(self, ty: ConstraintType) -> Constraint {
        Constraint {
            owner: self.owner,
            target: self.target,
            compliance: self.compliance,
            ty,
        }
    }
}
