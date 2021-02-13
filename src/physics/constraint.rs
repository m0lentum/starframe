//! Types of physical constraints.

use super::RigidBody;
use crate::{graph, math::uv};

/// A constraint restricts the relative motion of two bodies,
/// or the motion of a single body in the world.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub(crate) owner: graph::Node<RigidBody>,
    pub(crate) target: Option<graph::Node<RigidBody>>,
    pub(crate) compliance: f32,
    pub(crate) ty: ConstraintType,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ConstraintType {
    Distance {
        distance: f32,
        offsets: [uv::Vec2; 2],
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
    offsets: [uv::Vec2; 2],
    compliance: f32,
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
            offsets: [uv::Vec2::zero(); 2],
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
    pub fn with_origin(mut self, point: uv::Vec2) -> Self {
        self.offsets[0] = point;
        self
    }

    /// Set the origin point of the constraint on the target body
    /// relative to the center of mass,
    /// or in the world if the target is None.
    ///
    /// This has no effect on angular-only constraints.
    pub fn with_target_origin(mut self, point: uv::Vec2) -> Self {
        self.offsets[1] = point;
        self
    }

    /// Add compliance (inverse stiffness) to the constraint.
    /// This makes it behave like a spring instead of a hard limit.
    ///
    /// Units of compliance are m/N.
    pub fn with_compliance(mut self, compliance: f32) -> Self {
        self.compliance = compliance;
        self
    }

    /// Build a distance constraint.
    pub fn build_distance(self, distance: f32, dir: ConstraintLimit) -> Constraint {
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
