use super::Velocity;

#[derive(Clone, Copy)]
pub struct RigidBody {
    body_type: BodyType,
    mass: Mass,
    // TODO: moment of inertia (calculated from collider)
    elasticity: f32,
    // TODO: friction
    drag: f32,
    angular_drag: f32,

    pub velocity: Velocity,
}

impl Default for RigidBody {
    fn default() -> Self {
        RigidBody {
            body_type: BodyType::Dynamic,
            mass: Mass::mass(1.0),
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
    pub fn with_mass(mut self, mass: Mass) -> Self {
        self.mass = mass;
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
