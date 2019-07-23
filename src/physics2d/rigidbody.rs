use super::Velocity;

/// A rigid body can collide with other rigid bodies
/// and respond to physical forces.
/// Collisions only happen if a Collider and a RigidBody component are both present.
#[derive(Clone, Copy)]
pub struct RigidBody {
    pub body_type: BodyType,
    /// The mass of a rigid body determines how much its linear velocity is affected by impulses.
    pub mass: Mass,
    /// Moment of inertia determines how much impulses affect the angular velocity of a rigid body.
    pub moment_of_inertia: Mass,
    /// Elasticity determines how "bouncy" a rigid body is,
    /// in other words, how much energy is preserved in collisions.
    pub elasticity: f32,
    // TODO: friction
    /// Drag determines how much linear momentum is discarded between updates.
    /// You can think of it as air resistance.
    /// Avoid setting this to zero, as this can cause simulations to
    /// become unstable due to energy gained from numerical errors.
    pub drag: f32,
    /// Angular drag is like drag, but for angular momentum.
    pub angular_drag: f32,

    pub velocity: Velocity,
}

impl Default for RigidBody {
    fn default() -> Self {
        RigidBody {
            body_type: BodyType::Dynamic,
            mass: Mass::mass(1.0),
            moment_of_inertia: Mass::mass(3000.0), // TODO: physically based value for this
            elasticity: 0.75,
            drag: 0.002,
            angular_drag: 0.001,
            velocity: Velocity::default(),
        }
    }
}

impl RigidBody {
    /// Kinematic rigid bodies are not affected by collision forces.
    pub fn make_kinematic(mut self) -> Self {
        self.body_type = BodyType::Kinematic;
        self
    }

    /// Static rigid bodies do not move at all.
    pub fn make_static(mut self) -> Self {
        self.body_type = BodyType::Static;
        self
    }

    /// Mass determines how much collisions affect this body vs. the other one.
    pub fn with_mass(mut self, mass: f32) -> Self {
        self.mass = Mass::mass(mass);
        self
    }

    /// Elasticity determines how much energy is preserved in collisions
    /// (0 = none, 1 = all)
    pub fn with_elasticity(mut self, e: f32) -> Self {
        self.elasticity = e;
        self
    }

    pub fn with_drag(mut self, d: f32) -> Self {
        self.drag = d;
        self
    }

    pub fn with_angular_drag(mut self, d: f32) -> Self {
        self.angular_drag = d;
        self
    }

    pub fn get_body_type(&self) -> BodyType {
        self.body_type
    }
}

/// The type of a rigid body determines how it is treated in physics updates.
#[derive(Clone, Copy)]
pub enum BodyType {
    /// The default type of body; responds to collision forces.
    Dynamic,
    /// Does not respond to collision forces but can move by having its velocity set.
    Kinematic,
    /// Does not respond to collision forces and cannot move.
    Static,
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

    pub fn get(&self) -> f32 {
        self.mass
    }

    pub fn get_inv(&self) -> f32 {
        self.inverse
    }
}
