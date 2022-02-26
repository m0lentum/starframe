use super::{
    shape_shape::{ClosestBoundaryPoint, Edge, PolygonEdge, SupportingEdge},
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

    /// Set the collider to be solid with the given surface material.
    #[inline]
    pub fn with_material(mut self, mat: Material) -> Self {
        self.ty = ColliderType::Solid(mat);
        self
    }

    /// Turn the collider into a trigger.
    #[inline]
    pub fn trigger(mut self) -> Self {
        self.ty = ColliderType::Trigger;
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
    pub fn is_trigger(&self) -> bool {
        matches!(self.ty, ColliderType::Trigger)
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
        if r == 0.0 {
            self.polygon.area()
        } else {
            use std::f64::consts::PI;
            // area of a circle-convex-polygon sum:
            // the polygon itself
            // plus exactly one circle (sum of all corners),
            // plus an extra polygon with height r for each face of the polygon
            self.polygon.area() + PI * r * r + self.polygon.side_length_sum() * r
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
            // TODO: work out all of these with circle component added
            ColliderPolygon::LineSegment { hl } => (hl * hl + r * r) / 3.0,
            ColliderPolygon::Rect { hw, hh } => (hw * hw + hh * hh) / 3.0,
            // quick approximation as a circle for now
            ColliderPolygon::Triangle { outer_r } | ColliderPolygon::Hexagon { outer_r } => {
                (outer_r + r) / 2.0
            }
        }
    }

    #[inline]
    pub fn bounding_sphere_r(&self) -> f64 {
        self.polygon.bounding_sphere_r() + self.circle_r
    }

    pub fn aabb(&self, pose: m::Pose) -> AABB {
        self.polygon
            .aabb(pose.rotation)
            .padded(self.circle_r)
            .translated(pose.translation)
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
/// Dimensions are stored "halved",
/// as distances from the origin to the edge.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
pub enum ColliderPolygon {
    // With nonzero `circle_r`, this is a circle.
    // With zero r it doesn't make much sense.
    Point,
    // With nonzero `circle_r` this is a capsule.
    // It may or may not make sense with zero r.
    LineSegment {
        /// half-length
        hl: f64,
    },
    // A rectangle.
    Rect {
        /// half-width
        hw: f64,
        /// half-height
        hh: f64,
    },
    /// An equilateral triangle.
    ///
    /// The bottom edge is parallel to the x-axis.
    Triangle {
        /// Radius of the circumscribed circle of the triangle,
        /// i.e. distance to its points from the center
        outer_r: f64,
    },
    /// A regular hexagon.
    ///
    /// The bottom and top edges are parallel to the x-axis.
    Hexagon {
        /// Distance from the center to the points of the hexagon
        outer_r: f64,
    },
}

//
// shape-specific utilities and axis constants
//

// consts for axes pointing at different non-axis-aligned angles
// (naming convention: angle in degrees CCW from positive x-axis)
// trig functions aren't const so I have to precompute these
const FRAC_PI_6_COS: f64 = 0.866025403784;
const FRAC_PI_6_SIN: f64 = 0.5;
const FRAC_PI_6_TAN: f64 = 0.57735026919;
const FRAC_PI_3_COS: f64 = FRAC_PI_6_SIN;
const FRAC_PI_3_SIN: f64 = FRAC_PI_6_COS;
const AXIS_30_DEG: m::Unit<m::Vec2> =
    m::Unit::new_unchecked(m::Vec2::new(FRAC_PI_6_COS, FRAC_PI_6_SIN));
const AXIS_60_DEG: m::Unit<m::Vec2> =
    m::Unit::new_unchecked(m::Vec2::new(FRAC_PI_3_COS, FRAC_PI_3_SIN));
// these are left normals of 30 and 60 but normal can't be const computed because of negation
const AXIS_120_DEG: m::Unit<m::Vec2> =
    m::Unit::new_unchecked(m::Vec2::new(-FRAC_PI_6_SIN, FRAC_PI_6_COS));
const AXIS_150_DEG: m::Unit<m::Vec2> =
    m::Unit::new_unchecked(m::Vec2::new(-FRAC_PI_3_SIN, FRAC_PI_3_COS));

impl ColliderPolygon {
    //
    // physics properties
    //

    fn area(&self) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { .. } => 0.0,
            Self::Rect { hw, hh } => 4.0 * hw * hh,
            Self::Triangle { outer_r } => 3.0 * 0.25 * outer_r * outer_r / FRAC_PI_6_TAN,
            Self::Hexagon { outer_r } => 3.0 * outer_r * FRAC_PI_6_COS * outer_r,
        }
    }

    fn side_length_sum(&self) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { hl } => 4.0 * hl, // counts in both directions
            Self::Rect { hw, hh } => 4.0 * (hw + hh),
            Self::Triangle { outer_r } => 3.0 * outer_r / FRAC_PI_6_TAN,
            Self::Hexagon { outer_r } => 6.0 * outer_r,
        }
    }

    //
    // collision detection
    //

    fn bounding_sphere_r(&self) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { hl } => hl,
            Self::Rect { hw, hh } => (hw * hw + hh * hh).sqrt(),
            Self::Triangle { outer_r } => outer_r,
            Self::Hexagon { outer_r } => outer_r,
        }
    }

    fn aabb(&self, rotation: m::Rotor2) -> AABB {
        let symmetric_extent = match *self {
            Self::Point => m::Vec2::zero(),
            Self::LineSegment { hl } => (rotation * m::Vec2::new(hl, 0.0)).abs(),
            Self::Rect { hw, hh } => {
                (rotation * m::Vec2::new(hw, 0.0)).abs() + (rotation * m::Vec2::new(0.0, hh)).abs()
            }
            // probably not worth the computation time of all the trigonometry
            // to get the exact smallest aabb
            Self::Triangle { outer_r } | Self::Hexagon { outer_r } => {
                m::Vec2::new(outer_r, outer_r)
            }
        };
        AABB {
            min: -symmetric_extent,
            max: symmetric_extent,
        }
    }

    /// Get the closest point to a point on the exterior edge of the polygon,
    /// plus whether or not the queried point is inside the polygon.
    ///
    /// Used in the special case of circle - other shape collisions.
    pub(super) fn closest_boundary_point(&self, pt: m::Vec2) -> ClosestBoundaryPoint {
        match *self {
            Self::Point => ClosestBoundaryPoint {
                pt: m::Vec2::zero(),
                is_interior: false,
            },
            Self::LineSegment { hl } => ClosestBoundaryPoint {
                pt: m::Vec2::new(pt.x.max(-hl).min(hl), 0.0),
                is_interior: false,
            },
            Self::Rect { hw, hh } => {
                let x_dist = pt.x.abs() - hw;
                let y_dist = pt.y.abs() - hh;
                match (x_dist > 0.0, y_dist > 0.0) {
                    // we're outside the whole rect and closest point is a corner
                    (true, true) => ClosestBoundaryPoint {
                        pt: m::Vec2::new(hw.copysign(pt.x), hh.copysign(pt.y)),
                        is_interior: false,
                    },
                    // outside only on the x-axis
                    (true, false) => ClosestBoundaryPoint {
                        pt: m::Vec2::new(hw.copysign(pt.x), pt.y),
                        is_interior: false,
                    },
                    // outside only on the y-axis
                    (false, true) => ClosestBoundaryPoint {
                        pt: m::Vec2::new(pt.x, hh.copysign(pt.y)),
                        is_interior: false,
                    },
                    // inside
                    (false, false) => ClosestBoundaryPoint {
                        pt: if x_dist.abs() < y_dist.abs() {
                            m::Vec2::new(hw.copysign(pt.x), pt.y)
                        } else {
                            m::Vec2::new(pt.x, hh.copysign(pt.y))
                        },
                        is_interior: true,
                    },
                }
            }
            // the following works for any shape using just edge information
            Self::Triangle { .. } | Self::Hexagon { .. } => {
                // negative distance used to find the closest edge if we're on the inside
                let mut closest_edge_dist = f64::MIN;
                let mut closest_edge_normal = m::Unit::unit_x();
                // the closest vertex to return if we're in a vertex Voronoi region on the outside
                let mut closest_vertex: Option<m::Vec2> = None;
                for edge in (0..self.edge_count()).map(|i| self.get_edge(i)) {
                    let dist_towards_edge = edge.normal.dot(pt);
                    let (edge, dist_towards_edge) =
                        if dist_towards_edge < 0.0 && self.is_symmetrical() {
                            (edge.mirrored(), -dist_towards_edge)
                        } else {
                            (edge, dist_towards_edge)
                        };
                    let dist_from_edge = dist_towards_edge - edge.extent;
                    if dist_from_edge >= 0.0 {
                        // outside of the shape, either along this edge
                        // or in one of the adjacent vertex Voronoi regions
                        let edge_t_to_pt = edge.edge.dir.dot(pt - edge.edge.start);
                        if edge_t_to_pt >= 0.0 && edge_t_to_pt <= edge.edge.length {
                            // we're within the edge Voronoi region, can return immediately.
                            // otherwise we might be in a corner region but don't know for sure yet
                            return ClosestBoundaryPoint {
                                pt: edge.edge.start + edge_t_to_pt * *edge.edge.dir,
                                is_interior: false,
                            };
                        } else if let Some(pt) = closest_vertex {
                            // we're outside two edges,
                            // at this point we know for sure we're closest to a vertex
                            return ClosestBoundaryPoint {
                                pt,
                                is_interior: false,
                            };
                        } else {
                            // we're outside one edge. Wait until another similar edge is found
                            // (or none is) to see if closest point is on that edge or on the
                            // vertex between
                            closest_vertex = Some(
                                edge.edge.start
                                    + edge_t_to_pt.max(0.0).min(edge.edge.length) * *edge.edge.dir,
                            );
                        }
                    }
                    // dist_from_edge is negative, point is on its inside
                    if dist_from_edge > closest_edge_dist {
                        closest_edge_dist = dist_from_edge;
                        closest_edge_normal = edge.normal;
                    }
                }

                match closest_vertex {
                    Some(pt) => ClosestBoundaryPoint {
                        pt,
                        is_interior: false,
                    },
                    None => ClosestBoundaryPoint {
                        pt: pt - closest_edge_dist * *closest_edge_normal,
                        is_interior: true,
                    },
                }
            }
        }
    }

    /// Whether or not the shape has mirror symmetry with respect to the origin point.
    /// If true, we can only return half the edges and work with their mirror images.
    pub(crate) fn is_symmetrical(&self) -> bool {
        match *self {
            Self::Point | Self::LineSegment { .. } | Self::Rect { .. } | Self::Hexagon { .. } => {
                true
            }
            Self::Triangle { .. } => false,
        }
    }

    /// Poor man's generator by iterating indices and returning edges by matching on them
    /// so we don't need to allocate to iterate edges
    pub(crate) fn edge_count(&self) -> usize {
        match *self {
            Self::Point => 0,
            Self::LineSegment { .. } => 1,
            Self::Rect { .. } => 2,
            Self::Triangle { .. } => 3,
            Self::Hexagon { .. } => 3,
        }
    }

    /// Get all potential separating axes of the polygon.
    ///
    /// These must come in counterclockwise order and include every vertex,
    /// in order to facilitate automatic mesh generation.
    pub(crate) fn get_edge(&self, idx: usize) -> PolygonEdge {
        let bad_edge = || {
            panic!(
                "Called get_edge for {:?} with an out of bounds index {}",
                self, idx
            )
        };
        match *self {
            Self::Point => bad_edge(),
            Self::LineSegment { hl } => PolygonEdge {
                normal: m::Unit::unit_y(),
                extent: 0.0,
                edge: Edge {
                    start: m::Vec2::new(hl, 0.0),
                    dir: -m::Unit::unit_x(),
                    length: 2.0 * hl,
                },
            },
            Self::Rect { hw, hh } => match idx {
                0 => PolygonEdge {
                    normal: m::Unit::unit_x(),
                    extent: hw,
                    edge: Edge {
                        start: m::Vec2::new(hw, -hh),
                        dir: m::Unit::unit_y(),
                        length: 2.0 * hh,
                    },
                },
                1 => PolygonEdge {
                    normal: m::Unit::unit_y(),
                    extent: hh,
                    edge: Edge {
                        start: m::Vec2::new(hw, hh),
                        dir: -m::Unit::unit_x(),
                        length: 2.0 * hw,
                    },
                },
                _ => bad_edge(),
            },
            Self::Triangle { outer_r } => match idx {
                0 => PolygonEdge {
                    normal: -m::Unit::unit_y(),
                    // distance to the endpoints of an equilateral triangle
                    // is double the radius of the inscribed circle
                    extent: outer_r / 2.0,
                    edge: Edge {
                        start: -outer_r * *AXIS_30_DEG,
                        dir: m::Unit::unit_x(),
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                1 => PolygonEdge {
                    normal: AXIS_30_DEG,
                    extent: outer_r / 2.0,
                    edge: Edge {
                        start: -outer_r * *AXIS_150_DEG,
                        dir: AXIS_120_DEG,
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                2 => PolygonEdge {
                    normal: AXIS_150_DEG,
                    extent: outer_r / 2.0,
                    edge: Edge {
                        start: m::Vec2::new(0.0, outer_r),
                        dir: -AXIS_60_DEG,
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                _ => bad_edge(),
            },
            Self::Hexagon { outer_r } => match idx {
                0 => PolygonEdge {
                    normal: AXIS_30_DEG,
                    // TODO: all symmetrical shapes have the same extent in every direction.
                    // possibly put this in another place so we can cache it instead of
                    // computing it for every axis
                    extent: FRAC_PI_6_COS * outer_r,
                    edge: Edge {
                        start: m::Vec2::new(outer_r, 0.0),
                        dir: AXIS_120_DEG,
                        length: outer_r,
                    },
                },
                1 => PolygonEdge {
                    normal: m::Unit::unit_y(),
                    extent: FRAC_PI_6_COS * outer_r,
                    edge: Edge {
                        start: outer_r * *AXIS_60_DEG,
                        dir: -m::Unit::unit_x(),
                        length: outer_r,
                    },
                },
                2 => PolygonEdge {
                    normal: AXIS_150_DEG,
                    extent: FRAC_PI_6_COS * outer_r,
                    edge: Edge {
                        start: outer_r * *AXIS_120_DEG,
                        dir: -AXIS_60_DEG,
                        length: outer_r,
                    },
                },
                _ => bad_edge(),
            },
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
            Self::Triangle { outer_r } => {
                [m::Unit::unit_y(), -AXIS_30_DEG, -AXIS_150_DEG]
                    .into_iter()
                    .map(|dir_to_vertex| dir_to_vertex.dot(*dir))
                    .max_by(|p0, p1| p0.partial_cmp(p1).unwrap())
                    .unwrap()
                    * outer_r
            }
            Self::Hexagon { outer_r } => {
                [m::Unit::unit_x(), AXIS_60_DEG, AXIS_120_DEG]
                    .into_iter()
                    .map(|dir_to_vertex| dir_to_vertex.dot(*dir).abs())
                    .max_by(|p0, p1| p0.partial_cmp(p1).unwrap())
                    .unwrap()
                    * outer_r
            }
        }
    }

    /// Get the edge that is closest to the given direction,
    /// starting from the supporting point in that direction.
    ///
    /// `dir` must be given in object-local space but does not need to be
    /// normalized (note to self: DO NOT USE THE VALUE OF `dir * thing`, only compare).
    /// Returns None only if the shape is Point.
    pub(super) fn supporting_edge(&self, dir: m::Vec2) -> SupportingEdge {
        match *self {
            Self::Point => panic!("Don't call supporting_edge on a point"),
            Self::LineSegment { hl } => SupportingEdge {
                edge: Edge {
                    start: m::Vec2::new(hl.copysign(dir.x), 0.0),
                    dir: m::Unit::new_unchecked(m::Vec2::new(1_f64.copysign(-dir.x), 0.0)),
                    length: 2.0 * hl,
                },
                normal: m::Unit::new_unchecked(m::Vec2::new(0.0, 1_f64.copysign(dir.y))),
            },
            Self::Rect { hw, hh } => {
                let start = m::Vec2::new(hw.copysign(dir.x), hh.copysign(dir.y));
                if dir.x.abs() > dir.y.abs() {
                    SupportingEdge {
                        edge: Edge {
                            start,
                            dir: m::Unit::new_unchecked(m::Vec2::new(
                                0.0,
                                -(1_f64.copysign(dir.y)),
                            )),
                            length: hh * 2.0,
                        },
                        normal: m::Unit::new_unchecked(m::Vec2::new(1_f64.copysign(dir.x), 0.0)),
                    }
                } else {
                    SupportingEdge {
                        edge: Edge {
                            start,
                            dir: m::Unit::new_unchecked(m::Vec2::new(
                                -(1_f64.copysign(dir.x)),
                                0.0,
                            )),
                            length: hw * 2.0,
                        },
                        normal: m::Unit::new_unchecked(m::Vec2::new(0.0, 1_f64.copysign(dir.y))),
                    }
                }
            }
            Self::Triangle { .. } | Self::Hexagon { .. } => {
                let closest_edge = (0..self.edge_count())
                    .map(|i| {
                        let edge = self.get_edge(i);
                        if self.is_symmetrical() && edge.normal.dot(dir) < 0.0 {
                            edge.mirrored()
                        } else {
                            edge
                        }
                    })
                    .max_by(|e0, e1| e0.normal.dot(dir).partial_cmp(&e1.normal.dot(dir)).unwrap())
                    .unwrap();

                SupportingEdge {
                    edge: if closest_edge.edge.dir.dot(dir) < 0.0 {
                        closest_edge.edge
                    } else {
                        closest_edge.edge.flipped()
                    },
                    normal: closest_edge.normal,
                }
            }
        }
    }

    /// Tangent of half of the angle between edges is needed to compute the edges
    /// of the outer polygon from the inner polygon.
    ///
    /// For now we only have regular polygons and can get away with returning
    /// a constant with no parameters. If I want general polygons I'll have to think
    /// about how to associate a vertex and an edge
    pub(super) fn half_angle_between_edges_tan(&self) -> f64 {
        match *self {
            Self::Point | Self::LineSegment { .. } => {
                panic!("Angle between edges shouldn't be called for points or line segments")
            }
            Self::Rect { .. } => 1.0,
            // tan(pi / 3)
            Self::Triangle { .. } => 1.73205080757,
            // tan(2 * pi / 3)
            Self::Hexagon { .. } => -1.73205080757,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_POLYGONS: [ColliderPolygon; 3] = [
        ColliderPolygon::Rect { hw: 0.5, hh: 0.8 },
        ColliderPolygon::Triangle { outer_r: 1.0 },
        ColliderPolygon::Hexagon { outer_r: 1.0 },
    ];

    /// Closest boundary points are found correctly
    /// from every Voronoi region of every polygon shape
    #[test]
    fn closest_boundary_points() {
        for shape in TEST_POLYGONS {
            for edge in (0..shape.edge_count()).map(|i| shape.get_edge(i)).chain(
                // append mirrored edges if the shape is symmetrical
                if shape.is_symmetrical() {
                    0..shape.edge_count()
                } else {
                    0..0
                }
                .map(|i| shape.get_edge(i).mirrored()),
            ) {
                let assert_print_info = |cond: bool, pt: m::Vec2, cp: ClosestBoundaryPoint| {
                    assert!(
                        cond,
                        "shape {:?}\n\nedge {:?}\n\npoint {:?}\n\nclosest {:?}",
                        shape, edge, pt, cp
                    );
                };
                for t in [0.3, 0.5, 0.7] {
                    let pt_on = edge.edge.start + t * edge.edge.length * *edge.edge.dir;
                    let pt_in = pt_on - 0.05 * *edge.normal;
                    let cp_in = shape.closest_boundary_point(pt_in);
                    assert_print_info(
                        cp_in.is_interior
                            && (cp_in.pt - edge.edge.start).dot(*edge.normal).abs() < 0.0001
                            && (cp_in.pt - pt_in).dot(*edge.edge.dir).abs() < 0.0001,
                        pt_in,
                        cp_in,
                    );
                    let pt_out = pt_on + 0.05 * *edge.normal;
                    let cp_out = shape.closest_boundary_point(pt_out);
                    assert_print_info(!cp_out.is_interior, pt_out, cp_out);
                    assert_print_info(
                        !cp_out.is_interior
                            && (cp_out.pt - edge.edge.start).dot(*edge.normal).abs() < 0.0001
                            && (cp_out.pt - pt_out).dot(*edge.edge.dir).abs() < 0.0001,
                        pt_out,
                        cp_out,
                    );
                }
                let pt_before_out = edge.edge.start - 0.1 * *edge.edge.dir + 0.1 * *edge.normal;
                let cp_before_out = shape.closest_boundary_point(pt_before_out);
                assert_print_info(
                    !cp_before_out.is_interior
                        && (cp_before_out.pt - edge.edge.start).mag_sq() < 0.0001,
                    pt_before_out,
                    cp_before_out,
                );
                let edge_end = edge.edge.end_point();
                let pt_after_out = edge_end + 0.1 * *edge.edge.dir + 0.1 * *edge.normal;
                let cp_after_out = shape.closest_boundary_point(pt_after_out);
                assert_print_info(
                    !cp_after_out.is_interior && (cp_after_out.pt - edge_end).mag_sq() < 0.0001,
                    pt_after_out,
                    cp_after_out,
                );
            }
        }
    }

    #[test]
    fn supporting_edges_match_edge_list() {
        // go around in a circle and make sure supporting_edge always returns an edge
        // that is also returned by get_edge (and it's the closest one and oriented correctly)
        for shape in TEST_POLYGONS {
            for dir in sample_unit_circle(20) {
                let supp = shape.supporting_edge(*dir);
                let closest_edge = (0..shape.edge_count())
                    .map(|i| {
                        let edge = shape.get_edge(i);
                        if shape.is_symmetrical() && edge.normal.dot(*dir) < 0.0 {
                            edge.mirrored()
                        } else {
                            edge
                        }
                    })
                    .max_by(|e0, e1| {
                        e0.normal
                            .dot(*dir)
                            .partial_cmp(&e1.normal.dot(*dir))
                            .unwrap()
                    })
                    .unwrap();
                let closest_edge = if closest_edge.edge.dir.dot(*dir) < 0.0 {
                    closest_edge.edge
                } else {
                    closest_edge.edge.flipped()
                };

                let assert_print_info = |cond: bool| {
                    assert!(
                        cond,
                        "shape {:?}\n\ndir {:?}\n\nsupporting edge {:?}\n\nclosest edge {:?}",
                        shape, dir, supp, closest_edge
                    );
                };
                assert_print_info((closest_edge.start - supp.edge.start).mag_sq() < 0.0001);
                assert_print_info((*closest_edge.dir - *supp.edge.dir).mag_sq() < 0.0001);
                assert_print_info((closest_edge.length - supp.edge.length).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn projected_extent_matches_edge_list() {
        // go around in a circle again, this time checking that projected_extent
        // returns the distance of the farthest point in the edge list
        for shape in TEST_POLYGONS {
            for dir in sample_unit_circle(20) {
                let proj = shape.projected_extent(dir);
                let farthest_point_proj = (0..shape.edge_count())
                    .map(|i| {
                        let edge = shape.get_edge(i);
                        let point_proj = dir.dot(edge.edge.start);
                        if shape.is_symmetrical() && point_proj < 0.0 {
                            -point_proj
                        } else {
                            point_proj
                        }
                    })
                    .max_by(|p0, p1| p0.partial_cmp(p1).unwrap())
                    .unwrap();

                assert!(
                    (proj - farthest_point_proj).abs() < 0.0001,
                    "shape {:?}\n\ndir {:?}",
                    shape,
                    dir
                );
            }
        }
    }

    fn sample_unit_circle(sample_count: usize) -> impl Iterator<Item = m::Unit<m::Vec2>> {
        let angle_incr = std::f64::consts::TAU / sample_count as f64;
        (0..sample_count).map(move |i| {
            let angle = i as f64 * angle_incr;
            let (sin, cos) = angle.sin_cos();
            m::Unit::new_unchecked(m::Vec2::new(cos, sin))
        })
    }
}
