/// A component that allows a game object to collide with others.
/// Note that a Transform component must also be present.
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    shape: ColliderShape,
    bounds_info: ColliderBoundsInfo,
}

/// The physical shape of a collider.
#[derive(Clone, Copy, Debug)]
pub enum ColliderShape {
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

/// Extra information about the physical bounds of the collider,
/// used primarily in collision and physics calculations.
#[derive(Clone, Copy, Debug)]
pub struct ColliderBoundsInfo {
    pub area: f32,
    pub bounding_sphere_r: f32,
}

impl Collider {
    /// Create a circle collider from a radius.
    pub fn new_circle(radius: f32) -> Self {
        Collider {
            shape: ColliderShape::Circle { r: radius },
            bounds_info: ColliderBoundsInfo {
                area: std::f32::consts::PI * radius * radius,
                bounding_sphere_r: radius,
            },
        }
    }

    /// Create a rect collider with both sides set to the same length.
    pub fn new_square(side_length: f32) -> Self {
        Collider::new_rect(side_length, side_length)
    }

    /// Create a rect collider with two different side lengths.
    pub fn new_rect(width: f32, height: f32) -> Self {
        let hw = width / 2.0;
        let hh = height / 2.0;
        Collider {
            shape: ColliderShape::Rect { hw, hh },
            bounds_info: ColliderBoundsInfo {
                area: width * height,
                bounding_sphere_r: (hw * hw + hh * hh).sqrt(),
            },
        }
    }

    pub fn shape(&self) -> &ColliderShape {
        &self.shape
    }

    pub fn bounds_info(&self) -> &ColliderBoundsInfo {
        &self.bounds_info
    }
}
