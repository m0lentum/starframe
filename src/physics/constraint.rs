//! Types of physical constraints.

use crate::{
    math::{left_normal, uv},
    physics::BodyKey,
    PhysicsPose,
};

/// A constraint restricts the relative motion of two bodies,
/// or the motion of a single body in the world.
///
/// [`ConstraintBuilder`][self::ConstraintBuilder] is the preferred
/// way to create these, but the fields are public to allow in-place editing
/// for advanced users.
#[derive(Clone, Debug)]
pub struct Constraint {
    /// Bodies that are acted on by this constraint.
    pub target: ConstraintTargets,
    /// Stiffness, or how "hard" the constraint is.
    ///   is completely hard, lower values allow some flexing.
    ///
    /// This is the inverse of _compliance_, which can be easier to think about
    /// (zero compliance is a hard constraint, any nonzero amount allows some flexing).
    pub stiffness: f64,
    /// Damping coefficient for linear velocity.
    pub linear_damping: f64,
    /// Damping coefficient for angular velocity.
    pub angular_damping: f64,
    /// Minimum and maximum force to apply.
    /// Usually either (-, ) for an equality constraint
    /// or (0, ) for a one-sided constraint.
    pub limits: (f64, f64),
    /// Type of the constraint.
    pub ty: ConstraintType,
    /// Whether or not this constraint can be set to sleep when at rest.
    ///
    /// Most of the time this is safe to leave on.
    /// However, if the constraint is modified at runtime while sleeping,
    /// that will not wake it up.
    pub can_sleep: bool,
}

/// Which Bodies a Constraint affects.
#[derive(Clone, Debug)]
pub enum ConstraintTargets {
    Single(BodyKey),
    Pair(BodyKey, BodyKey),
    Multiple(Vec<BodyKey>),
}

impl ConstraintTargets {
    /// Add another body to the constraint.
    pub fn push(&mut self, body: BodyKey) {
        match self {
            Self::Single(b) => {
                *self = Self::Pair(*b, body);
            }
            Self::Pair(b1, b2) => {
                *self = Self::Multiple(vec![*b1, *b2, body]);
            }
            Self::Multiple(bodies) => {
                bodies.push(body);
            }
        }
    }

    /// Iterate over the bodies participating in the constraint.
    pub fn iter(&self) -> ConstraintTargetIter<'_> {
        ConstraintTargetIter {
            next_idx: 0,
            targets: self,
        }
    }
}

/// Iterator over the bodies affected by a constraint.
pub struct ConstraintTargetIter<'a> {
    next_idx: usize,
    targets: &'a ConstraintTargets,
}

impl<'a> Iterator for ConstraintTargetIter<'a> {
    type Item = BodyKey;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = match self.targets {
            ConstraintTargets::Single(b) => {
                if self.next_idx == 0 {
                    Some(*b)
                } else {
                    None
                }
            }
            ConstraintTargets::Pair(b1, b2) => {
                if self.next_idx <= 1 {
                    Some([*b1, *b2][self.next_idx])
                } else {
                    None
                }
            }
            ConstraintTargets::Multiple(bs) => {
                if self.next_idx < bs.len() {
                    Some(bs[self.next_idx])
                } else {
                    None
                }
            }
        };
        self.next_idx += 1;
        ret
    }
}

/// Function that determines the behavior of a constraint.
#[derive(Clone, Copy, Debug)]
pub enum ConstraintType {
    /// An attachment constraint is a special case of a distance constraint with distance zero,
    /// attempting to make the given points overlap.
    Attachment { offsets: [uv::DVec2; 2] },
    /// A distance constraint enforces a specific distance between two points.
    Distance {
        /// The desired distance.
        distance: f64,
        /// Offsets from each body's center
        /// for the points that are to be kept `distance` apart.
        offsets: [uv::DVec2; 2],
    },
}

/// A builder for constructing constraints.
#[derive(Clone, Debug)]
pub struct ConstraintBuilder {
    target: ConstraintTargets,
    limits: (f64, f64),
    stiffness: f64,
    linear_damping: f64,
    angular_damping: f64,
    can_sleep: bool,
}

impl ConstraintBuilder {
    /// Start building a constraint.
    ///
    /// At least one body must be given.
    /// You can connect the constraint to one or more other bodies with `add_body`
    /// (although most constraints only support at most two bodies).
    pub fn new(target: BodyKey) -> Self {
        Self {
            target: ConstraintTargets::Single(target),
            limits: (f64::NEG_INFINITY, f64::INFINITY),
            stiffness: f64::INFINITY,
            linear_damping: 0.1,
            angular_damping: 0.0,
            can_sleep: true,
        }
    }

    /// Attach the constraint to another body.
    pub fn add_body(mut self, target: BodyKey) -> Self {
        self.target.push(target);
        self
    }

