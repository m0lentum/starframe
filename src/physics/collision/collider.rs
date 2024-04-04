use std::f64::consts::{FRAC_PI_2, PI};

use super::{
    shape_shape::{ClosestBoundaryPoint, Edge, PolygonEdge},
    AABB,
};
use crate::{
    math::{self as m, uv, UnitDVec2},
    physics::body,
};

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

    /// Create a solid hexagon collider that fits inside a circle of the given radius.
    #[inline]
    pub fn new_hexagon(outer_r: f64) -> Self {
        ColliderPolygon::Hexagon { outer_r }.into()
    }

    /// Create a solid triangle collider that fits inside a circle of the given radius.
    #[inline]
    pub fn new_triangle(outer_r: f64) -> Self {
        ColliderPolygon::Triangle { outer_r }.into()
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

/// The shape of a collider, expressed as the Minkowski sum of a
/// convex polygon (or point or line segment) and a circle.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde-types", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde-types", serde(default))]
pub struct ColliderShape {
    pub polygon: ColliderPolygon,
    pub circle_r: f64,
}

impl Default for ColliderShape {
    fn default() -> Self {
        Self {
            polygon: ColliderPolygon::Point,
            circle_r: 0.0,
        }
    }
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
            // area of a circle-convex-polygon sum:
            // the polygon itself
            // plus exactly one circle (sum of all corners),
            // plus an extra polygon with height r for each face of the polygon
            self.polygon.area() + PI * r * r + self.polygon.side_length_sum() * r
        }
    }

    #[inline]
    pub fn second_moment_of_area(&self) -> f64 {
        // circle and polygon formulas from
        // https://en.wikipedia.org/wiki/List_of_second_moments_of_area
        #[inline]
        fn circle(r: f64) -> f64 {
            FRAC_PI_2 * r.powi(4)
        }
        #[inline]
        fn rect(hw: f64, hh: f64) -> f64 {
            (4.0 / 3.0) * (hw.powi(3) * hh + hw * hh.powi(3))
        }
        use ColliderPolygon as P;
        let polygon_part = match self.polygon {
            // special cases for circles and capsules
            P::Point => return circle(self.circle_r),
            P::LineSegment { hl } => {
                let rect_part = rect(hl, self.circle_r);
                // parallel axis theorem (I' = I + Ad^2) for the circle halves
                // both halves are offset the same distance, so it winds up being the same as
                // offsetting the whole thing that distance in any direction
                let circle_with_offset =
                    circle(self.circle_r) + (PI * self.circle_r.powi(2) * hl.powi(2));
                return rect_part + circle_with_offset;
            }
            P::Rect { hw, hh } => rect(hw, hh),
            // this one is from https://amesweb.info/section/area-moment-of-inertia-of-equilateral-triangle.aspx
            P::Triangle { outer_r } => 0.03608 * (outer_r / FRAC_PI_6_TAN).powi(4),
            P::Hexagon { outer_r } => (5.0 * SQRT_3 / 8.0) * outer_r.powi(4),
        };
        // simple polygon without rounding
        if self.circle_r == 0.0 {
            return polygon_part;
        }
        // rounded polygon. express as a composite shape of the inner polygon, circle sectors
        // and edge rectangles and compute using the parallel axis theorem
        let expanded_part = match self.polygon {
            // already returned if point or capsule
            P::Point | P::LineSegment { .. } => unreachable!(),
            P::Rect { hw, hh } => {
                // two horizontal and two vertical edge rectangles
                let horiz_edge_rect = {
                    let base = rect(hw, self.circle_r / 2.0);
                    let area = 2.0 * hw * self.circle_r;
                    let offset_sq = hh.powi(2);
                    base + area * offset_sq
                };
                let vert_edge_rect = {
                    let base = rect(hh, self.circle_r / 2.0);
                    let area = 2.0 * hh * self.circle_r;
                    let offset_sq = hw.powi(2);
                    base + area * offset_sq
                };
                // all circle caps are an equal distance away
                // so we get the sum of them by just offsetting the whole circle
                let circle_cap_sum = {
                    let base = circle(self.circle_r);
                    let area = PI * self.circle_r.powi(2);
                    let offset_sq = uv::DVec2::new(hw, hh).mag_sq();
                    base + area * offset_sq
                };
                2.0 * (horiz_edge_rect + vert_edge_rect) + circle_cap_sum
            }
            P::Triangle { outer_r } => {
                // three edge rectangles, all the same distance away,
                // and three circle caps also all the same distance away
                let edge_rect = {
                    let long_edge_len = outer_r / FRAC_PI_6_TAN;
                    let base = rect(long_edge_len / 2.0, self.circle_r / 2.0);
                    let area = long_edge_len * self.circle_r;
                    let offset_sq = (outer_r / 2.0).powi(2);
                    base + area * offset_sq
                };
                // again, all the same distance away so we can treat it as one offset circle
                let circle_cap_sum = {
                    let base = circle(self.circle_r);
                    let area = PI * self.circle_r.powi(2);
                    let offset_sq = outer_r.powi(2);
                    base + area * offset_sq
                };
                3.0 * edge_rect + circle_cap_sum
            }
            P::Hexagon { outer_r } => {
                // six edge rectangles and six circle caps, all the same distance away
                let edge_rect = {
                    let long_edge_len = outer_r;
                    let base = rect(long_edge_len / 2.0, self.circle_r / 2.0);
                    let area = long_edge_len * self.circle_r;
                    let offset_sq = (FRAC_PI_6_COS * outer_r).powi(2);
                    base + area * offset_sq
                };
                let circle_cap_sum = {
                    let base = circle(self.circle_r);
                    let area = PI * self.circle_r.powi(2);
                    let offset_sq = outer_r.powi(2);
                    base + area * offset_sq
                };
                6.0 * edge_rect + circle_cap_sum
            }
        };

        polygon_part + expanded_part
    }

    #[inline]
    pub fn bounding_sphere_r(&self) -> f64 {
        self.polygon.bounding_sphere_r() + self.circle_r
    }

    pub fn aabb(&self, pose: m::PhysicsPose) -> AABB {
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

    /// Increase rounding such that edges remain the same distance from the origin.
    /// Works like the CSS "corner-radius" property.
    ///
    /// May not have the full effect if the shape reaches the limit of how rounded it can be.
    pub fn rounded_inward(&self, amount: f64) -> Self {
        let (shrunk_poly, actual_amount) = self.polygon.shrink(amount);
        Self {
            circle_r: self.circle_r + actual_amount,
            polygon: shrunk_poly,
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
const SQRT_3: f64 = 1.73205080757;
const FRAC_PI_6_COS: f64 = 0.866025403784;
const FRAC_PI_6_SIN: f64 = 0.5;
const FRAC_PI_6_TAN: f64 = 0.57735026919;
const FRAC_PI_3_COS: f64 = FRAC_PI_6_SIN;
const FRAC_PI_3_SIN: f64 = FRAC_PI_6_COS;
const AXIS_30_DEG: UnitDVec2 =
    UnitDVec2::new_unchecked(uv::DVec2::new(FRAC_PI_6_COS, FRAC_PI_6_SIN));
const AXIS_60_DEG: UnitDVec2 =
    UnitDVec2::new_unchecked(uv::DVec2::new(FRAC_PI_3_COS, FRAC_PI_3_SIN));
// these are left normals of 30 and 60 but normal can't be const computed because of negation
const AXIS_120_DEG: UnitDVec2 =
    UnitDVec2::new_unchecked(uv::DVec2::new(-FRAC_PI_6_SIN, FRAC_PI_6_COS));
const AXIS_150_DEG: UnitDVec2 =
    UnitDVec2::new_unchecked(uv::DVec2::new(-FRAC_PI_3_SIN, FRAC_PI_3_COS));

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

    /// Move the edges inward such that the space between the old and new edges is `amount`
    /// (measured perpendicularly).
    ///
    /// Clamps to a small positive value to ensure physics doesn't break,
    /// and returns the amount of actual shrinking done.
    fn shrink(&self, amount: f64) -> (Self, f64) {
        const MIN: f64 = 0.001;
        match *self {
            Self::Point | Self::LineSegment { .. } => (*self, 0.0),
            Self::Rect { hw, hh } => {
                let hw_ = (hw - amount).max(MIN);
                let hh_ = (hh - amount).max(MIN);
                (Self::Rect { hw: hw_, hh: hh_ }, (hw - hw_).min(hh - hh_))
            }
            Self::Triangle { outer_r } => {
                let r_ = (outer_r - amount / FRAC_PI_6_SIN).max(MIN);
                (
                    Self::Triangle { outer_r: r_ },
                    (outer_r - r_) * FRAC_PI_6_SIN,
                )
            }
            Self::Hexagon { outer_r } => {
                let r_ = (outer_r - amount / FRAC_PI_3_SIN).max(MIN);
                (
                    Self::Hexagon { outer_r: r_ },
                    (outer_r - r_) * FRAC_PI_3_SIN,
                )
            }
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

    fn aabb(&self, rotation: uv::DRotor2) -> AABB {
        let symmetric_extent = match *self {
            Self::Point => uv::DVec2::zero(),
            Self::LineSegment { hl } => (rotation * uv::DVec2::new(hl, 0.0)).abs(),
            Self::Rect { hw, hh } => {
                (rotation * uv::DVec2::new(hw, 0.0)).abs()
                    + (rotation * uv::DVec2::new(0.0, hh)).abs()
            }
            // probably not worth the computation time of all the trigonometry
            // to get the exact smallest aabb
            Self::Triangle { outer_r } | Self::Hexagon { outer_r } => {
                uv::DVec2::new(outer_r, outer_r)
            }
        };
        AABB {
            min: -symmetric_extent,
            max: symmetric_extent,
        }
    }

    /// Whether or not the shape has mirror symmetry with respect to the origin point.
    /// If true, we can only return half the edges and work with their mirror images.
    #[inline]
    pub(crate) fn is_rotationally_symmetrical(&self) -> bool {
        match *self {
            Self::Point | Self::LineSegment { .. } | Self::Rect { .. } | Self::Hexagon { .. } => {
                true
            }
            Self::Triangle { .. } => false,
        }
    }

    /// Tangent of HALF (future self, remember this) of the angle between edges is needed to
    /// compute the edges of the outer polygon from the inner polygon.
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
            // tan(1/2 * pi/3)
            Self::Triangle { .. } => 0.57735026919,
            // tan(1/2 * 2pi/3)
            Self::Hexagon { .. } => 1.73205080757,
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
                normal: UnitDVec2::unit_y(),
                edge: Edge {
                    start: uv::DVec2::new(hl, 0.0),
                    dir: -UnitDVec2::unit_x(),
                    length: 2.0 * hl,
                },
            },
            Self::Rect { hw, hh } => match idx {
                0 => PolygonEdge {
                    normal: UnitDVec2::unit_x(),
                    edge: Edge {
                        start: uv::DVec2::new(hw, -hh),
                        dir: UnitDVec2::unit_y(),
                        length: 2.0 * hh,
                    },
                },
                1 => PolygonEdge {
                    normal: UnitDVec2::unit_y(),
                    edge: Edge {
                        start: uv::DVec2::new(hw, hh),
                        dir: -UnitDVec2::unit_x(),
                        length: 2.0 * hw,
                    },
                },
                _ => bad_edge(),
            },
            Self::Triangle { outer_r } => match idx {
                0 => PolygonEdge {
                    normal: -UnitDVec2::unit_y(),
                    // distance to the endpoints of an equilateral triangle
                    // is double the radius of the inscribed circle
                    edge: Edge {
                        start: -outer_r * *AXIS_30_DEG,
                        dir: UnitDVec2::unit_x(),
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                1 => PolygonEdge {
                    normal: AXIS_30_DEG,
                    edge: Edge {
                        start: -outer_r * *AXIS_150_DEG,
                        dir: AXIS_120_DEG,
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                2 => PolygonEdge {
                    normal: AXIS_150_DEG,
                    edge: Edge {
                        start: uv::DVec2::new(0.0, outer_r),
                        dir: -AXIS_60_DEG,
                        length: outer_r / FRAC_PI_6_TAN,
                    },
                },
                _ => bad_edge(),
            },
            Self::Hexagon { outer_r } => match idx {
                0 => PolygonEdge {
                    normal: AXIS_30_DEG,
                    edge: Edge {
                        start: uv::DVec2::new(outer_r, 0.0),
                        dir: AXIS_120_DEG,
                        length: outer_r,
                    },
                },
                1 => PolygonEdge {
                    normal: UnitDVec2::unit_y(),
                    edge: Edge {
                        start: outer_r * *AXIS_60_DEG,
                        dir: -UnitDVec2::unit_x(),
                        length: outer_r,
                    },
                },
                2 => PolygonEdge {
                    normal: AXIS_150_DEG,
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

    /// Get the perpendicular distance from the shape's center to the given edge.
    pub(super) fn get_edge_extent(&self, idx: usize) -> f64 {
        let bad_edge = || {
            panic!(
                "Called get_edge for {:?} with an out of bounds index {}",
                self, idx
            )
        };
        match *self {
            Self::Point => bad_edge(),
            Self::LineSegment { .. } => 0.0,
            Self::Rect { hw, hh } => match idx {
                0 => hw,
                1 => hh,
                _ => bad_edge(),
            },
            Self::Triangle { outer_r } => outer_r / 2.0,
            Self::Hexagon { outer_r } => FRAC_PI_6_COS * outer_r,
        }
    }

    /// Get the distance from the shape's center to its farthest extent
    /// when projected onto the given axis.
    ///
    /// `dir` must be given in object-local space.
    pub(super) fn projected_extent(&self, dir: UnitDVec2) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { hl } => dir.x.abs() * hl,
            Self::Rect { hw, hh } => dir.x.abs() * hw + dir.y.abs() * hh,
            Self::Triangle { outer_r } => {
                [UnitDVec2::unit_y(), -AXIS_30_DEG, -AXIS_150_DEG]
                    .into_iter()
                    .map(|dir_to_vertex| dir_to_vertex.dot(*dir))
                    .max_by(|p0, p1| p0.total_cmp(p1))
                    .unwrap()
                    * outer_r
            }
            Self::Hexagon { outer_r } => {
                [UnitDVec2::unit_x(), AXIS_60_DEG, AXIS_120_DEG]
                    .into_iter()
                    .map(|dir_to_vertex| dir_to_vertex.dot(*dir).abs())
                    .max_by(|p0, p1| p0.total_cmp(p1))
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
    pub(super) fn supporting_edge(&self, dir: uv::DVec2) -> PolygonEdge {
        match *self {
            Self::Point => panic!("Don't call supporting_edge on a point"),
            Self::LineSegment { hl } => PolygonEdge {
                edge: Edge {
                    start: uv::DVec2::new(hl.copysign(dir.x), 0.0),
                    dir: UnitDVec2::new_unchecked(uv::DVec2::new(1_f64.copysign(-dir.x), 0.0)),
                    length: 2.0 * hl,
                },
                normal: UnitDVec2::new_unchecked(uv::DVec2::new(0.0, 1_f64.copysign(dir.y))),
            },
            Self::Rect { hw, hh } => {
                let start = uv::DVec2::new(hw.copysign(dir.x), hh.copysign(dir.y));
                if dir.x.abs() > dir.y.abs() {
                    PolygonEdge {
                        edge: Edge {
                            start,
                            dir: UnitDVec2::new_unchecked(uv::DVec2::new(
                                0.0,
                                -(1_f64.copysign(dir.y)),
                            )),
                            length: hh * 2.0,
                        },
                        normal: UnitDVec2::new_unchecked(uv::DVec2::new(
                            1_f64.copysign(dir.x),
                            0.0,
                        )),
                    }
                } else {
                    PolygonEdge {
                        edge: Edge {
                            start,
                            dir: UnitDVec2::new_unchecked(uv::DVec2::new(
                                -(1_f64.copysign(dir.x)),
                                0.0,
                            )),
                            length: hw * 2.0,
                        },
                        normal: UnitDVec2::new_unchecked(uv::DVec2::new(
                            0.0,
                            1_f64.copysign(dir.y),
                        )),
                    }
                }
            }
            Self::Triangle { .. } | Self::Hexagon { .. } => {
                let closest_edge = (0..self.edge_count())
                    .map(|i| {
                        let edge = self.get_edge(i);
                        if self.is_rotationally_symmetrical() && edge.normal.dot(dir) < 0.0 {
                            edge.mirrored()
                        } else {
                            edge
                        }
                    })
                    .max_by(|e0, e1| e0.normal.dot(dir).partial_cmp(&e1.normal.dot(dir)).unwrap())
                    .unwrap();

                PolygonEdge {
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

    /// Get the closest point to a point on the exterior edge of the polygon,
    /// plus whether or not the queried point is inside the polygon.
    ///
    /// Used in the special case of circle - other shape collisions.
    pub(super) fn closest_boundary_point(&self, pt: uv::DVec2) -> ClosestBoundaryPoint {
        match *self {
            Self::Point => ClosestBoundaryPoint {
                pt: uv::DVec2::zero(),
                is_interior: false,
            },
            Self::LineSegment { hl } => ClosestBoundaryPoint {
                pt: uv::DVec2::new(pt.x.max(-hl).min(hl), 0.0),
                is_interior: false,
            },
            Self::Rect { hw, hh } => {
                let x_dist = pt.x.abs() - hw;
                let y_dist = pt.y.abs() - hh;
                match (x_dist > 0.0, y_dist > 0.0) {
                    // we're outside the whole rect and closest point is a corner
                    (true, true) => ClosestBoundaryPoint {
                        pt: uv::DVec2::new(hw.copysign(pt.x), hh.copysign(pt.y)),
                        is_interior: false,
                    },
                    // outside only on the x-axis
                    (true, false) => ClosestBoundaryPoint {
                        pt: uv::DVec2::new(hw.copysign(pt.x), pt.y),
                        is_interior: false,
                    },
                    // outside only on the y-axis
                    (false, true) => ClosestBoundaryPoint {
                        pt: uv::DVec2::new(pt.x, hh.copysign(pt.y)),
                        is_interior: false,
                    },
                    // inside
                    (false, false) => ClosestBoundaryPoint {
                        pt: if x_dist.abs() < y_dist.abs() {
                            uv::DVec2::new(hw.copysign(pt.x), pt.y)
                        } else {
                            uv::DVec2::new(pt.x, hh.copysign(pt.y))
                        },
                        is_interior: true,
                    },
                }
            }
            // the following works for any shape:
            // find the edge where the point's projection is the closest to the point,
            // return the point's projection.
            Self::Triangle { .. } | Self::Hexagon { .. } => {
                let mut min_dist_to_edge = f64::MAX;
                // meaningless default that is guaranteed to be overwritten
                let mut closest_point = ClosestBoundaryPoint {
                    pt: uv::DVec2::zero(),
                    is_interior: false,
                };
                for edge in (0..self.edge_count()).map(|i| self.get_edge(i)) {
                    let PolygonEdge { normal, edge } =
                        if self.is_rotationally_symmetrical() && edge.normal.dot(pt) < 0.0 {
                            edge.mirrored()
                        } else {
                            edge
                        };
                    let edge_start_to_pt = pt - edge.start;
                    let edge_t_to_pt = edge.dir.dot(edge_start_to_pt);
                    if edge_t_to_pt < 0.0 {
                        // projects outside of current edge,
                        // if this is the closest point overall we're definitely outside the shape
                        let dist_to_edge = edge_start_to_pt.mag();
                        if dist_to_edge < min_dist_to_edge {
                            min_dist_to_edge = dist_to_edge;
                            closest_point = ClosestBoundaryPoint {
                                pt: edge.start,
                                is_interior: false,
                            }
                        }
                    } else if edge_t_to_pt <= edge.length {
                        // projects inside of current edge,
                        // need to check which side of the edge we're on
                        let normal_dist = normal.dot(edge_start_to_pt);
                        let (normal_dist, is_interior) = if normal_dist >= 0.0 {
                            (normal_dist, false)
                        } else {
                            (-normal_dist, true)
                        };
                        if normal_dist < min_dist_to_edge {
                            min_dist_to_edge = normal_dist;
                            closest_point = ClosestBoundaryPoint {
                                pt: edge.start + edge_t_to_pt * *edge.dir,
                                is_interior,
                            }
                        }
                    } else {
                        // beyond the far end of the edge which is also the start of another edge,
                        // we will handle this one when we get to that other edge
                        continue;
                    };
                }

                closest_point
            }
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
pub struct PhysicsMaterial {
    /// Coefficient of static friction.
    /// Set to None to opt out of static friction.
    pub static_friction_coef: Option<f64>,
    /// Coefficient of dynamic friction.
    /// Set to None to opt out of dynamic friction.
    pub dynamic_friction_coef: Option<f64>,
    pub restitution_coef: f64,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        PhysicsMaterial {
            static_friction_coef: Some(1.6),
            dynamic_friction_coef: Some(1.5),
            restitution_coef: 0.0,
        }
    }
}

impl PhysicsMaterial {
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
                if shape.is_rotationally_symmetrical() {
                    0..shape.edge_count()
                } else {
                    0..0
                }
                .map(|i| shape.get_edge(i).mirrored()),
            ) {
                let assert_print_info = |cond: bool,
                                         pt: uv::DVec2,
                                         cp: ClosestBoundaryPoint,
                                         region: &str,
                                         t: f64| {
                    assert!(
                        cond,
                        "shape {shape:?}\n\nregion {region}\n\nt {t}\n\nedge {edge:?}\n\npoint {pt:?}\n\nclosest {cp:?}",
                    );
                };
                // inside voronoi regions
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
                        "inside",
                        t,
                    );
                }
                // outside edge voronoi regions
                for t in [0.0, 0.01, 0.3, 0.45, 0.5, 0.55, 0.7, 0.99, 1.0] {
                    let pt_on = edge.edge.start + t * edge.edge.length * *edge.edge.dir;
                    let pt_out = pt_on + *edge.normal;
                    let cp_out = shape.closest_boundary_point(pt_out);
                    assert_print_info(
                        !cp_out.is_interior
                            && (cp_out.pt - edge.edge.start).dot(*edge.normal).abs() < 0.0001
                            && (cp_out.pt - pt_out).dot(*edge.edge.dir).abs() < 0.0001,
                        pt_out,
                        cp_out,
                        "outside edge",
                        t,
                    );
                }
                // outside vertex voronoi regions
                let pt_before_out = edge.edge.start - 0.1 * *edge.edge.dir + *edge.normal;
                let cp_before_out = shape.closest_boundary_point(pt_before_out);
                assert_print_info(
                    !cp_before_out.is_interior
                        && (cp_before_out.pt - edge.edge.start).mag_sq() < 0.0001,
                    pt_before_out,
                    cp_before_out,
                    "outside vertex (before edge)",
                    -0.1,
                );
                let edge_end = edge.edge.end_point();
                let pt_after_out = edge_end + 0.1 * *edge.edge.dir + *edge.normal;
                let cp_after_out = shape.closest_boundary_point(pt_after_out);
                assert_print_info(
                    !cp_after_out.is_interior && (cp_after_out.pt - edge_end).mag_sq() < 0.0001,
                    pt_after_out,
                    cp_after_out,
                    "outside vertex (after edge)",
                    1.1,
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
                        if shape.is_rotationally_symmetrical() && edge.normal.dot(*dir) < 0.0 {
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
                        "shape {shape:?}\n\ndir {dir:?}\n\nsupporting edge {supp:?}\n\nclosest edge {closest_edge:?}",
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
                        if shape.is_rotationally_symmetrical() && point_proj < 0.0 {
                            -point_proj
                        } else {
                            point_proj
                        }
                    })
                    .max_by(|p0, p1| p0.partial_cmp(p1).unwrap())
                    .unwrap();

                assert!(
                    (proj - farthest_point_proj).abs() < 0.0001,
                    "shape {shape:?}\n\ndir {dir:?}",
                );
            }
        }
    }

    #[test]
    fn round_inward_preserves_edge_distance() {
        for shape in TEST_POLYGONS.into_iter().map(ColliderShape::from) {
            let rounded_shape = shape.rounded_inward(0.2);
            for edge_idx in 0..shape.polygon.edge_count() {
                let orig_edge = shape.polygon.get_edge(edge_idx);
                let new_edge_inner = rounded_shape.polygon.get_edge(edge_idx);
                let new_edge_outer = new_edge_inner
                    .edge
                    .offset(rounded_shape.circle_r * *new_edge_inner.normal);

                let dist_diff = new_edge_outer.start.dot(*new_edge_inner.normal)
                    - orig_edge.edge.start.dot(*orig_edge.normal);
                assert!(
                    dist_diff.abs() < 0.0001,
                    "{shape:?} edge was moved by {dist_diff}"
                );
            }
        }
    }

    #[test]
    fn second_moment_of_area_changes_smoothly_and_monotonically() {
        // little sanity test to make sure my math does at least vaguely the right thing

        for shape in TEST_POLYGONS.into_iter().map(ColliderShape::from) {
            dbg!(shape);
            let mut prev_mom_area = shape.second_moment_of_area();

            const SHRINK_STEP: f64 = 0.02;
            for shrink in 1..20 {
                let rounded_shape = shape.rounded_inward(shrink as f64 * SHRINK_STEP);
                let new_mom_area = rounded_shape.second_moment_of_area();
                let diff = new_mom_area - prev_mom_area;
                assert!(
                    diff < 0.0,
                    "second moment of area grew on step {shrink} by {diff}"
                );
                assert!(
                    diff > -0.05,
                    "second moment of area changed unexpectedly much on step {shrink} (changed by {diff})",
                );

                prev_mom_area = new_mom_area
            }
        }
    }

    fn sample_unit_circle(sample_count: usize) -> impl Iterator<Item = m::UnitDVec2> {
        let angle_incr = std::f64::consts::TAU / sample_count as f64;
        (0..sample_count).map(move |i| {
            let angle = i as f64 * angle_incr;
            let (sin, cos) = angle.sin_cos();
            m::UnitDVec2::new_unchecked(uv::DVec2::new(cos, sin))
        })
    }
}
