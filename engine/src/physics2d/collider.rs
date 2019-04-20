pub const COLLIDER_MAX_VERTS: usize = 16;

/// A component that allows a game object to collide with others.
/// Note that a Transform component must also be present.
#[derive(Clone, Copy)]
pub enum Collider {
    Circle {
        r: f32,
    },
    /// The rect collider stores its side lengths halved because this makes
    /// intersection tests easier.
    Rect {
        hw: f32,
        hh: f32,
    },
}

impl Collider {
    /// Create a circle collider from a radius.
    pub fn new_circle(radius: f32) -> Self {
        Collider::Circle { r: radius }
    }

    /// Create a rect collider with both sides set to the same length.
    pub fn new_square(side_length: f32) -> Self {
        let hw = side_length * 0.5;
        Collider::Rect { hw, hh: hw }
    }

    /// Create a rect collider with two different side lengths.
    pub fn new_rect(width: f32, height: f32) -> Self {
        Collider::Rect {
            hw: width * 0.5,
            hh: height * 0.5,
        }
    }

    /// Transform this collider into points that can be used to create a
    /// moleengine_visuals::Shape.
    pub fn as_points(&self) -> Vec<[f64; 2]> {
        match self {
            Collider::Circle { r } => {
                let r = f64::from(*r);
                // point count proportional to circle size
                let num_points = f64::max((r * 0.5).floor(), 12.0);
                let angle_interval = 2.0 * std::f64::consts::PI / num_points;

                let num_points = num_points as usize;
                let mut points = Vec::with_capacity(num_points);
                for i in 0..num_points {
                    let angle = i as f64 * angle_interval;
                    points.push([r * angle.cos(), r * angle.sin()]);
                }
                points
            }
            Collider::Rect { hw, hh } => {
                let hw = f64::from(*hw);
                let hh = f64::from(*hh);
                vec![[-hw, hh], [hw, hh], [hw, -hh], [-hw, -hh]]
            }
        }
    }
}
