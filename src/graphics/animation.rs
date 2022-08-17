use crate::math::uv;
use std::ops::{Add, Mul};

/// Interpolated animation for floating point properties.
///
/// Target is an optional per-channel identifier type
/// for routing animation channels to the things they animate.
#[derive(Debug, Clone)]
pub struct Animation<Target = ()> {
    pub name: Option<String>,
    pub duration: f32,
    pub channels: Vec<Channel<Target>>,
}

impl<Target> Animation<Target> {
    pub fn new(name: Option<String>, channels: Vec<Channel<Target>>) -> Self {
        Self {
            name,
            duration: channels
                .iter()
                .map(|c| c.duration())
                .max_by(f32::total_cmp)
                .unwrap_or(0.0),
            channels,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Channel<Target = ()> {
    pub target: Target,
    pub ty: ChannelType,
    pub interpolation: InterpolationMode,
    pub keyframe_ts: Vec<f32>,
    pub data: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum InterpolationMode {
    Step,
    Linear,
    CubicSpline,
}

#[derive(Debug, Clone)]
pub enum ChannelType {
    Vector3,
    Rotor3,
}

impl<Target> Channel<Target> {
    #[inline]
    pub fn duration(&self) -> f32 {
        self.keyframe_ts.iter().last().cloned().unwrap_or(0.0)
    }

    /// Get the value of this animation channel at the given time t as a 3D vector.
    /// # Panics
    /// Panics if the channel type isn't Vector3.
    pub fn sample_vec3(&self, t: f32) -> uv::Vec3 {
        assert!(
            matches!(self.ty, ChannelType::Vector3),
            "Sample type mismatch"
        );
        let [prev_idx, next_idx] = self.current_window(t);

        match self.interpolation {
            InterpolationMode::Step | InterpolationMode::Linear => {
                if prev_idx == next_idx {
                    // outside animation's span, don't interpolate anything
                    let first_data = prev_idx * 3;
                    let v = &self.data[first_data..first_data + 3];
                    return uv::Vec3::new(v[0], v[1], v[2]);
                }

                let prev_fst_data = prev_idx * 3;
                let next_fst_data = next_idx * 3;
                let v_prev = &self.data[prev_fst_data..prev_fst_data + 3];
                let v_prev = uv::Vec3::new(v_prev[0], v_prev[1], v_prev[2]);
                let v_next = &self.data[next_fst_data..next_fst_data + 3];
                let v_next = uv::Vec3::new(v_next[0], v_next[1], v_next[2]);

                let t_normalized = (t - self.keyframe_ts[prev_idx])
                    / (self.keyframe_ts[next_idx] - self.keyframe_ts[prev_idx]);
                lerp(v_prev, v_next, t_normalized)
            }
            InterpolationMode::CubicSpline => {
                // cubic spline interpolation comes with two tangents per value,
                // so we need to step through the data differently
                if prev_idx == next_idx {
                    let first_data = prev_idx * 9;
                    let v = &self.data[first_data + 3..first_data + 6];
                    return uv::Vec3::new(v[0], v[1], v[2]);
                }

                let prev_fst_data = prev_idx * 9;
                let next_fst_data = next_idx * 9;
                let d_prev = &self.data[prev_fst_data..prev_fst_data + 9];
                let val_prev = uv::Vec3::new(d_prev[3], d_prev[4], d_prev[5]);
                let tan_prev = uv::Vec3::new(d_prev[6], d_prev[7], d_prev[8]);
                let d_next = &self.data[next_fst_data..next_fst_data + 9];
                let tan_next = uv::Vec3::new(d_next[0], d_next[1], d_next[2]);
                let val_next = uv::Vec3::new(d_next[3], d_next[4], d_next[5]);

                let t_normalized = (t - self.keyframe_ts[prev_idx])
                    / (self.keyframe_ts[next_idx] - self.keyframe_ts[prev_idx]);
                cubic_spline(val_prev, tan_prev, val_next, tan_next, t_normalized)
            }
        }
    }

    /// Get the value of this animation channel at the given time t as a 3D rotor.
    /// # Panics
    /// Panics if the channel type isn't Rotor3.
    pub fn sample_rotor3(&self, t: f32) -> uv::Rotor3 {
        assert!(
            matches!(self.ty, ChannelType::Rotor3),
            "Sample type mismatch"
        );
        let [prev_idx, next_idx] = self.current_window(t);

        match self.interpolation {
            InterpolationMode::Step | InterpolationMode::Linear => {
                if prev_idx == next_idx {
                    // outside animation's span, don't interpolate anything
                    let first_data = prev_idx * 4;
                    let v = &self.data[first_data..first_data + 4];
                    return uv::Rotor3::from_quaternion_array(v.try_into().unwrap());
                }

                let prev_fst_data = prev_idx * 4;
                let next_fst_data = next_idx * 4;
                let v_prev = &self.data[prev_fst_data..prev_fst_data + 4];
                let v_prev = uv::Rotor3::from_quaternion_array(v_prev.try_into().unwrap());
                let v_next = &self.data[next_fst_data..next_fst_data + 4];
                let v_next = uv::Rotor3::from_quaternion_array(v_next.try_into().unwrap());

                let t_normalized = (t - self.keyframe_ts[prev_idx])
                    / (self.keyframe_ts[next_idx] - self.keyframe_ts[prev_idx]);
                use uv::interp::Slerp;
                v_prev.slerp(v_next, t_normalized)
            }
            InterpolationMode::CubicSpline => {
                // cubic spline interpolation comes with two tangents per value,
                // so we need to step through the data differently
                if prev_idx == next_idx {
                    let first_data = prev_idx * 12;
                    let v = &self.data[first_data + 4..first_data + 8];
                    return uv::Rotor3::from_quaternion_array(v.try_into().unwrap());
                }

                let prev_fst_data = prev_idx * 12;
                let next_fst_data = next_idx * 12;
                let d_prev = &self.data[prev_fst_data..prev_fst_data + 12];
                let val_prev =
                    uv::Rotor3::from_quaternion_array((&d_prev[4..8]).try_into().unwrap());
                let tan_prev =
                    uv::Rotor3::from_quaternion_array((&d_prev[8..12]).try_into().unwrap());
                let d_next = &self.data[next_fst_data..next_fst_data + 12];
                let tan_next =
                    uv::Rotor3::from_quaternion_array((&d_next[0..4]).try_into().unwrap());
                let val_next =
                    uv::Rotor3::from_quaternion_array((&d_next[4..8]).try_into().unwrap());

                let t_normalized = (t - self.keyframe_ts[prev_idx])
                    / (self.keyframe_ts[next_idx] - self.keyframe_ts[prev_idx]);
                let spline_val = cubic_spline(val_prev, tan_prev, val_next, tan_next, t_normalized);
                spline_val.normalized()
            }
        }
    }

    /// Get the keyframe before and the keyframe after the given time.
    /// Returns 0 or keyframe_ts.len twice if outside the entire span of the animation.
    /// It is assumed the animation has at least one keyframe.
    fn current_window(&self, t: f32) -> [usize; 2] {
        if t <= self.keyframe_ts[0] {
            return [0, 0];
        }
        if let Some((i, _)) = self.keyframe_ts.iter().enumerate().find(|(_, kf)| t < **kf) {
            return [(i - 1).max(0), i];
        }
        let end = self.keyframe_ts.len() - 1;
        [end, end]
    }
}

pub fn lerp<T>(start: T, end: T, t: f32) -> T
where
    T: Copy + Mul<f32, Output = T> + Add<T, Output = T>,
{
    start * (1.0 - t) + end * t
}

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