    /// Set the compliance (inverse stiffness) of the constraint.
    /// This makes it behave like a spring instead of a hard limit.
    ///
    /// Units of compliance are m/N.
    pub fn with_compliance(mut self, compliance: f64) -> Self {
        self.stiffness = if compliance == 0. {
            f64::INFINITY
        } else {
            1. / compliance
        };
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

    /// Set the lower force limit for the constraint.
    pub fn with_limit_min(mut self, limit: f64) -> Self {
        self.limits.0 = limit;
        self
    }

    /// Set the higher force limit for the constraint.
    pub fn with_limit_max(mut self, limit: f64) -> Self {
        self.limits.1 = limit;
        self
    }

    /// Don't allow this constraint to be set to sleep when at rest.
    ///
    /// Generally you shouldn't do this unless you intend to modify the constraint at runtime.
    pub fn disable_sleeping(mut self) -> Self {
        self.can_sleep = false;
        self
    }

    /// Select the type of constraint and finish building.
    pub fn build(self, ty: ConstraintType) -> Constraint {
        Constraint {
            target: self.target,
            stiffness: self.stiffness,
            linear_damping: self.linear_damping,
            angular_damping: self.angular_damping,
            limits: self.limits,
            can_sleep: self.can_sleep,
            ty,
        }
    }
}

/// Value, gradient, and Hessian of a constraint
/// with respect to an individual body
/// at a given point in state space.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstraintDerivatives {
    pub value: f64,
    pub gradient: uv::DVec3,
    pub hessian: uv::DMat3,
}

impl Constraint {
    // TODO: there's some annoying duplication here, this can probably be done cleaner

    /// Compute the value of the constraint.
    ///
    /// Poses must be given in an order that matches the order of bodies in the constraint target.
    /// This one doesn't need the body index, since we're not computing derivatives.
    pub(crate) fn compute_value(&self, poses: &[PhysicsPose]) -> f64 {
        match self.ty {
            ConstraintType::Attachment { offsets } | ConstraintType::Distance { offsets, .. } => {
                let target_dist = match self.ty {
                    ConstraintType::Attachment { .. } => 0.,
                    ConstraintType::Distance { distance, .. } => distance,
                };
                let my_pose = poses[0];
                let their_pose = match self.target {
                    ConstraintTargets::Single(_) => PhysicsPose::default(),
                    _ => poses[1],
                };
                let points_world = [my_pose * offsets[0], their_pose * offsets[1]];
                let dist = points_world[1] - points_world[0];
                let l = dist.mag();

                target_dist - l
            }
        }
    }

    /// Compute the derivatives of the constraint for one body.
    ///
    /// Poses must be given in an order that matches the order of bodies in the constraint target.
    /// The body_idx parameter is the index in that order of the body we're computing the constraints for.
    pub(crate) fn compute_derivatives(
        &self,
        poses: &[PhysicsPose],
        body_idx: usize,
    ) -> ConstraintDerivatives {
        match self.ty {
            ConstraintType::Attachment { offsets } | ConstraintType::Distance { offsets, .. } => {
                let target_dist = match self.ty {
                    ConstraintType::Attachment { .. } => 0.,
                    ConstraintType::Distance { distance, .. } => distance,
                };
                let poses = [
                    poses[0],
                    match self.target {
                        ConstraintTargets::Single(_) => PhysicsPose::default(),
                        _ => poses[1],
                    },
                ];
                let points_world = [poses[0] * offsets[0], poses[1] * offsets[1]];
                let dist = points_world[(body_idx + 1) % 2] - points_world[body_idx];
                let l = dist.mag();

                let value = target_dist - l;

                let l_squared = l * l;
                let l_cubed = l * l * l;

                // tangential direction that rotation moves the point in,
                // part of the angular derivative of the constraint
                let offset_rotated = poses[body_idx].rotation * offsets[body_idx];
                let rotating_dir = left_normal(offset_rotated);
                let dist_unit = dist / l;
                let angular_grad = rotating_dir.dot(dist_unit);

                let gradient = uv::DVec3::new(dist_unit.x, dist_unit.y, angular_grad);

                let h_00 = (dist.x * dist.x / l_cubed) - (1. / l);
                let h_01 = dist.x * dist.y / l_cubed;
                let h_11 = (dist.y * dist.y / l_cubed) - (1. / l);
                let h_02 =
                    (-offset_rotated.y * l - dist.x * rotating_dir.dot(dist_unit)) / l_squared;
                let h_12 =
                    (offset_rotated.x * l - dist.y * rotating_dir.dot(dist_unit)) / l_squared;
                let h_22 = (l * (-offset_rotated.dot(dist) - offset_rotated.mag_sq())
                    + rotating_dir.dot(dist_unit) * offset_rotated.dot(dist_unit))
                    / l_squared;
                let hessian =
                    uv::DMat3::from([[h_00, h_01, h_02], [h_01, h_11, h_12], [h_02, h_12, h_22]]);

                ConstraintDerivatives {
                    value,
                    gradient,
                    hessian,
                }
            }
        }
    }
}
