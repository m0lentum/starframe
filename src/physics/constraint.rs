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
    /// Damping coefficient for linear velocity.
    pub linear_damping: f64,
    /// Damping coefficient for angular velocity.
    pub angular_damping: f64,
    /// Offsets from each body's center of mass (or world origin).
    pub offsets: [m::Vec2; 2],
    /// Which directions to enforce the constraint in.
    pub limit: ConstraintLimit,
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
    limit: ConstraintLimit,
    compliance: f64,
    linear_damping: f64,
    angular_damping: f64,
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
            limit: ConstraintLimit::Eq,
            compliance: 0.0,
            linear_damping: 0.1,
            angular_damping: 0.0,
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

    /// Set the linear damping coefficient of the constraint. This will slow down
    /// the relative linear velocities of the participating bodies.
    pub fn with_linear_damping(mut self, damping: f64) -> Self {
        self.linear_damping = damping;
        self
    }

    /// Set the angular damping coefficient of the constraint. This will slow down
    /// the relative angular velocities of the participating bodies.
    pub fn with_angular_damping(mut self, damping: f64) -> Self {
        self.angular_damping = damping;
        self
    }

    /// Set the limit for when to enforce the constraint.
    pub fn with_limit(mut self, limit: ConstraintLimit) -> Self {
        self.limit = limit;
        self
    }

    /// Build a distance constraint.
    pub fn build_distance(self, distance: f64, dir: ConstraintLimit) -> Constraint {
        self.build(ConstraintType::Distance { distance })
    }

    /// Build an attachment constraint, i.e. a distance constraint of zero,
    /// forcing the origin points to overlap.
    pub fn build_attachment(self) -> Constraint {
        self.build(ConstraintType::Distance { distance: 0.0 })
    }

    fn build(self, ty: ConstraintType) -> Constraint {
        Constraint {
            owner: self.owner,
            target: self.target,
            compliance: self.compliance,
            linear_damping: self.linear_damping,
            angular_damping: self.angular_damping,
            offsets: self.offsets,
            limit: self.limit,
            ty,
        }
    }
}
