use super::{
    shape_shape::{AxisIter, Edge, SeparatingAxis, SupportingEdge},
    AABB,
};
use crate::math as m;

/// A component that allows a game object to collide with others
/// or act as a trigger.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde-types", serde(default))]
pub struct Collider {
    pub shape: ColliderShape,
    pub ty: ColliderType,
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
    pub fn new_circle(radius: f64) -> Self {
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::Point,
                circle_r: radius,
            },
            ..Self::default()
        }
    }

    /// Create a solid rect collider with both sides set to the same length.
    pub fn new_square(side_length: f64) -> Self {
        Collider::new_rect(side_length, side_length)
    }

    /// Create a solid rect collider with two different side lengths.
    pub fn new_rect(width: f64, height: f64) -> Self {
        let hw = width / 2.0;
        let hh = height / 2.0;
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::Rect { hw, hh },
                circle_r: 0.0,
            },
            ..Self::default()
        }
    }

    /// Create a solid capsule collider (a rectangle with semicircles at the ends on the x-axis).
    pub fn new_capsule(length: f64, radius: f64) -> Self {
        Self {
            shape: ColliderShape {
                polygon: ColliderPolygon::LineSegment { hl: length / 2.0 },
                circle_r: radius,
            },
            ..Self::default()
        }
    }

    /// Set the collider to be solid with the given surface material.
    pub fn with_material(mut self, mat: Material) -> Self {
        self.ty = ColliderType::Solid(mat);
        self
    }

    /// Turn the collider into a trigger.
    pub fn trigger(mut self) -> Self {
        self.ty = ColliderType::Trigger;
        self
    }

    pub fn with_layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }

    #[inline]
    pub fn area(&self) -> f64 {
        self.shape.area()
    }

    #[inline]
    pub fn moment_of_inertia_coef(&self) -> f64 {
        self.shape.moment_of_inertia_coef()
    }

    #[inline]
    pub fn is_solid(&self) -> bool {
        matches!(self.ty, ColliderType::Solid(_))
    }

    #[inline]
    pub fn is_trigger(&self) -> bool {
        matches!(self.ty, ColliderType::Trigger)
    }

    #[inline]
    pub fn bounding_sphere_r(&self) -> f64 {
        self.shape.bounding_sphere_r()
    }

    #[inline]
    pub fn aabb(&self, pose: &m::Pose) -> AABB {
        self.shape.aabb(pose)
    }
}

/// The shape of a collider, expressed as the Minkowski sum of a
/// convex polygon (or point or line segment) and a circle.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub struct ColliderShape {
    pub polygon: ColliderPolygon,
    pub circle_r: f64,
}

impl From<ColliderPolygon> for ColliderShape {
    fn from(polygon: ColliderPolygon) -> Self {
        Self {
            polygon,
            circle_r: 0.0,
        }
    }
}

impl ColliderShape {
    #[inline]
    pub fn area(&self) -> f64 {
        let r = self.circle_r;
        // area of a circle-convex polygon sum:
        // the polygon itself
        // plus exactly one circle (sum of all corners),
        // plus an extra polygon with height r for each face of the polygon
        let circ_area = std::f64::consts::PI * r * r;
        match self.polygon {
            ColliderPolygon::Point => circ_area,
            ColliderPolygon::LineSegment { hl } => circ_area + (4.0 * hl * r),
            ColliderPolygon::Rect { hw, hh } => 4.0 * hw * hh,
        }
    }

    #[inline]
    pub fn moment_of_inertia_coef(&self) -> f64 {
        let r = self.circle_r;
        // from https://en.wikipedia.org/wiki/List_of_moments_of_inertia

        // TODO: how does the circular part change this?
        // maybe we can just add r to all dimensions and call it close enough
        match self.polygon {
            ColliderPolygon::Point => r * r / 2.0,
            // rough estimation of a capsule as a rectangle,
            // since an accurate formula is not on wikipedia.
            // TODO: calculate a formula by hand
            ColliderPolygon::LineSegment { hl } => (hl * hl + r * r) / 3.0,
            ColliderPolygon::Rect { hw, hh } => (hw * hw + hh * hh) / 3.0,
        }
    }

    #[inline]
    pub fn bounding_sphere_r(&self) -> f64 {
        let r = self.circle_r;
        match self.polygon {
            ColliderPolygon::Point => r,
            ColliderPolygon::LineSegment { hl } => hl + r,
            ColliderPolygon::Rect { hw, hh } => (hw * hw + hh * hh).sqrt(),
        }
    }

    pub fn aabb(&self, pose: &m::Pose) -> AABB {
        let r = self.circle_r;
        // for symmetrical shapes, the box is one vector mirrored both ways
        // (always plus r in both x and y)
        let extent = match self.polygon {
            ColliderPolygon::Point => m::Vec2::zero(),
            ColliderPolygon::LineSegment { hl } => (pose.rotation * m::Vec2::new(hl, 0.0)).abs(),
            ColliderPolygon::Rect { hw, hh } => {
                (pose.rotation * m::Vec2::new(hw, 0.0)).abs()
                    + (pose.rotation * m::Vec2::new(0.0, hh)).abs()
            }
        } + m::Vec2::new(r, r);
        AABB {
            min: pose.translation - extent,
            max: pose.translation + extent,
        }
    }

    /// Enlarge the circle component of the shape.
    pub fn expanded(&self, amount: f64) -> Self {
        Self {
            circle_r: self.circle_r + amount,
            polygon: self.polygon,
        }
    }
}

