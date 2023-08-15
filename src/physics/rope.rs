//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    math as m,
    physics::{
        collision::ROPE_LAYER, Body, BodyKey, Collider, ColliderKey, EntitySet, PhysicsMaterial,
    },
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
    pub particles: Vec<RopeParticle>,
}

#[derive(Clone, Copy, Debug)]
pub struct RopeParticle {
    pub body: BodyKey,
    pub collider: ColliderKey,
}

impl Rope {
    /// Spawn a rope in the shape of the line, adjusting spacing so that a particle lands on both
    /// the start and end points.
    pub fn spawn_line(
        mut params: RopeParameters,
        start: m::Vec2,
        end: m::Vec2,
        entity_set: &mut EntitySet,
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
            entity_set,
        );

        Rope { params, particles }
    }

    /// Add `count` particles to the end of an existing rope in a line.
    pub fn extend_line(&mut self, dir: m::Unit<m::Vec2>, count: usize, entity_set: &mut EntitySet) {
        let Some(&last_particle) = self
            .particles
            .iter()
            .last()
            .and_then(|p| entity_set.bodies.get(p.body.0)) else { return; };

        let step = *dir * self.params.spacing;
        let first_new_pos = last_particle.pose.translation + step;

        Self::build_line(
            &mut self.particles,
            &self.params,
            first_new_pos,
            step,
            count,
            entity_set,
        );
    }

    /// Spawn `count` particles in a line, pushing them to the given Vec.
    fn build_line(
        particles: &mut Vec<RopeParticle>,
        params: &RopeParameters,
        start: m::Vec2,
        step: m::Vec2,
        count: usize,
        entity_set: &mut EntitySet,
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
            let body_key = entity_set.insert_body(body);
            let collider_key = entity_set.attach_collider(body_key, collider_proto);
            particles.push(RopeParticle {
                body: body_key,
                collider: collider_key,
            });

            next_pos += step;
        }
    }

    /// Split a rope into two after the given particle.
    ///
    /// Returns the newly created rope if the particle was part of the rope
    /// and there were more particles after it, otherwise returns `None`.
    pub fn cut_after(&mut self, particle: BodyKey) -> Option<Rope> {
        let particle_idx = self
            .particles
            .iter()
            .enumerate()
            .find(|(_, p)| p.body == particle)
            .map(|(i, _)| i)?;
        if particle_idx == self.particles.len() - 1 {
            self.particles.pop();
            None
        } else {
            let cut_particles = self.particles.split_off(particle_idx + 1);
            let cut_rope = Rope {
                params: self.params,
                particles: cut_particles,
            };
            Some(cut_rope)
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
    pub fn remove(&mut self, key: RopeKey, entity_set: &mut EntitySet) {
        let Some(rope) = self.ropes.remove(key.0) else { return };
        for particle in rope.particles {
            entity_set.remove_body(particle.body);
        }
    }

    /// Remove all ropes. Does NOT remove the particles,
    /// and should only be used during full clear of the physics world,
    /// hence only pub(super)
    #[inline]
    pub(super) fn clear(&mut self) {
        self.ropes.clear();
    }

    /// If individual particles have been removed,
    /// take them out of the ropes they were part of
    /// and cut the ropes into parts.
    ///
    /// TODO: consider a generic-constraint-based implementation
    /// that would do this automatically
    pub(super) fn remove_dead_particles(&mut self, entity_set: &mut EntitySet) {
        let mut new_ropes: Vec<Rope> = Vec::new();
        for (_, rope) in self.ropes.iter_mut() {
            // tricky logic to mutate either the existing rope or a newly cut-off one.
            // certainly not the cleanest way to do this
            // but I want to redesign this whole thing anyway
            let mut queued_rope: Option<Rope> = None;
            'curr_rope: loop {
                let editing_rope = queued_rope.as_mut().unwrap_or(rope);
                let Some(removed_particle_idx) =
                    editing_rope
                        .particles
                        .iter()
                        .enumerate()
                        .find(|(_, p)| !entity_set.bodies.contains(p.body.0))
                        .map(|(i, _)| i) else { break 'curr_rope };
                // take the particles after the removed one into a new rope,
                // removing any consecutive dead particles,
                // then repeat this process for the new rope
                let cut_particles: Vec<RopeParticle> = editing_rope
                    .particles
                    .iter()
                    .skip(removed_particle_idx + 1)
                    .skip_while(|p| !entity_set.bodies.contains(p.body.0))
                    .cloned()
                    .collect();
                editing_rope.particles.truncate(removed_particle_idx);
                if cut_particles.is_empty() {
                    break 'curr_rope;
                }

                if let Some(q) = queued_rope.take() {
                    new_ropes.push(q);
                }
                queued_rope = Some(Rope {
                    particles: cut_particles,
                    params: rope.params,
                });
            }
            if let Some(q) = queued_rope.take() {
                new_ropes.push(q);
            }
        }

        for new_rope in new_ropes {
            self.insert(new_rope);
        }

        // delete ropes with no particles
        let to_delete: Vec<td::Index> = self
            .ropes
            .iter()
            .filter(|(_, rope)| rope.particles.is_empty())
            .map(|(idx, _)| idx)
            .collect();
        for rope_idx in to_delete {
            self.ropes.remove(rope_idx);
        }
    }
}
