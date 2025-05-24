use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::math::{self as m, uv, UnitDVec2};
use crate::physics::collision::{
    shape_shape::{ClosestBoundaryPoint, Edge, PolygonEdge},
    AABB,
};

use super::constants as cs;

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
            P::Point | P::Regular { points: 0, .. } => return circle(self.circle_r),
            P::LineSegment { hl }
            | P::Regular {
                points: 1 | 2,
                r: hl,
            } => {
                let rect_part = rect(hl, self.circle_r);
                // parallel axis theorem (I' = I + Ad^2) for the circle halves
                // both halves are offset the same distance, so it winds up being the same as
                // offsetting the whole thing that distance in any direction
                let circle_with_offset =
                    circle(self.circle_r) + (PI * self.circle_r.powi(2) * hl.powi(2));
                return rect_part + circle_with_offset;
            }
            P::Rect { hw, hh } => rect(hw, hh),
            P::Regular { points, r } => {
                // regular polygon decomposes into `2 * points` right triangles
                // with one point at the centroid;
                // compute one of these using the formula from wikipedia linked above
                // (variable names here match that formula, with a = b)
                let (b, h) = if points <= cs::PRECOMPUTED_COUNT {
                    (
                        r * cs::EDGE_EXTENT_COEFS[points],
                        0.5 * r * cs::EDGE_LENGTH_COEFS[points],
                    )
                } else {
                    (
                        r * f64::cos(PI / points as f64),
                        r * f64::sin(PI / points as f64),
                    )
                };
                let one_triangle = (b * h.powi(3) + 3. * b.powi(3) * h) / 12.;
                2. * one_triangle * points as f64
            }
        };
        // simple polygon without rounding
        if self.circle_r == 0.0 {
            return polygon_part;
        }
        // rounded polygon. express as a composite shape of the inner polygon, circle sectors
        // and edge rectangles and compute using the parallel axis theorem
        let expanded_part = match self.polygon {
            // already returned if point or capsule
            P::Point | P::LineSegment { .. } | P::Regular { points: 0..=2, .. } => unreachable!(),
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
            P::Regular { points, r } => {
                // `points` edge rectangles, all the same distance away,
                // and `points` circle caps also the same distance away
                let edge_rect = {
                    let long_edge_len = if points <= cs::PRECOMPUTED_COUNT {
                        r * cs::EDGE_LENGTH_COEFS[points]
                    } else {
                        r * 2. * f64::sin(PI / points as f64)
                    };
                    let base = rect(long_edge_len / 2., self.circle_r / 2.);
                    let area = long_edge_len * self.circle_r;
                    let offset_sq = (r / 2.).powi(2);
                    base + area * offset_sq
                };
                // again, all the same distance away so we can treat it as one offset circle
                let circle_cap_sum = {
                    let base = circle(self.circle_r);
                    let area = PI * self.circle_r.powi(2);
                    let offset_sq = r.powi(2);
                    base + area * offset_sq
                };
                points as f64 * edge_rect + circle_cap_sum
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
    /// A regular polygon parameterized by number of points
    /// and distance to the points from the center.
    ///
    /// This is always oriented such that bottom edge is parallel to the x-axis,
    /// and if the number of vertices is odd, symmetry is thus also along the x-axis.
    Regular {
        /// Number of points in the polygon.
        ///
        /// If points < 3, results are unspecified and certainly wrong.
        points: usize,
        /// Distance to the points from the center.
        r: f64,
    },
}

//
// shape-specific utilities and axis constants
//

impl ColliderPolygon {
    //
    // physics properties
    //

    fn area(&self) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { .. } => 0.0,
            Self::Rect { hw, hh } => 4.0 * hw * hh,
            Self::Regular { points, r } => {
                // this isn't called every frame
                // so I can't be bothered to precompute these trig bits
                let points = points as f64;
                let angle_btw_points = TAU / points;
                0.5 * points * r.powi(2) * f64::sin(angle_btw_points)
            }
        }
    }

    fn side_length_sum(&self) -> f64 {
        match *self {
            Self::Point => 0.0,
            Self::LineSegment { hl } => 4.0 * hl, // counts in both directions
            Self::Rect { hw, hh } => 4.0 * (hw + hh),
            Self::Regular { points, r } => {
                let points = points as f64;
                let half_angle_btw_points = PI / points;
                2. * points * r * f64::sin(half_angle_btw_points)
            }
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
            Self::Regular { points, r } => {
                let r_proportion = if points <= cs::PRECOMPUTED_COUNT {
                    cs::EDGE_EXTENT_COEFS[points]
                } else {
                    f64::cos(PI / points as f64)
                };
                let r_ = (r - amount / r_proportion).max(MIN);
                (Self::Regular { points, r: r_ }, (r - r_) * r_proportion)
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
            Self::Regular { r, .. } => r,
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
            Self::Regular { r, .. } => uv::DVec2::new(r, r),
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
            Self::Point | Self::LineSegment { .. } | Self::Rect { .. } => true,
            Self::Regular { points, .. } => points % 2 == 0,
        }
    }

    /// Tangent of HALF (future self, remember this) of the angle between edges is needed to
    /// compute the edges of the outer polygon from the inner polygon.
    /// This is specifically the angle _inside_ the polygon, not its complement.
    ///
    /// This is called every collision check so it's precomputed for regular polygons.
    ///
    /// For now we only have regular polygons and can get away with returning
    /// a constant with no parameters. If I want general polygons I'll have to think
    /// about how to associate a vertex and an edge
    pub(crate) fn half_angle_between_edges_tan(&self) -> f64 {
        match *self {
            Self::Point | Self::LineSegment { .. } => {
                panic!("Angle between edges shouldn't be called for points or line segments")
            }
            Self::Rect { .. } => 1.0,
            Self::Regular { points, .. } => {
                if points <= cs::PRECOMPUTED_COUNT {
                    cs::HALF_ANGLE_TANGENTS[points]
                } else {
                    // half the angle between edges is π-α
                    // where α is half the angle between points measured from the circumcenter,
                    // and tan(π-α) = 1 / tan(α)
                    1. / f64::tan(PI / points as f64)
                }
            }
        }
    }

    /// Poor man's generator by iterating indices and returning edges by matching on them
    /// so we don't need to allocate to iterate edges.
    /// This gives the number of edges used for this, accounting for symmetry
    /// by halving even edge counts
    pub(crate) fn edge_count(&self) -> usize {
        match *self {
            Self::Point => 0,
            Self::LineSegment { .. } => 1,
            Self::Rect { .. } => 2,
            Self::Regular { points, .. } => {
                if points % 2 == 0 {
                    points / 2
                } else {
                    points
                }
            }
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
            Self::Point | Self::Regular { points: 0, .. } => bad_edge(),
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
            Self::Regular { points, r } => {
                if points <= cs::PRECOMPUTED_COUNT {
                    let axis = cs::REGULAR_POLY_AXIS_LUTS[points][idx];
                    PolygonEdge {
                        normal: axis,
                        edge: Edge {
                            start: r * *cs::REGULAR_POLY_POINT_LUTS[points][idx],
                            dir: m::unit_left_normal(axis),
                            length: r * cs::EDGE_LENGTH_COEFS[points],
                        },
                    }
                } else {
                    // this is expensive to do for every edge of a large polygon
                    // but polygons this big are unlikely to show up in games
                    // and already expensive just by virtue of having so many edges;
                    // just here for completeness's sake so we don't crash when they do show up
                    let angle_incr = TAU / points as f64;
                    let angle = idx as f64 * angle_incr;
                    let (sin, cos) = angle.sin_cos();
                    // remember that the first axis is -y, not +x like usual
                    let axis = UnitDVec2::new_unchecked(m::DVec2::new(sin, -cos));
                    let tangent = m::unit_left_normal(axis);
                    let half_edge_length = r * f64::sin(0.5 * angle_incr);
                    let inner_r = r * f64::cos(0.5 * angle_incr);

                    PolygonEdge {
                        normal: axis,
                        edge: Edge {
                            start: inner_r * *axis - half_edge_length * *tangent,
                            dir: tangent,
                            length: 2. * half_edge_length,
                        },
                    }
                }
            }
        }
    }

    /// Get the perpendicular distance from the shape's center to the given edge.
    pub(crate) fn get_edge_extent(&self, idx: usize) -> f64 {
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
            Self::Regular { points, r } => {
                if points <= cs::PRECOMPUTED_COUNT {
                    r * cs::EDGE_EXTENT_COEFS[points]
                } else {
                    r * f64::cos(PI / points as f64)
                }
            }
        }
    }

    /// Get the distance from the shape's center to its farthest extent
    /// when projected onto the given axis.
    ///
    /// `dir` must be given in object-local space.
    pub(crate) fn projected_extent(&self, dir: UnitDVec2) -> f64 {
        match *self {
            Self::Point | Self::Regular { points: 0, .. } => 0.0,
            Self::LineSegment { hl } => dir.x.abs() * hl,
            Self::Rect { hw, hh } => dir.x.abs() * hw + dir.y.abs() * hh,
            Self::Regular { points, r } => {
                // for even numbers we have mirror symmetry
                // so we can check half the edges and take the absolute value
                let extent = |dir_to_vertex: &UnitDVec2| {
                    if points % 2 == 0 {
                        dir_to_vertex.dot(*dir).abs()
                    } else {
                        dir_to_vertex.dot(*dir)
                    }
                };
                if points <= cs::PRECOMPUTED_COUNT {
                    r * cs::REGULAR_POLY_POINT_LUTS[points]
                        .iter()
                        .map(extent)
                        .max_by(f64::total_cmp)
                        .unwrap()
                } else {
                    let angle_incr = TAU / points as f64;
                    r * (0..points)
                        .map(|i| {
                            // recall that the first vertex is the one just before the negative y axis,
                            // so these do not start at (1, 0)
                            let angle = (i as f64 - 0.5) * angle_incr;
                            let (sin, cos) = angle.sin_cos();
                            let dir_to_vertex = UnitDVec2::new_unchecked(m::DVec2::new(sin, -cos));
                            extent(&dir_to_vertex)
                        })
                        .max_by(f64::total_cmp)
                        .unwrap()
                }
            }
        }
    }

    /// Get the edge that is closest to the given direction,
    /// starting from the supporting point in that direction.
    ///
    /// `dir` must be given in object-local space but does not need to be
    /// normalized (note to self: DO NOT USE THE VALUE OF `dir * thing`, only compare).
    pub(crate) fn supporting_edge(&self, dir: uv::DVec2) -> PolygonEdge {
        match *self {
            Self::Point => {
                panic!("Don't call supporting_edge on a point")
            }
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
            Self::Regular { .. } => {
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
    pub(crate) fn closest_boundary_point(&self, pt: uv::DVec2) -> ClosestBoundaryPoint {
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
            Self::Regular { .. } => {
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

//
// tests
//

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{math::DVec2, physics::collision::shape_shape::ClosestBoundaryPoint};

    const TEST_POLYGONS: [ColliderPolygon; 5] = [
        ColliderPolygon::Rect { hw: 0.5, hh: 0.8 },
        ColliderPolygon::Regular { points: 3, r: 1. },
        ColliderPolygon::Regular { points: 6, r: 1. },
        ColliderPolygon::Regular { points: 7, r: 1.1 },
        ColliderPolygon::Regular {
            points: 18,
            r: 0.75,
        },
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
                                         pt: DVec2,
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
                    // note: the difference can become large quite quickly
                    // if we add polygons with large radius to TEST_POLYGONS,
                    // which is expected because this value grows cubically with radius
                    diff > -0.1,
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
            m::UnitDVec2::new_unchecked(DVec2::new(cos, sin))
        })
    }
}
