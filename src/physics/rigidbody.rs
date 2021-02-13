use super::{Collider, Velocity};

/// A rigid body can collide with other rigid bodies and respond to physical forces.
#[derive(Clone, Copy, Debug)]
pub struct RigidBody {
    pub(crate) body: BodyType,
    pub(crate) material: SurfaceMaterial,
}

/// The type of a rigid body determines how it is treated in physics updates.
#[derive(Clone, Copy, Debug)]
pub enum BodyType {
    /// Does not respond to collision forces and cannot move.
    Static,
    /// Does not respond to collision forces but can move.
    Kinematic { velocity: Velocity },
    /// The default type of body; responds to collision forces.
    Dynamic {
        velocity: Velocity,
        mass: Mass,
        moment_of_inertia: Mass,
    },
}

/// Determines how the surface of a body responds to collisions.
///
/// Using a simplified friction model where each material has its own friction
/// coefficients (rather than the realistic model where every pair of materials
/// would have its own coefficients).
#[derive(Clone, Copy, Debug)]
pub struct SurfaceMaterial {
    pub static_friction_coef: f32,
    pub dynamic_friction_coef: f32,
    pub restitution_coef: f32,
}

impl Default for SurfaceMaterial {
    fn default() -> Self {
        SurfaceMaterial {
            static_friction_coef: 3.0,
            dynamic_friction_coef: 2.5,
            restitution_coef: 0.0,
        }
    }
}

impl SurfaceMaterial {
    /// Get the static friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn static_friction_with(&self, other: &Self) -> f32 {
        (self.static_friction_coef + other.static_friction_coef) / 2.0
    }

    /// Get the dynamic friction coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn dynamic_friction_with(&self, other: &Self) -> f32 {
        (self.dynamic_friction_coef + other.dynamic_friction_coef) / 2.0
    }

    /// Get the restitution coefficient between this material and another.
    ///
    /// It is computed as the average between the two materials' friction coefficients.
    pub fn restitution_with(&self, other: &Self) -> f32 {
        (self.restitution_coef + other.restitution_coef) / 2.0
    }
}

impl RigidBody {
    /// Dynamic rigid bodies respond to collisions and environment forces.
    /// This constructor calculates mass and moment of inertia from the given density.
    pub fn new_dynamic(collider: &Collider, density: f32) -> Self {
        Self::new_dynamic_const_mass(collider, collider.area() * density)
    }

    /// Create a dynamic rigid body with the given mass instead of using density.
    /// The collider is still required to compute moment of inertia.
    pub fn new_dynamic_const_mass(collider: &Collider, mass: f32) -> Self {
        RigidBody {
            body: BodyType::Dynamic {
                velocity: Velocity::default(),
                mass: Mass::new(mass),
                moment_of_inertia: Mass::new(collider.moment_of_inertia_coef() * mass),
            },
            material: SurfaceMaterial::default(),
        }
    }

    /// Kinematic rigid bodies are not affected by collision forces.
    pub fn new_kinematic() -> Self {
        RigidBody {
            body: BodyType::Kinematic {
                velocity: Velocity::default(),
            },
            material: SurfaceMaterial::default(),
        }
    }

    /// Static rigid bodies do not move at all.
    pub fn new_static() -> Self {
        RigidBody {
            body: BodyType::Static,
            material: SurfaceMaterial::default(),
        }
    }

    pub fn with_velocity(mut self, vel: Velocity) -> Self {
        self.velocity_mut().map(|v| *v = vel);
        self
    }

    // accessors

    pub fn body(&self) -> &BodyType {
        &self.body
    }

    pub fn material(&self) -> &SurfaceMaterial {
        &self.material
    }

    pub fn responds_to_collisions(&self) -> bool {
        match self.body {
            BodyType::Dynamic { .. } => true,
            _ => false,
        }
    }

    pub fn velocity(&self) -> Option<&Velocity> {
        match self.body {
            BodyType::Static => None,
            BodyType::Kinematic { velocity: ref vel } => Some(vel),
            BodyType::Dynamic {
                velocity: ref vel, ..
            } => Some(vel),
        }
    }

    pub fn velocity_mut(&mut self) -> Option<&mut Velocity> {
        match self.body {
            BodyType::Static => None,
            BodyType::Kinematic {
                velocity: ref mut vel,
            } => Some(vel),
            BodyType::Dynamic {
                velocity: ref mut vel,
                ..
            } => Some(vel),
        }
    }

    pub fn velocity_or_zero(&self) -> Velocity {
        match self.body {
            BodyType::Static => Velocity::default(),
            BodyType::Kinematic { velocity: vel } => vel,
            BodyType::Dynamic { velocity: vel, .. } => vel,
        }
    }

    /// Returns the mass of the body if finite, otherwise None.
    pub fn mass(&self) -> Option<f32> {
        match self.body {
            BodyType::Dynamic { mass: m, .. } => Some(m.mass()),
            _ => None,
        }
    }

    /// Returns the inverse mass of the body, which is zero if the mass is infinite.
    pub fn inverse_mass(&self) -> f32 {
        match self.body {
            BodyType::Dynamic { mass: m, .. } => m.inv(),
            _ => 0.0,
        }
    }

    /// Returns the moment of inertia of the body if finite, otherwise None.
    pub fn moment_of_inertia(&self) -> Option<f32> {
        match self.body {
            BodyType::Dynamic {
                moment_of_inertia: m,
                ..
            } => Some(m.mass()),
            _ => None,
        }
    }

    pub fn inverse_moment_of_inertia(&self) -> f32 {
        match self.body {
            BodyType::Dynamic {
                moment_of_inertia: m,
                ..
            } => m.inv(),
            _ => 0.0,
        }
    }
}

/// This stores both a mass value and its inverse, because calculating inverse mass
/// is expensive and needed a lot in physics calculations.
#[derive(Clone, Copy, Debug)]
pub struct Mass {
    mass: f32,
    inverse: f32,
}

impl Mass {
    pub fn new(mass: f32) -> Self {
        Mass {
            mass,
            inverse: 1.0 / mass,
        }
    }

    pub fn from_inv(inverse: f32) -> Self {
        Mass {
            mass: 1.0 / inverse,
            inverse,
        }
    }

    pub fn mass(&self) -> f32 {
        self.mass
    }

    pub fn inv(&self) -> f32 {
        self.inverse
    }
}