/// The polygonal part of a collider's shape.
///
/// For symmetrical shapes, dimensions are stored "halved",
/// as distances from the origin to the edge.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub enum ColliderPolygon {
    Point,
    LineSegment { hl: f64 },
    Rect { hw: f64, hh: f64 },
}

impl ColliderPolygon {
    //
    // internals for collision detection
    //

    /// Get all potential separating axes of the polygon.
    pub(super) fn separating_axes(&self) -> AxisIter {
        match *self {
            Self::Point => AxisIter::Zero,
            Self::LineSegment { hl } => AxisIter::One(
                [SeparatingAxis {
                    axis: m::Unit::unit_y(),
                    extent: 0.0,
                    edge: Edge {
                        start: m::Vec2::new(-hl, 0.0),
                        dir: m::Unit::unit_x(),
                        length: 2.0 * hl,
                    },
                    symmetrical: true,
                }]
                .into_iter(),
            ),
            Self::Rect { hw, hh } => AxisIter::Two(
                [
                    SeparatingAxis {
                        axis: m::Unit::unit_x(),
                        extent: hw,
                        edge: Edge {
                            start: m::Vec2::new(hw, -hh),
                            dir: m::Unit::unit_y(),
                            length: 2.0 * hh,
                        },
                        symmetrical: true,
                    },
                    SeparatingAxis {
                        axis: m::Unit::unit_y(),
                        extent: hh,
                        edge: Edge {
                            start: m::Vec2::new(-hw, hh),
                            dir: m::Unit::unit_x(),
                            length: 2.0 * hw,
                        },
                        symmetrical: true,
                    },
                ]
                .into_iter(),
            ),
        }
    }

    /// Get the distance from the shape's center to its farthest extent
    /// when projected onto the given axis.
    ///
    /// `dir` must be given in object-local space.
    pub(super) fn projected_extent(&self, dir: m::Unit<m::Vec2>) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { hl } => dir.x.abs() * hl,
            Self::Rect { hw, hh } => dir.x.abs() * hw + dir.y.abs() * hh,
        }
    }

    /// Get the edge that is closest to the given direction,
    /// starting from the supporting point in that direction.
    ///
    /// `dir` must be given in object-local space.
    /// Returns None only if the shape is Point.
    pub(super) fn supporting_edge(&self, dir: m::Unit<m::Vec2>) -> Option<SupportingEdge> {
        match *self {
            Self::Point => None,
            Self::LineSegment { hl } => Some(SupportingEdge {
                edge: Edge {
                    start: m::Vec2::new(hl.copysign(dir.x), 0.0),
                    dir: m::Unit::new_unchecked(m::Vec2::new(1_f64.copysign(-dir.x), 0.0)),
                    length: 2.0 * hl,
                },
                normal: m::Unit::new_unchecked(m::Vec2::new(0.0, 1_f64.copysign(dir.y))),
            }),
            Self::Rect { hw, hh } => {
                let start = m::Vec2::new(hw.copysign(dir.x), hh.copysign(dir.y));
                if dir.x.abs() > dir.y.abs() {
                    Some(SupportingEdge {
                        edge: Edge {
                            start,
                            dir: m::Unit::new_unchecked(m::Vec2::new(
                                0.0,
                                -(1_f64.copysign(dir.y)),
                            )),
                            length: hh * 2.0,
                        },
                        normal: m::Unit::new_unchecked(m::Vec2::new(1_f64.copysign(dir.x), 0.0)),
                    })
                } else {
                    Some(SupportingEdge {
                        edge: Edge {
                            start,
                            dir: m::Unit::new_unchecked(m::Vec2::new(
                                -(1_f64.copysign(dir.x)),
                                0.0,
                            )),
                            length: hw * 2.0,
                        },
                        normal: m::Unit::new_unchecked(m::Vec2::new(0.0, 1_f64.copysign(dir.y))),
                    })
                }
            }
        }
    }
}

/// Type of a collider. Solid ones respond to collisions when attached to bodies.
/// Triggers only cause an event to be sent.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub enum ColliderType {
    Solid(Material),
    Trigger,
}

impl Default for ColliderType {
    fn default() -> Self {
        Self::Solid(Material::default())
    }
}

/// Determines how the surface of a collider affects collisions.
///
/// Using a simplified friction model where each material has its own friction
/// coefficients (rather than the realistic model where every pair of materials
/// would have its own coefficients).
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde-types", serde(default))]
pub struct Material {
    /// Coefficient of static friction.
    /// Set to None to opt out of static friction.
    pub static_friction_coef: Option<f64>,
    /// Coefficient of dynamic friction.
    /// Set to None to opt out of dynamic friction.
    pub dynamic_friction_coef: Option<f64>,
    pub restitution_coef: f64,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            static_friction_coef: Some(1.6),
            dynamic_friction_coef: Some(1.5),
            restitution_coef: 0.0,
        }
    }
}

impl Material {
    /// Get the static friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn static_friction_with(&self, other: &Self) -> Option<f64> {
        match (self.static_friction_coef, other.static_friction_coef) {
            (Some(mine), Some(theirs)) => Some((mine + theirs) / 2.0),
            _ => None,
        }
    }

    /// Get the dynamic friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn dynamic_friction_with(&self, other: &Self) -> Option<f64> {
        match (self.dynamic_friction_coef, other.dynamic_friction_coef) {
            (Some(mine), Some(theirs)) => Some((mine + theirs) / 2.0),
            _ => None,
        }
    }

    /// Get the restitution coefficient between this material and another.
    ///
    /// It is computed as the largest coefficient between the two bodies.
    pub fn restitution_with(&self, other: &Self) -> f64 {
        self.restitution_coef.max(other.restitution_coef)
    }
}
