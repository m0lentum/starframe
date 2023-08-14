//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    math as m,
    physics::{collision::ROPE_LAYER, Body, BodyKey, Collider, EntitySet, PhysicsMaterial},
};

use thunderdome as td;

//

/// Parameters for constructing a [`Rope`][self::Rope].
#[derive(Clone, Copy, Debug)]
pub struct RopeParameters {
    pub spacing: f64,
    pub thickness: f64,
    pub compliance: f64,
    pub bending_max_angle: f64,
    pub bending_compliance: f64,
    pub damping: f64,
    pub material: PhysicsMaterial,
    pub particle_mass: f64,
}
impl Default for RopeParameters {
    fn default() -> Self {
        Self {
            spacing: 0.1,
            thickness: 0.12,
            compliance: 0.0000001,
            bending_max_angle: m::Angle::Deg(30.0).rad(),
            bending_compliance: 0.2,
            damping: 20.0,
            material: PhysicsMaterial {
                static_friction_coef: None,
                dynamic_friction_coef: Some(1.5),
                restitution_coef: 0.0,
            },
            particle_mass: 0.02,
        }
    }
}

/// A rope built out of connected particles with circular colliders.
#[derive(Clone, Default, Debug)]
pub struct Rope {
    pub params: RopeParameters,
    pub particles: Vec<BodyKey>,
}

impl Rope {
    /// Spawn a rope in the shape of the line, adjusting spacing so that a particle lands on both
    /// the start and end points.
    pub fn spawn_line(
        mut params: RopeParameters,
        start: m::Vec2,
        end: m::Vec2,
        entities: &mut EntitySet,
    ) -> Self {
        let dist = end - start;
        let dist_mag = dist.mag();
        let segment_count = (dist_mag / params.spacing).round() as usize;
        let particle_count = segment_count + 1;
        params.spacing = dist_mag / segment_count as f64;
        let dir = dist / dist_mag;
        let step = params.spacing * dir;

        let mut particles = Vec::new();
        Self::build_line(
            &mut particles,
            &params,
            start,
            step,
            particle_count,
            entities,
        );

        Rope { params, particles }
    }

    /// Add `count` particles to the end of an existing rope in a line.
    pub fn extend_line(&mut self, dir: m::Unit<m::Vec2>, count: usize, graph: &mut EntitySet) {
        let Some(&last_particle) = self.particles.iter().last().and_then(|key| graph.bodies.get(key.0)) else { return; };

        let step = *dir * self.params.spacing;
        let first_new_pos = last_particle.pose.translation + step;

        Self::build_line(
            &mut self.particles,
            &self.params,
            first_new_pos,
            step,
            count,
            graph,
        );
    }

    /// Spawn `count` particles in a line, pushing them to the given Vec.
    fn build_line(
        particles: &mut Vec<BodyKey>,
        params: &RopeParameters,
        start: m::Vec2,
        step: m::Vec2,
        count: usize,
        entities: &mut EntitySet,
    ) {
        let body_proto = Body::new_particle(params.particle_mass);
        let collider_proto = Collider::new_circle(params.thickness / 2.0)
            .with_layer(ROPE_LAYER)
            .with_material(params.material);

        let mut next_pos: m::Vec2 = start;
        for _ in 0..count {
            let body = Body {
                pose: m::Pose::new(next_pos, Default::default()),
                ..body_proto
            };
            let body_key = entities.insert_body(body);
            entities.attach_collider(body_key, collider_proto);
            particles.push(body_key);

            next_pos += step;
        }
    }

    /// Split a rope into two after the given particle.
    ///
    /// Returns the [`Rope`][crate::physics::rope::Rope] nodes of the two rope parts
    /// if the particle is part of a rope and not its last particle,
    /// otherwise returns `None`.
    pub fn cut_after(mut self, particle: BodyKey) -> Option<[Rope; 2]> {
        let particle_idx = self
            .particles
            .iter()
            .enumerate()
            .find(|(_, p)| **p == particle)
            .map(|(i, _)| i)?;
        if particle_idx >= self.particles.len() {
            None
        } else {
            let cut_particles = self.particles.split_off(particle_idx + 1);
            let cut_rope = Rope {
                params: self.params,
                particles: cut_particles,
            };
            Some([self, cut_rope])
        }
    }
}

/// Key type to look up a rope stored in the physics world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RopeKey(pub(super) td::Index);

/// Manager struct holding ropes inside of a physics world.
#[derive(Clone, Debug, Default)]
pub struct RopeSet {
    pub(super) ropes: td::Arena<Rope>,
}

impl RopeSet {
    #[inline]
    pub(super) fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn insert(&mut self, rope: Rope) -> RopeKey {
        RopeKey(self.ropes.insert(rope))
    }

    /// Access a Rope in the physics world, if it still exists.
    #[inline]
    pub fn get(&self, key: RopeKey) -> Option<&Rope> {
        self.ropes.get(key.0)
    }

    /// Mutably access a Rope in the physics world, if it still exists.
    #[inline]
    pub fn get_mut(&mut self, key: RopeKey) -> Option<&mut Rope> {
        self.ropes.get_mut(key.0)
    }

    /// Remove a Rope and all its particles from the physics world.
    #[inline]
    pub fn remove(&mut self, key: RopeKey, entities: &mut EntitySet) {
        let Some(rope) = self.ropes.remove(key.0) else { return };
        for particle in rope.particles {
            entities.remove_body(particle);
        }
    }

    /// Remove all ropes. Does NOT remove the particles,
    /// and should only be used during full clear of the physics world,
    /// hence only pub(super)
    #[inline]
    pub(super) fn clear(&mut self) {
        self.ropes.clear();
    }
}
