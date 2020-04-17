use super::{Collider, Velocity};

/// A rigid body can collide with other rigid bodies and respond to physical forces.
#[derive(Clone, Copy)]
pub struct RigidBody {
    body: BodyType,
    material: SurfaceMaterial,
}

/// The type of a rigid body determines how it is treated in physics updates.
#[derive(Clone, Copy)]
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
/// NOTE: work in progress, does not actually do anything at the moment!
#[derive(Clone, Copy)]
pub struct SurfaceMaterial {
    /// How "bouncy" a body is, i.e. how much energy is preserved in collisions.
    pub restitution: f32,
    /// How much the body resists motion parallel to its surface.
    pub friction: f32,
}

impl Default for SurfaceMaterial {
    fn default() -> Self {
        SurfaceMaterial {
            restitution: 0.2,
            friction: 0.1,
        }
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
                mass: Mass::mass(mass),
                moment_of_inertia: Mass::mass(collider.moment_of_inertia_coef() * mass),
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

    /// Restitution determines how much energy is preserved in collisions
    /// (0 = none, 1 = all).
    pub fn with_restitution(mut self, e: f32) -> Self {
        self.material.restitution = e;
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

    pub fn inverse_mass(&self) -> f32 {
        match self.body {
            BodyType::Dynamic { mass: m, .. } => m.inv(),
            _ => 0.0,
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
#[derive(Clone, Copy)]
pub struct Mass {
    mass: f32,
    inverse: f32,
}

impl Mass {
    pub fn mass(mass: f32) -> Self {
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

    pub fn get(self) -> f32 {
        self.mass
    }

    pub fn inv(self) -> f32 {
        self.inverse
    }
}
