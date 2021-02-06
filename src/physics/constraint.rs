//! Types of physical constraints and tools for creating and solving them.
//!
//! Major sources:
//! [Cat05] Catto, E. (2005). Iterative Dynamics With Temporal Coherence.
//!     https://www.gamedevs.org/uploads/iterative-dynamics-with-temporal-coherence.pdf
//! [Cat11] Catto, E. (2011). Soft Constraints.
//!     https://box2d.org/files/ErinCatto_SoftConstraints_GDC2011.pdf
//! [Tam15] Tamis, M. (2015). 3D Constraint Derivations for Impulse Solvers.
//!     http://www.mft-spirit.nl/files/MTamis_Constraints.pdf

pub(crate) mod func;
use func::ConstraintFunction;

//

use super::{RigidBody, Velocity};
use crate::{graph, math::uv};

/// A constraint restricts the relative motion of two bodies,
/// or the motion of a single body in the world.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub(crate) owner: graph::Node<RigidBody>,
    pub(crate) target: Option<graph::Node<RigidBody>>,
    pub(crate) impulse_bounds: (Option<f32>, Option<f32>),
    pub(crate) softness: Option<OscillatorParams>,
    pub(crate) func: ConstraintFunction,
}

/// Designer-friendly parameters for tuning soft constraints.
#[derive(Clone, Copy, Debug)]
pub struct OscillatorParams {
    /// Oscillations per second.
    pub frequency: f32,
    /// Damping ratio.
    ///
    /// If == 0, allows indefinite oscillation.  
    /// If < 1, decays to zero with some oscillation.  
    /// If == 1, decays smoothly to zero.  
    /// If > 1, decays smoothly to zero but slower.
    pub damping: f32,
}

/// A builder that allows ergonomic construction of different constraints.
#[derive(Clone, Copy, Debug)]
pub struct ConstraintBuilder {
    owner: graph::Node<RigidBody>,
    owner_origin: uv::Vec2,
    target: Option<graph::Node<RigidBody>>,
    target_origin: uv::Vec2,
    impulse_bounds: (Option<f32>, Option<f32>),
    softness: Option<OscillatorParams>,
}

impl ConstraintBuilder {
    /// Start building a constraint. An owning body is required.
    ///
    /// If you don't connect the constraint to another body with
    /// `with_target`, it will be connected to ground, i.e. the world origin.
    pub fn new(owner: graph::Node<RigidBody>) -> Self {
        Self {
            owner,
            owner_origin: uv::Vec2::zero(),
            target: None,
            target_origin: uv::Vec2::zero(),
            impulse_bounds: (None, None),
            softness: None,
        }
    }

    /// Attach the constraint to another body.
    pub fn with_target(mut self, target: graph::Node<RigidBody>) -> Self {
        self.target = Some(target);
        self
    }

    /// Set the origin point of the constraint on the owning body.
    ///
    /// Note that this does not have an effect on all constraint types,
    /// but it's so common it's included in the generic builder nonetheless.
    pub fn with_origin(mut self, point: uv::Vec2) -> Self {
        self.owner_origin = point;
        self
    }

    /// Set the origin point of the constraint on the target body,
    /// or in the world if the target is None.
    ///
    /// Note that this does not have an effect on all constraint types,
    /// but it's so common it's included in the generic builder nonetheless.
    pub fn with_target_origin(mut self, point: uv::Vec2) -> Self {
        self.target_origin = point;
        self
    }

    /// Allow constraint function values above zero.
    pub fn inequality_gt(mut self) -> Self {
        self.impulse_bounds.0 = Some(0.0);
        self
    }

    /// Allow constraint function values below zero.
    pub fn inequality_lt(mut self) -> Self {
        self.impulse_bounds.1 = Some(0.0);
        self
    }

    /// Make the constraint soft. This makes it equivalent to a spring.
    ///
    /// See [`SpringParams`](self::SpringParams) for details.
    pub fn soft(mut self, params: OscillatorParams) -> Self {
        self.softness = Some(params);
        self
    }

    /// Limit the maximum impulse of the constraint.
    /// This can be useful to e.g. limit the torque of a motor.
    pub fn with_max_impulse(mut self, max_impulse: f32) -> Self {
        let (lb, rb) = self.impulse_bounds;
        self.impulse_bounds = (
            lb.map(|lb| lb.max(-max_impulse)).or(Some(-max_impulse)),
            rb.map(|rb| rb.min(max_impulse)).or(Some(max_impulse)),
        );
        self
    }

    /// Build a distance constraint.
    pub fn build_distance(self, distance: f32) -> Constraint {
        let func = func::ConstraintFunction::Distance {
            distance_squared: distance * distance,
            offsets: [self.owner_origin, self.target_origin],
        };
        self.build(func)
    }

    fn build(self, func: ConstraintFunction) -> Constraint {
        Constraint {
            owner: self.owner,
            target: self.target,
            impulse_bounds: self.impulse_bounds,
            softness: self.softness,
            func,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Vec6 {
    pub(crate) v1: uv::Vec2,
    pub(crate) w1: f32,
    pub(crate) v2: uv::Vec2,
    pub(crate) w2: f32,
}

impl Vec6 {
    fn dot(&self, other: &Vec6) -> f32 {
        self.v1.dot(other.v1) + self.w1 * other.w1 + self.v2.dot(other.v2) + self.w2 * other.w2
    }

    /// Multiply the first three elements with a scalar and return the result as a Velocity.
    fn mul_first(&self, magnitude: f32) -> Velocity {
        Velocity {
            linear: self.v1 * magnitude,
            angular: self.w1 * magnitude,
        }
    }

    /// Multiply the last three elements with a scalar and return the result as a Velocity.
    fn mul_second(&self, magnitude: f32) -> Velocity {
        Velocity {
            linear: self.v2 * magnitude,
            angular: self.w2 * magnitude,
        }
    }
}
impl From<[Velocity; 2]> for Vec6 {
    fn from(v: [Velocity; 2]) -> Self {
        Self {
            v1: v[0].linear,
            w1: v[0].angular,
            v2: v[1].linear,
            w2: v[1].angular,
        }
    }
}
