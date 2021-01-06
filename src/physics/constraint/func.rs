//! Constraint functions and their derivatives.

use super::Vec6;
use crate::math::{self, uv};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ConstraintFunction {
    Normal {
        normal: math::Unit<uv::Vec2>,
        offsets: [uv::Vec2; 2],
    },
    Distance {
        distance_squared: f32,
        offsets: [uv::Vec2; 2],
    },
}

impl ConstraintFunction {
    pub(crate) fn value(&self, tr1: uv::Isometry2, tr2: Option<uv::Isometry2>) -> f32 {
        let tr2 = tr2.unwrap_or(uv::Isometry2::identity());
        use ConstraintFunction::*;
        match self {
            Normal { .. } => {
                // we've already computed the value in collision detection
                // and we set the bias for this type of constraint separately in the solver
                0.0
            }
            Distance {
                distance_squared,
                offsets,
            } => {
                let actual_dist_sq = (tr2 * offsets[1] - tr1 * offsets[0]).mag_sq();

                // divide by 2 to make the jacobian match the derivative of this
                (actual_dist_sq - distance_squared) / 2.0
            }
        }
    }

    pub(crate) fn jacobian(&self, tr1: uv::Isometry2, tr2: Option<uv::Isometry2>) -> Vec6 {
        let tr2 = tr2.unwrap_or(uv::Isometry2::identity());
        use ConstraintFunction::*;
        match self {
            Normal { normal, offsets } => Vec6 {
                v1: **normal,
                w1: math::left_normal(offsets[0]).dot(**normal),
                v2: -**normal,
                w2: -math::left_normal(offsets[1]).dot(**normal),
            },
            Distance { offsets, .. } => {
                let dist_v = tr2 * offsets[1] - tr1 * offsets[0];
                let dist_v = if dist_v.x == 0.0 && dist_v.y == 0.0 {
                    // return the downwards direction if the points overlap perfectly,
                    // else this would cause a NaN to enter the system and cause a crash
                    -uv::Vec2::unit_y()
                } else {
                    dist_v
                };
                Vec6 {
                    v1: -dist_v,
                    w1: -math::left_normal(tr1.rotation * offsets[0]).dot(dist_v),
                    v2: dist_v,
                    w2: math::left_normal(tr2.rotation * offsets[1]).dot(dist_v),
                }
            }
        }
    }
}
