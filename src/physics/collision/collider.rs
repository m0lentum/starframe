use crate::{
    math::{self as m},
    physics::body,
};

mod constants;
mod shape;
pub use shape::{ColliderPolygon, ColliderShape};

/// A component that allows a game object to collide with others
/// or act as a sensor.
///
/// TODOC: compound colliders
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde-types", serde(default))]
pub struct Collider {
    pub shape: ColliderShape,
    pub ty: ColliderType,
    /// Pose of the collider relative to the body it's attached to,
    /// or the world if it's not attached to a body.
    #[serde(with = "m::serde_physics_pose")]
    pub pose: m::PhysicsPose,
    /// Collision layer, see [`MaskMatrix`][super::MaskMatrix] for info.
    /// Defaults to 0.
    pub layer: usize,
}
impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::Point,
                circle_r: 1.0,
            },
            ty: ColliderType::default(),
            pose: m::PhysicsPose::default(),
            layer: 0,
        }
    }
}
impl From<ColliderShape> for Collider {
    fn from(shape: ColliderShape) -> Self {
        Self {
            shape,
            ..Default::default()
        }
    }
}
impl From<ColliderPolygon> for Collider {
    fn from(polygon: ColliderPolygon) -> Self {
        Self {
            shape: ColliderShape::from(polygon),
            ..Default::default()
        }
    }
}

impl Collider {
    /// Create a solid circle collider from a radius.
    #[inline]
    pub fn new_circle(radius: f64) -> Self {
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::Point,
                circle_r: radius,
            },
            ..Self::default()
        }
    }

    /// Create a solid rectangle collider with both sides set to the same length.
    #[inline]
    pub fn new_square(side_length: f64) -> Self {
        Collider::new_rect(side_length, side_length)
    }

    /// Create a solid rectangle collider with two different side lengths.
    #[inline]
    pub fn new_rect(width: f64, height: f64) -> Self {
        Self::new_rounded_rect(width, height, 0.0)
    }

    /// Create a solid rectangle collider with rounded corners.
    ///
    /// Width and height are outer dimensions of the box, with corners cut out.
    /// Think `corner-radius` in CSS. If the radius is greater than there's
    /// actually room for, it's reduced until it fits.
    pub fn new_rounded_rect(width: f64, height: f64, radius: f64) -> Self {
        let hw = ((width / 2.0) - radius).max(0.05);
        let hh = ((height / 2.0) - radius).max(0.05);
        let circle_r = radius.min(width / 2.0).min(height / 2.0);
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::Rect { hw, hh },
                circle_r,
            },
            ..Self::default()
        }
    }

    /// Create a solid capsule collider (a rectangle with semicircles at the ends on the x-axis).
    #[inline]
    pub fn new_capsule(length: f64, radius: f64) -> Self {
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::LineSegment { hl: length / 2.0 },
                circle_r: radius,
            },
            ..Self::default()
        }
    }

    /// Create a solid regular polygon collider
    /// with the specified number of points
    /// that fits inside a circle of the given radius.
    #[inline]
    pub fn new_regular(points: usize, r: f64) -> Self {
        ColliderPolygon::Regular { points, r }.into()
    }

    /// Set the pose of the collider relative to the body it's attached to,
    /// or relative to the world if it's not attached to a body.
    #[inline]
    pub fn with_pose(mut self, pose: m::PhysicsPose) -> Self {
        self.pose = pose;
        self
    }

    /// Set the collider to be solid with the given surface material.
    #[inline]
    pub fn with_material(mut self, mat: PhysicsMaterial) -> Self {
        self.ty = ColliderType::Solid(mat);
        self
    }

    /// Turn the collider into a sensor
    /// (i.e. a collider that doesn't affect the movement of bodies,
    /// only reports when they intersect it).
    #[inline]
    pub fn sensor(mut self) -> Self {
        self.ty = ColliderType::Sensor;
        self
    }

    #[inline]
    pub fn with_layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }

    #[inline]
    pub fn is_solid(&self) -> bool {
        matches!(self.ty, ColliderType::Solid(_))
    }

    #[inline]
    pub fn is_sensor(&self) -> bool {
        matches!(self.ty, ColliderType::Sensor)
    }

    /// Get the info required to construct a body with this collider.
    #[inline]
    pub fn info(&self) -> body::ColliderInfo {
        body::ColliderInfo {
            area: self.shape.area(),
            second_moment_of_area: self.shape.second_moment_of_area(),
        }
    }
}

/// Type of a collider. Solid ones respond to collisions when attached to bodies.
/// Triggers only cause an event to be sent.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub enum ColliderType {
    Solid(PhysicsMaterial),
    Sensor,
}

impl Default for ColliderType {
    fn default() -> Self {
        Self::Solid(PhysicsMaterial::default())
    }
}

/// Determines how the surface of a collider affects collisions.
///
/// Using a simplified friction model where each material has its own friction
/// coefficient (rather than the realistic model where every pair of materials
/// would have its own coefficients).
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde-types", serde(default))]
pub struct PhysicsMaterial {
    /// Coefficient of (dynamic) friction.
    /// Range: 0..inf, default: 1.
    ///
    /// A higher value means more friction;
    /// set to zero for a completely frictionless material.
    pub friction_coef: f64,
    /// Coefficient of restitution.
    /// Range: 0..=1, default: 0.
    ///
    /// Determines how strongly the body bounces when colliding with something;
    /// a value of 1.0 means a completely elastic collision (preserves all speed).
    pub restitution_coef: f64,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        PhysicsMaterial {
            friction_coef: 1.,
            restitution_coef: 0.,
        }
    }
}

impl PhysicsMaterial {
    /// Preset for a material that doesn't experience friction and doesn't bounce.
    /// Commonly used for game characters.
    pub fn frictionless_unelastic() -> Self {
        Self {
            friction_coef: 0.,
            restitution_coef: 0.,
        }
    }

    /// Get the dynamic friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn friction_with(&self, other: &Self) -> f64 {
        // special case for frictionless bodies
        if self.friction_coef == 0. || other.friction_coef == 0. {
            0.
        } else {
            (self.friction_coef + other.friction_coef) / 2.
        }
    }

    /// Get the restitution coefficient between this material and another.
    ///
    /// It is computed as the largest coefficient between the two bodies.
    pub fn restitution_with(&self, other: &Self) -> f64 {
        self.restitution_coef.max(other.restitution_coef)
    }
}
