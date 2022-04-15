use super::{Collider, Velocity};

/// A body is something that moves, typically a physics-enabled rigid body or particle.
/// Connect a Body with a Collider to make it collide with other things.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub velocity: Velocity,
    pub mass: Mass,
    pub moment_of_inertia: Mass,
}

impl Body {
    /// A particle responds to external forces but does not rotate.
    pub fn new_particle(mass: f64) -> Self {
        Self {
            velocity: Velocity::default(),
            mass: Mass::from(mass),
            moment_of_inertia: Mass::Infinite,
        }
    }

    /// Dynamic bodies respond to external forces and are allowed to rotate.
    /// This constructor calculates mass and moment of inertia from the given density and
    /// collider shape.
    pub fn new_dynamic(collider: &Collider, density: f64) -> Self {
        let area = collider.shape.area();
        let mass = area * density;
        Self {
            velocity: Velocity::default(),
            mass: Mass::from(mass),
            moment_of_inertia: Mass::from(collider.shape.second_moment_of_area() * density),
        }
    }

    /// Create a dynamic body with the given mass instead of using density.
    /// The collider is still required in order to compute moment of inertia.
    pub fn new_dynamic_const_mass(collider: &Collider, mass: f64) -> Self {
        let area = collider.shape.area();
        let density = mass / area;
        Self {
            velocity: Velocity::default(),
            mass: Mass::from(mass),
            moment_of_inertia: Mass::from(collider.shape.second_moment_of_area() * density),
        }
    }

    /// Kinematic bodies are not affected by collision forces.
    pub fn new_kinematic() -> Self {
        Self {
            velocity: Velocity::default(),
            mass: Mass::Infinite,
            moment_of_inertia: Mass::Infinite,
        }
    }

    /// Set the velocity of the body in a builder-like chain.
    pub fn with_velocity(mut self, vel: Velocity) -> Self {
        self.velocity = vel;
        self
    }

    /// Check whether the body has finite mass or moment of inertia, allowing forces to have an
    /// effect on it.
    #[inline]
    pub fn sees_forces(&self) -> bool {
        !matches!(
            (self.mass, self.moment_of_inertia),
            (Mass::Infinite, Mass::Infinite)
        )
    }
}

/// Mass or moment of inertia of a body, which can be infinite.
///
/// This stores both a mass value and its inverse, because calculating inverse mass
/// is expensive and needed a lot in physics calculations.
#[derive(Clone, Copy, Debug)]
pub enum Mass {
    Finite { mass: f64, inverse: f64 },
    Infinite,
}

impl From<f64> for Mass {
    #[inline]
    fn from(mass: f64) -> Self {
        Mass::Finite {
            mass,
            inverse: 1.0 / mass,
        }
    }
}

impl Mass {
    /// Get the inverse of the mass, which is zero if the mass is infinite.
    #[inline]
    pub fn inv(&self) -> f64 {
        match self {
            Mass::Finite { inverse, .. } => *inverse,
            Mass::Infinite => 0.0,
        }
    }
}
