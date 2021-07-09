/// A component that allows a game object to collide with others.
/// Note that a Pose component must also be present.
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub shape: ColliderShape,
    pub ty: ColliderType,
}

impl Collider {
    /// Create a solid circle collider from a radius.
    pub fn new_circle(radius: f64) -> Self {
        Collider {
            shape: ColliderShape::Circle { r: radius },
            ty: ColliderType::default(),
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
        Collider {
            shape: ColliderShape::Rect { hw, hh },
            ty: ColliderType::default(),
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

    pub fn area(&self) -> f64 {
        match self.shape {
            ColliderShape::Circle { r } => std::f64::consts::PI * r * r,
            ColliderShape::Rect { hw, hh } => 4.0 * hw * hh,
        }
    }

    pub fn moment_of_inertia_coef(&self) -> f64 {
        // from https://en.wikipedia.org/wiki/List_of_moments_of_inertia
        match self.shape {
            ColliderShape::Circle { r } => r * r / 2.0,
            ColliderShape::Rect { hw, hh } => (hw * hw + hh * hh) / 3.0,
        }
    }

    pub fn is_solid(&self) -> bool {
        matches!(self.ty, ColliderType::Solid(_))
    }
}

/// The physical shape of a collider.
#[derive(Clone, Copy, Debug)]
pub enum ColliderShape {
    Circle {
        r: f64,
    },
    /// The rect collider stores its side lengths halved because this makes
    /// intersection tests easier.
    Rect {
        hw: f64,
        hh: f64,
    },
}

/// Type of a collider. Solid ones respond to collisions when attached to bodies.
/// Triggers only cause an event to be sent.
#[derive(Clone, Copy, Debug)]
pub enum ColliderType {
    Solid(Material),
    Trigger,
}

impl Default for ColliderType {
    fn default() -> Self {
        Self::Solid(Material::default())
    }
}

/// Determines how the surface of a body responds to collisions.
///
/// Using a simplified friction model where each material has its own friction
/// coefficients (rather than the realistic model where every pair of materials
/// would have its own coefficients).
#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub static_friction_coef: f64,
    pub dynamic_friction_coef: f64,
    pub restitution_coef: f64,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            static_friction_coef: 1.8,
            dynamic_friction_coef: 1.5,
            restitution_coef: 0.0,
        }
    }
}

impl Material {
    /// Get the static friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn static_friction_with(&self, other: &Self) -> f64 {
        (self.static_friction_coef + other.static_friction_coef) / 2.0
    }

    /// Get the dynamic friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn dynamic_friction_with(&self, other: &Self) -> f64 {
        (self.dynamic_friction_coef + other.dynamic_friction_coef) / 2.0
    }

    /// Get the restitution coefficient between this material and another.
    ///
    /// It is computed as the largest coefficient between the two bodies.
    pub fn restitution_with(&self, other: &Self) -> f64 {
        self.restitution_coef.max(other.restitution_coef)
    }
}
