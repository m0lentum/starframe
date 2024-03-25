//! Interpolation methods useful in animation.
//!
//! All functions here assume `t` moves from 0 to 1.

use std::ops::{Add, Mul};

/// Linear interpolation.
pub fn lerp<T>(start: T, end: T, t: f32) -> T
where
    T: Copy + Mul<f32, Output = T> + Add<T, Output = T>,
{
    start * (1.0 - t) + end * t
}

/// Cubic spline interpolation as defined by the glTF standard.
pub fn cubic_spline<T>(start: T, start_tangent: T, end: T, end_tangent: T, t: f32) -> T
where
    T: Copy + Mul<f32, Output = T> + Add<T, Output = T>,
{
    let t_sq: f32 = t * t;
    let t_cu: f32 = t_sq * t;
    let a: f32 = 2.0 * t_cu - 3.0 * t_sq + 1.0;
    let b: f32 = t_cu - 2.0 * t_sq + t;
    let c: f32 = -2.0 * t_cu + 3.0 * t_sq;
    let d: f32 = t_cu - t_sq;
    start * a + start_tangent * b + end * c + end_tangent * d
}
