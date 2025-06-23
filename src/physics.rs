use itertools::izip;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::math::{left_normal, uv, PhysicsPose, UnitDVec2};

//

pub mod collision;
use collision::bvh::Bvh;
pub use collision::{
    Collider, ColliderPolygon, ColliderShape, ColliderType, CollisionLayerMask, Contact,
    ContactResult, PhysicsMaterial, Ray,
};

pub(super) mod constraint;
use constraint::ConstraintTargets;
pub use constraint::{Constraint, ConstraintBuilder, ConstraintType};

pub mod forcefield;
pub use forcefield::ForceField;

pub(super) mod body;
pub use body::{Body, ColliderInfo};

mod rope;
pub use rope::{Rope, RopeKey, RopeParameters, RopeSet};

mod entity_set;
pub use entity_set::{BodyKey, ColliderKey, EntitySet};

mod constraint_set;
pub use constraint_set::{ConstraintKey, ConstraintSet};

mod constraint_graph;
use constraint_graph::*;

mod solver;

pub mod hecs_sync;

//
// public types
//

/// Linear and angular velocity of an object.
#[derive(Copy, Clone, Debug)]
pub struct Velocity {
    /// Linear velocity in metres per second.
    pub linear: uv::DVec2,
    /// Angular velocity in radians per second.
    pub angular: f64,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: uv::DVec2::zero(),
            angular: 0.0,
        }
    }
}

impl Velocity {
    #[inline]
    pub fn mag_sq(&self) -> f64 {
        self.linear.mag_sq() + self.angular * self.angular
    }

    /// Get the linear velocity of a point offset from the center of mass.
    #[inline]
    pub fn point_velocity(&self, offset: uv::DVec2) -> uv::DVec2 {
        let tangent = left_normal(offset) * self.angular;
        self.linear + tangent
    }

    #[inline]
    pub fn apply_to_pose(&self, dt: f64, mut pose: PhysicsPose) -> PhysicsPose {
        let scaled = *self * dt;
        pose.append_translation(scaled.linear);
        pose.prepend_rotation(uv::DRotor2::from_angle(scaled.angular));
        pose
    }
}

impl std::ops::Add for Velocity {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            linear: self.linear + other.linear,
            angular: self.angular + other.angular,
        }
    }
}
impl std::ops::AddAssign for Velocity {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}
impl std::ops::Mul<f64> for Velocity {
    type Output = Velocity;

    fn mul(self, rhs: f64) -> Self::Output {
        Velocity {
            linear: self.linear * rhs,
            angular: self.angular * rhs,
        }
    }
}

/// Pertinent information about a contact between two colliders.
#[derive(Clone, Copy, Debug)]
pub struct ContactInfo {
    pub colliders: [ColliderKey; 2],
    pub normal: UnitDVec2,
    // island id stored to allow retaining of sleeping contacts
    island_id: IslandId,
}
impl ContactInfo {
    pub(self) fn flip(self) -> Self {
        Self {
            colliders: [self.colliders[1], self.colliders[0]],
            normal: -self.normal,
            island_id: self.island_id,
        }
    }
}

/// Result of a [`raycast`][self::PhysicsWorld::raycast]
/// or [`spherecast`][self::PhysicsWorld::spherecast].
#[derive(Clone, Copy, Debug)]
pub struct CastHit {
    /// A key to the collider that was hit.
    pub collider: ColliderKey,
    /// The point in world space where the ray or swept shape intersected the collider.
    ///
    /// This is always a point on the hit collider's surface.
    /// To get the center point of the swept shape on impact, use `ray.point_at_t(hit.t)`.
    pub point: uv::DVec2,
    /// The surface normal of the collider at the point of impact.
    pub normal: UnitDVec2,
    /// The parameter `t` in the ray equation `start + t * dir`
    /// for the point where the ray or swept shape intersected the collider.
    ///
    /// Useful with the ray's [`point_at_t`][self::Ray::point_at_t] method,
    /// e.g. if you want to step backward along the ray from the contact point
    /// or sample the whole distance travelled by the ray.
    pub t: f64,
}

//
// internal types
//

/// A collection of bodies that's disjoint (in terms of constraints and contacts)
/// from every body outside of it.
#[derive(Clone, Copy, Debug)]
struct Island {
    id: IslandId,
    // bodies, ropes, constraints and collider pairs belonging to the island,
    // stored in sorted_* in WorkingBuffers
    body_range_start: usize,
    body_count: usize,
    rope_range_start: usize,
    rope_count: usize,
    constr_range_start: usize,
    constr_count: usize,
    pair_range_start: usize,
    pair_count: usize,
    // some kinds of bodies (particles, mainly) may not ever want to sleep
    can_sleep: bool,
}

/// Information to identify an island
/// and check that its topology hasn't changed with reasonable confidence.
///
/// Used to set idle islands to sleep and keep them that way until they change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IslandId {
    first_body: usize,
    // a "checksum" of all constraint graph edges in this island,
    // used to identify islands that haven't changed between ticks
    edge_sum: usize,
}
#[derive(Clone, Copy, Debug)]
struct SleepingIsland {
    id: IslandId,
    /// flag to allow removing islands that no longer exist
    continues_sleeping: bool,
    /// islands are monitored for a little while before actually skipping computing them
    /// to avoid situations where constraints are working but velocity is briefly zero
    ticks_slept: usize,
}
impl From<IslandId> for SleepingIsland {
    fn from(id: IslandId) -> Self {
        Self {
            id,
            continues_sleeping: false,
            ticks_slept: 0,
        }
    }
}

/// Cached buffers to avoid allocating a bunch of memory every frame.
/// Explanations in `tick` where populated
#[derive(Default)]
struct WorkingBuffers {
    // indices sorted by island for efficient island graph formation
    // without individual Vecs for each island.
    // two passes needed to first gather islands and then sort the islands
    sorted_first_pass: SortedIndices,
    sorted_second_pass: SortedIndices,
    island_assigned: Vec<bool>,
    islands: Vec<Island>,
    // islands grouped roughly evenly for efficient threading
    island_group_sizes: Vec<usize>,

    // constraints collected into a vec so they can be indexed
    // without iterating a slotmap
    constraints: Vec<Constraint>,
    sorted_constraints: Vec<Constraint>,
    constraint_stiffnesses: Vec<f64>,
    constraint_lagrange_mults: Vec<f64>,

    sorted_coll_pairs: Vec<[ColliderKey; 2]>,
    contact_stiffnesses: Vec<f64>,
    contact_lagrange_mults: Vec<f64>,

    // bodies, sorted in island order
    bodies: Vec<Body>,
    // map from body keys to their position in the sorted buffer
    body_order: Vec<usize>,
    rope_next_particles: Vec<Option<usize>>,
    rope_prev_particles: Vec<Option<usize>>,
    rope_lateral_corrections: Vec<Option<uv::DVec2>>,

    old_poses: Vec<PhysicsPose>,
    inertial_poses: Vec<PhysicsPose>,
    pre_contact_poses: Vec<PhysicsPose>,
    old_velocities: Vec<Velocity>,
    ext_f_accelerations: Vec<uv::DVec2>,

    constraint_body_pairs: Vec<(usize, Option<usize>)>,
    coll_pair_keys: Vec<[ColliderKey; 2]>,
    contacts: Vec<ContactResult>,
    last_contacts: Vec<ContactResult>,
    contact_angles: Vec<uv::DRotor2>,
    contact_lambdas: Vec<f64>,
}

#[derive(Default)]
struct SortedIndices {
    bodies: Vec<usize>,
    ropes: Vec<usize>,
    constraints: Vec<usize>,
    coll_pairs: Vec<usize>,
}
impl SortedIndices {
    fn clear(&mut self) {
        self.bodies.clear();
        self.ropes.clear();
        self.constraints.clear();
        self.coll_pairs.clear();
    }
}

//
// physics proper
//

/// Constants used to adjust various features of the physics solver.
///
/// Start with `Default::default()` and adjust as needed.
#[derive(Clone, Copy, Debug)]
pub struct TuningConstants {
    /// The number of solver iterations per frame.
    ///
    /// Higher is more expensive and more accurate.
    pub iterations: usize,
    /// Stiffness value to start AVBD iterations.
    pub min_stiffness: f64,
    /// Maximum stiffness.
    pub max_stiffness: f64,
    /// Maximum Lagrange multiplier on a force.
    pub max_lambda: f64,
    /// Rate of increasing stiffness between iterations, β in the AVDB paper.
    /// Range: `[0, )`.
    pub stiffness_growth_coef: f64,
    /// Proportion of a hard constraint's value removed from subsequent iterations,
    /// α in the AVDB paper.
    /// Range: [0, 1], 1 lets constraints remain violated,
    /// 0 makes potentially explosive corrections.
    pub regularization_rate: f64,
    /// Maximum velocity of a body to be considered at rest.
    pub sleep_vel_threshold: f64,
    /// Number of frames (not substeps) before an island where every body is at rest
    /// is set to sleep.
    ///
    /// Should be more than 1 to avoid situations where something is set to sleep when it
    /// briefly stops but isn't at rest.
    pub fall_asleep_frames: usize,
    /// Highest acceleration expected to happen over a frame,
    /// used to ensure all collisions are detected in every substep.
    ///
    /// If higher accelerations occur under just the right conditions,
    /// this can cause a missed collision, leading to a deep collision the next frame
    /// and bodies flying apart violently.
    pub max_expected_acceleration: f64,
    #[cfg(feature = "parallel")]
    /// Minimum limit for bodies per thread to make sure work is divided efficiently.
    pub min_bodies_per_thread: usize,
}

impl Default for TuningConstants {
    fn default() -> Self {
        Self {
            iterations: 5,
            sleep_vel_threshold: 0.001,
            min_stiffness: 10.,
            max_stiffness: 1e5,
            max_lambda: 1e4,
            stiffness_growth_coef: 1000.,
            regularization_rate: 0.95,
            fall_asleep_frames: 10,
            max_expected_acceleration: 10.0,
            #[cfg(feature = "parallel")]
            min_bodies_per_thread: 64,
        }
    }
}

/// The top-level container for all the data the physics engine uses.
pub struct PhysicsWorld {
    pub consts: TuningConstants,
    pub mask_matrix: collision::CollisionMaskMatrix,
    pub entity_set: EntitySet,
    pub rope_set: RopeSet,
    pub constraint_set: ConstraintSet,
    pub(crate) bvh: Bvh,
    constraint_graph: ConstraintGraph,
    sleeping_islands: Vec<SleepingIsland>,
    working_bufs: WorkingBuffers,
    contacts: Vec<ContactInfo>,
}

impl PhysicsWorld {
    pub fn new(consts: TuningConstants, mask_matrix: collision::CollisionMaskMatrix) -> Self {
        PhysicsWorld {
            consts,
            mask_matrix,
            entity_set: EntitySet::new(),
            rope_set: RopeSet::new(),
            constraint_set: ConstraintSet::new(),
            bvh: Bvh::new(),
            constraint_graph: ConstraintGraph {
                first_nodes_per_body: Vec::new(),
                last_nodes_per_body: Vec::new(),
                nodes: Vec::new(),
            },
            sleeping_islands: Vec::new(),
            working_bufs: WorkingBuffers::default(),
            contacts: Vec::new(),
        }
    }

    /// Remove all constraints and reset internal state.
    pub fn clear(&mut self) {
        self.entity_set.clear();
        self.rope_set.clear();
        self.constraint_set.clear();
        self.sleeping_islands.clear();
        self.contacts.clear();
        self.working_bufs = WorkingBuffers::default();
    }

    /// Advance the simulation forward by `frame_dt` seconds.
    pub fn tick(&mut self, frame_dt: f64, time_scale: Option<f64>, forcefield: &impl ForceField) {
        let _main_span = tracy_client::span!("physics tick");

        self.entity_set.remove_orphan_colliders();
        self.rope_set.remove_dead_particles(&mut self.entity_set);

        let dt = frame_dt * time_scale.unwrap_or(1.);

        let bufs = &mut self.working_bufs;

        // remove constraints where one or both participating bodies have been destroyed
        self.constraint_set.constraints.retain(|_, c| {
            c.target
                .iter()
                .all(|body| self.entity_set.get_body(body).is_some())
        });
        // TODO: probably shouldn't be cloning constraints here
        bufs.constraints.clear();
        bufs.constraints.extend(
            self.constraint_set
                .constraints
                .iter()
                .map(|(_, v)| v.clone()),
        );

        //
        // Prepare the spatial index
        //

        let spi_span = tracy_client::span!("build spatial index");

        // constant for padding bounding volumes to fit movement during substeps,
        // collisions may be missed if higher accelerations occur
        let max_expected_accel_over_frame = self.consts.max_expected_acceleration * frame_dt;

        self.bvh.clear();
        bufs.coll_pair_keys.clear();
        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep.
        for (coll_key, coll) in self.entity_set.colliders.iter() {
            let coll_key = ColliderKey(coll_key);
            let body = self.entity_set.get_collider_body(coll_key);
            let aabb = match body {
                Some(body) => {
                    let pose = body.pose * coll.pose;
                    coll.shape
                        .aabb(pose)
                        .extended(body.velocity.linear * frame_dt)
                        .padded(max_expected_accel_over_frame)
                }
                None => coll.shape.aabb(coll.pose),
            };

            bufs.coll_pair_keys.extend(
                self.bvh
                    .test_aabb(aabb)
                    .filter(|other| {
                        self.mask_matrix.get(
                            coll.layer,
                            // unwrap is safe here because we rebuild the BVH every frame,
                            // hence nothing has had the opportunity to be deleted at this point
                            self.entity_set.get_collider(*other).unwrap().layer,
                        )
                    })
                    .map(move |other| [coll_key, other]),
            );
            self.bvh.insert(coll_key, aabb);
        }

        tracy_client::plot!("colliders", self.entity_set.colliders.len() as f64);
        tracy_client::plot!("collider pairs tested", bufs.coll_pair_keys.len() as f64);

        drop(spi_span);

        //
        // Build constraint graph
        //

        let constr_graph_span = tracy_client::span!("build constraint graph");

        self.constraint_graph.clear();
        self.constraint_graph
            .resize(self.entity_set.body_slot_count);

        // constraints

        for (constr_idx, constr) in bufs.constraints.iter().enumerate() {
            for (instance_idx, body_key) in constr.target.iter().enumerate() {
                if !matches!(constr.target, ConstraintTargets::Single(_)) {
                    for other_body_key in constr.target.iter().filter(|other| *other != body_key) {
                        self.constraint_graph.insert(
                            body_key.0.slot() as usize,
                            ConstraintGraphEdge::Constraint {
                                other_body_idx: Some(other_body_key.0.slot() as usize),
                                constr_idx,
                                instance_idx,
                            },
                        );
                    }
                } else {
                    self.constraint_graph.insert(
                        body_key.0.slot() as usize,
                        ConstraintGraphEdge::Constraint {
                            other_body_idx: None,
                            constr_idx,
                            instance_idx,
                        },
                    );
                }
            }
        }

        // potential contacts from spatial index.
        // this doesn't necessarily cull as much as actually checking collisions,
        // but that would require redoing this every substep which would be costly.
        for (pair_idx, pair) in bufs.coll_pair_keys.iter().enumerate() {
            match pair.map(|ci| {
                self.entity_set
                    .coll_bodies
                    .get(ci.0)
                    .map(|b| b.0.slot() as usize)
            }) {
                [Some(b1), Some(b2)] => {
                    if b1 == b2 {
                        // both colliders are part of the same compound collider,
                        // skip tests between them
                        continue;
                    }
                    self.constraint_graph.insert(
                        b1,
                        ConstraintGraphEdge::Contact {
                            other_body_idx: Some(b2),
                            pair_idx,
                            instance_idx: 0,
                        },
                    );
                    self.constraint_graph.insert(
                        b2,
                        ConstraintGraphEdge::Contact {
                            other_body_idx: Some(b1),
                            pair_idx,
                            instance_idx: 1,
                        },
                    );
                }
                [Some(b1), None] => {
                    self.constraint_graph.insert(
                        b1,
                        ConstraintGraphEdge::Contact {
                            other_body_idx: None,
                            pair_idx,
                            instance_idx: 0,
                        },
                    );
                }
                [None, Some(b2)] => {
                    self.constraint_graph.insert(
                        b2,
                        ConstraintGraphEdge::Contact {
                            other_body_idx: None,
                            pair_idx,
                            instance_idx: 1,
                        },
                    );
                }
                [None, None] => {}
            }
        }

        drop(constr_graph_span);

        //
        // Generate islands from graph
        //

        bufs.island_assigned.clear();
        bufs.island_assigned
            .resize(self.entity_set.body_slot_count, false);
        bufs.islands.clear();
        bufs.sorted_first_pass.clear();
        bufs.sorted_second_pass.clear();

        let island_span = tracy_client::span!("build islands");

        fn search(
            root_body_idx: usize,
            island: &mut Island,
            constraint_graph: &ConstraintGraph,
            bufs: &mut WorkingBuffers,
        ) {
            if bufs.island_assigned[root_body_idx] {
                return;
            }
            bufs.island_assigned[root_body_idx] = true;
            bufs.sorted_first_pass.bodies.push(root_body_idx);
            island.body_count += 1;
            for edge in constraint_graph.body_iter(root_body_idx) {
                match edge {
                    ConstraintGraphEdge::Constraint {
                        other_body_idx,
                        constr_idx,
                        ..
                    } => {
                        bufs.sorted_first_pass.constraints.push(constr_idx);
                        island.constr_count += 1;

                        if !bufs.constraints[constr_idx].can_sleep {
                            island.can_sleep = false;
                        }
                        if let Some(other_idx) = other_body_idx {
                            // add 1 to root_body_idx so that we never get zero from this
                            // (which would essentially allow a constraint to be added
                            // to the island without changing its identity,
                            // causing bugs in the vicinity of body index 0)
                            island.id.edge_sum += (root_body_idx + 1) * (other_idx + 1);

                            search(other_idx, island, constraint_graph, bufs);
                        } else {
                            // no guarantee constr_idx is stable between frames,
                            // but we still need to stop sleeping when any constraint changes.
                            // adding a root_body_idx should do the job
                            island.id.edge_sum += root_body_idx + 1;
                        }
                    }
                    ConstraintGraphEdge::Contact {
                        other_body_idx,
                        pair_idx,
                        ..
                    } => {
                        bufs.sorted_first_pass.coll_pairs.push(pair_idx);
                        island.pair_count += 1;

                        if let Some(other_idx) = other_body_idx {
                            island.id.edge_sum += (root_body_idx + 1) * (other_idx + 1);

                            search(other_idx, island, constraint_graph, bufs);
                        } else {
                            island.id.edge_sum += root_body_idx + 1;
                        }
                    }
                }
            }
        }

        for (body_key, _) in self.entity_set.bodies.iter() {
            if bufs.island_assigned[body_key.slot() as usize] {
                continue;
            }
            let mut island = Island {
                id: IslandId {
                    first_body: body_key.slot() as usize,
                    edge_sum: 0, // this is incremented during search
                },
                can_sleep: true,
                body_range_start: bufs.sorted_first_pass.bodies.len(),
                body_count: 0,
                rope_range_start: bufs.sorted_first_pass.ropes.len(),
                rope_count: 0,
                constr_range_start: bufs.sorted_first_pass.constraints.len(),
                constr_count: 0,
                pair_range_start: bufs.sorted_first_pass.coll_pairs.len(),
                pair_count: 0,
            };
            search(
                body_key.slot() as usize,
                &mut island,
                &self.constraint_graph,
                bufs,
            );
            bufs.islands.push(island);
        }

        //
        // sort islands by size and handle sleeping
        //

        for sleeping in &mut self.sleeping_islands {
            sleeping.continues_sleeping = false;
        }
        // remove sleeping islands from computation and set them to keep sleeping
        bufs.islands.retain(|isl| {
            if let Some(sleeping) = self
                .sleeping_islands
                .iter_mut()
                .find(|slep| slep.id == isl.id)
            {
                // we need to check if anything started moving between frames due to user code
                if bufs.sorted_first_pass.bodies
                    [isl.body_range_start..isl.body_range_start + isl.body_count]
                    .iter()
                    .any(|bi| {
                        let (_, body) = self.entity_set.bodies.get_by_slot(*bi as u32).unwrap();
                        body.velocity.mag_sq() >= self.consts.sleep_vel_threshold
                    })
                {
                    return true;
                }

                sleeping.continues_sleeping = true;
                // keep island in computations if it hasn't slept for long enough
                sleeping.ticks_slept < self.consts.fall_asleep_frames
            } else {
                true
            }
        });
        // remove sleeping island ids that weren't found
        self.sleeping_islands.retain(|slep| slep.continues_sleeping);
        // sort remaining islands by size for better work distribution over threads
        bufs.islands
            .sort_unstable_by_key(|isl| usize::MAX - isl.body_count);
        // move indices associated with islands according to sorted island order
        for isl in &mut bufs.islands {
            let new_body_start = bufs.sorted_second_pass.bodies.len();
            bufs.sorted_second_pass.bodies.extend_from_slice(
                &bufs.sorted_first_pass.bodies
                    [isl.body_range_start..isl.body_range_start + isl.body_count],
            );
            isl.body_range_start = new_body_start;

            let new_rope_start = bufs.sorted_second_pass.ropes.len();
            bufs.sorted_second_pass.ropes.extend_from_slice(
                &bufs.sorted_first_pass.ropes
                    [isl.rope_range_start..isl.rope_range_start + isl.rope_count],
            );
            isl.rope_range_start = new_rope_start;

            let new_constr_start = bufs.sorted_second_pass.constraints.len();
            bufs.sorted_second_pass.constraints.extend_from_slice(
                &bufs.sorted_first_pass.constraints
                    [isl.constr_range_start..isl.constr_range_start + isl.constr_count],
            );
            isl.constr_range_start = new_constr_start;

            let new_pair_start = bufs.sorted_second_pass.coll_pairs.len();
            bufs.sorted_second_pass.coll_pairs.extend_from_slice(
                &bufs.sorted_first_pass.coll_pairs
                    [isl.pair_range_start..isl.pair_range_start + isl.pair_count],
            );
            isl.pair_range_start = new_pair_start;
        }

        drop(island_span);

        //
        // Populate working buffers
        //

        let buf_span = tracy_client::span!("populate buffers");

        bufs.bodies.clear();
        bufs.bodies.extend(
            bufs.sorted_second_pass
                .bodies
                .iter()
                .map(|bi| self.entity_set.bodies.get_by_slot(*bi as u32).unwrap().1),
        );
        // maps from the slot of a body in the thunderdome arena
        // to the index of a body in bufs.bodies.
        // usize::MAX indicates slots that were sleeping.
        // could use Option here, but the only time this is relevant is at the end of the step
        // when bodies are synced back into self.entity_set.
        // in the solver it's guaranteed all bodies are live,
        // and unwrapping Options everywhere would be annoying
        bufs.body_order.clear();
        bufs.body_order
            .resize(self.entity_set.body_slot_count, usize::MAX);
        for (sorted_idx, body_slot) in bufs.sorted_second_pass.bodies.iter().enumerate() {
            bufs.body_order[*body_slot] = sorted_idx;
        }

        // TODO: probably don't need to be cloning constraints here
        bufs.sorted_constraints.clear();
        bufs.sorted_constraints.extend(
            bufs.sorted_second_pass
                .constraints
                .iter()
                .map(|&ci| bufs.constraints[ci].clone()),
        );

        // store indices into neighboring particles for rope nodes
        bufs.rope_next_particles.clear();
        bufs.rope_next_particles.resize(bufs.bodies.len(), None);
        bufs.rope_prev_particles.clear();
        bufs.rope_prev_particles.resize(bufs.bodies.len(), None);
        for (_, rope) in self.rope_set.ropes.iter() {
            let mut iter = rope.particles.iter().peekable();
            while let Some(particle) = iter.next() {
                if let Some(next_particle) = iter.peek() {
                    let body_idx = bufs.body_order[particle.body.0.slot() as usize];
                    let next_body_idx = bufs.body_order[next_particle.body.0.slot() as usize];
                    bufs.rope_next_particles[body_idx] = Some(next_body_idx);
                    bufs.rope_prev_particles[next_body_idx] = Some(body_idx);
                }
            }
        }
        // store lateral position corrections (bending resistance) for velocity correction
        bufs.rope_lateral_corrections.clear();
        bufs.rope_lateral_corrections
            .resize(bufs.sorted_second_pass.bodies.len(), None);

        bufs.old_poses.clear();
        bufs.old_poses.extend(bufs.bodies.iter().map(|b| b.pose));
        bufs.inertial_poses.clear();
        bufs.inertial_poses
            .resize(bufs.bodies.len(), PhysicsPose::identity());
        // poses after velocity and constraints are applied, used for rope normal correction
        bufs.pre_contact_poses.clear();
        bufs.pre_contact_poses.extend_from_slice(&bufs.old_poses);
        // old velocities used for restitution
        bufs.old_velocities.clear();
        bufs.old_velocities
            .extend(bufs.bodies.iter().map(|b| b.velocity));

        // accelerations from external forces used as a speed limit for restitution
        bufs.ext_f_accelerations.clear();
        bufs.ext_f_accelerations
            .resize(bufs.sorted_second_pass.bodies.len(), uv::DVec2::default());

        bufs.sorted_coll_pairs.clear();
        bufs.sorted_coll_pairs.extend(
            bufs.sorted_second_pass
                .coll_pairs
                .iter()
                .map(|pi| bufs.coll_pair_keys[*pi]),
        );
        // store latest contacts for use in the velocity step
        bufs.contacts.clear();
        bufs.contacts
            .resize(bufs.sorted_coll_pairs.len(), ContactResult::Zero);
        // collect pairs that had contacts for sending events after solving everything
        bufs.last_contacts.clear();
        bufs.last_contacts
            .resize(bufs.sorted_coll_pairs.len(), ContactResult::Zero);
        bufs.contact_angles.clear();
        bufs.contact_angles
            .resize(bufs.sorted_coll_pairs.len(), uv::DRotor2::identity());
        // store contact forces for friction purposes
        bufs.contact_lambdas.clear();
        bufs.contact_lambdas
            .resize(bufs.sorted_coll_pairs.len(), 0.0);

        // TODO: warm starting
        // prior to this clear we still have the values from last frame,
        // maybe we could just use that? needs associating to the right constraints though,
        // maybe cleaner to put them in an arena keyed by ConstraintKey / ColliderKey

        bufs.constraint_stiffnesses.clear();
        bufs.constraint_stiffnesses
            .resize(bufs.sorted_constraints.len(), self.consts.min_stiffness);
        bufs.contact_stiffnesses.clear();
        bufs.contact_stiffnesses
            .resize(bufs.sorted_coll_pairs.len(), self.consts.min_stiffness);

        bufs.constraint_lagrange_mults.clear();
        bufs.constraint_lagrange_mults
            .resize(bufs.sorted_constraints.len(), 0.);
        bufs.contact_lagrange_mults.clear();
        bufs.contact_lagrange_mults
            .resize(bufs.sorted_coll_pairs.len(), 0.);

        drop(buf_span);

        //
        // group islands for parallel solving
        //

        bufs.island_group_sizes.clear();

        #[cfg(feature = "parallel")]
        let thread_count = rayon::current_num_threads();
        #[cfg(not(feature = "parallel"))]
        let thread_count = 1;

        #[cfg(feature = "parallel")]
        {
            let ideal_body_count = (self.entity_set.bodies.len() + thread_count - 1) / thread_count;
            let ideal_body_count = ideal_body_count.max(self.consts.min_bodies_per_thread);

            let mut covered_body_count = 0;
            let mut islands_in_group = 0;
            let mut next_split = ideal_body_count;

            let mut island_iter = bufs.islands.iter().peekable();
            while let Some(island) = island_iter.next() {
                let body_count_after = covered_body_count + island.body_count;

                // special case for last island because it has to get pushed no matter what
                if island_iter.peek().is_none() {
                    bufs.island_group_sizes.push(islands_in_group + 1);
                    continue;
                }

                if body_count_after < next_split {
                    islands_in_group += 1;
                } else {
                    // pick the island boundary closer to the ideal
                    if body_count_after - next_split < next_split - covered_body_count {
                        // boundary after this island is closer,
                        // current island goes in current group
                        bufs.island_group_sizes.push(islands_in_group + 1);
                        islands_in_group = 0;
                    } else {
                        // boundary before this island is closer,
                        // current island goes in next group
                        bufs.island_group_sizes.push(islands_in_group);
                        islands_in_group = 1;
                    }

                    // set next split to first one after current island
                    // (may skip some in case one island is larger than two ideal splits)
                    while body_count_after > next_split {
                        next_split += ideal_body_count;
                    }
                }
                covered_body_count = body_count_after;
            }
        }
        #[cfg(not(feature = "parallel"))]
        bufs.island_group_sizes.push(bufs.islands.len());

        //
        // Slice buffers into island-group-specific views
        //

        let mut island_group_views: Vec<solver::DataView<'_>> = Vec::with_capacity(thread_count);

        let mut bodies_s = bufs.bodies.as_mut_slice();
        let mut old_poses_s = bufs.old_poses.as_mut_slice();
        let mut inertial_pose_s = bufs.inertial_poses.as_mut_slice();
        let mut constr_s = bufs.sorted_constraints.as_slice();
        let mut constr_stiff_s = bufs.constraint_stiffnesses.as_mut_slice();
        let mut constr_lambda_s = bufs.constraint_lagrange_mults.as_mut_slice();
        let mut coll_pairs_s = bufs.sorted_coll_pairs.as_mut_slice();
        let mut contacts_s = bufs.contacts.as_mut_slice();
        let mut cont_stiff_s = bufs.contact_stiffnesses.as_mut_slice();
        let mut cont_lambda_s = bufs.contact_lagrange_mults.as_mut_slice();
        let mut last_contacts_s = bufs.last_contacts.as_mut_slice();

        let mut body_index_offset = 0;
        let mut constraint_index_offset = 0;

        for group in bufs
            .island_group_sizes
            .iter()
            .scan(0, |group_start, group_size| {
                let curr_group_start = *group_start;
                *group_start += *group_size;
                Some(&bufs.islands[curr_group_start..*group_start])
            })
        {
            let body_count = group.iter().map(|isl| isl.body_count).sum();
            let constr_count = group.iter().map(|isl| isl.constr_count).sum();
            let pair_count = group.iter().map(|isl| isl.pair_count).sum();

            let (bodies, body_rest) = bodies_s.split_at_mut(body_count);
            bodies_s = body_rest;
            let (old_poses, old_pose_rest) = old_poses_s.split_at_mut(body_count);
            old_poses_s = old_pose_rest;
            let (inertial_poses, inertial_pose_rest) = inertial_pose_s.split_at_mut(body_count);
            inertial_pose_s = inertial_pose_rest;

            let (constraints, constr_rest) = constr_s.split_at(constr_count);
            constr_s = constr_rest;
            let (constraint_stiffnesses, constr_stiff_rest) =
                constr_stiff_s.split_at_mut(constr_count);
            constr_stiff_s = constr_stiff_rest;
            let (constraint_lagrange_mults, constr_lambda_rest) =
                constr_lambda_s.split_at_mut(constr_count);
            constr_lambda_s = constr_lambda_rest;

            let (coll_pairs, coll_p_rest) = coll_pairs_s.split_at_mut(pair_count);
            coll_pairs_s = coll_p_rest;
            let (contacts, contacts_rest) = contacts_s.split_at_mut(pair_count);
            contacts_s = contacts_rest;
            let (last_contacts, last_conts_rest) = last_contacts_s.split_at_mut(pair_count);
            last_contacts_s = last_conts_rest;

            let (contact_stiffnesses, cont_stiff_rest) = cont_stiff_s.split_at_mut(pair_count);
            cont_stiff_s = cont_stiff_rest;
            let (contact_lagrange_mults, cont_lambda_rest) = cont_lambda_s.split_at_mut(pair_count);
            cont_lambda_s = cont_lambda_rest;

            island_group_views.push(solver::DataView {
                dt,
                consts: &self.consts,
                constraint_graph: &self.constraint_graph,
                body_index_offset,
                constraint_index_offset,
                global_body_order: &bufs.body_order,
                bodies,
                old_poses,
                inertial_poses,
                constraints,
                constraint_stiffnesses,
                constraint_lagrange_mults,
                coll_pairs,
                contacts,
                contact_stiffnesses,
                contact_lagrange_mults,
                last_contacts,
            });

            body_index_offset += body_count;
            constraint_index_offset += constr_count;
        }

        //
        // Actual physics step
        //

        #[cfg(feature = "parallel")]
        let island_iter = island_group_views.par_iter_mut();

        #[cfg(not(feature = "parallel"))]
        let island_iter = island_group_views.iter_mut();

        island_iter.for_each(|island_view| {
            let _solve_span = tracy_client::span!("solve");

            solver::solve(forcefield, island_view, &self.entity_set);
        });

        tracy_client::plot!(
            "contacts",
            island_group_views
                .iter()
                .flat_map(|island_view| island_view.last_contacts.iter())
                .filter(|c| !c.is_zero())
                .count() as f64
        );

        //
        // store contacts for user queries and other systems
        //

        self.contacts.retain(|cont| {
            // contacts that are part of sleeping islands are conceptually still there,
            // but not generated because we skip collision detection.
            // keep them in the buffer so they keep getting returned from queries
            // as the user would expect
            self.sleeping_islands.iter().any(|isl| {
                isl.id == cont.island_id && isl.ticks_slept >= self.consts.fall_asleep_frames
            })
        });
        for isl in &bufs.islands {
            self.contacts.extend(
                izip!(
                    &bufs.sorted_coll_pairs
                        [isl.pair_range_start..isl.pair_range_start + isl.pair_count],
                    &bufs.last_contacts
                        [isl.pair_range_start..isl.pair_range_start + isl.pair_count]
                )
                .filter_map(|(pair, contact)| {
                    contact.iter().next().map(|cont| ContactInfo {
                        colliders: *pair,
                        normal: cont.normal,
                        island_id: isl.id,
                    })
                }),
            );
        }

        //
        // set islands where movement was below a threshold to sleep
        //

        for isl in &bufs.islands {
            if isl.can_sleep
                && bufs.bodies[isl.body_range_start..isl.body_range_start + isl.body_count]
                    .iter()
                    .all(|body| body.velocity.mag_sq() < self.consts.sleep_vel_threshold)
            {
                if let Some(already_sleeping) = self
                    .sleeping_islands
                    .iter_mut()
                    .find(|slep| slep.id == isl.id)
                {
                    already_sleeping.ticks_slept += 1;
                } else {
                    self.sleeping_islands.push(isl.id.into());
                }
            }
        }

        //
        // apply results back to retained state
        //

        for (body_key, body) in self.entity_set.bodies.iter_mut() {
            let working_body = bufs.body_order[body_key.slot() as usize];
            if working_body == usize::MAX {
                // this body is sleeping
                continue;
            }
            *body = bufs.bodies[working_body];
        }
    }

    /// Get all contacts that the given collider participated in during the last frame.
    ///
    /// All returned [`ContactInfo`][self::ContactInfo] objects are oriented such that the
    /// collider being searched for is the first item in `colliders` and `normal`
    /// faces away from it.
    pub fn contacts_for_collider(
        &self,
        coll: ColliderKey,
    ) -> impl '_ + Iterator<Item = ContactInfo> {
        self.contacts.iter().filter_map(move |&cont| {
            if cont.colliders[0] == coll {
                Some(cont)
            } else if cont.colliders[1] == coll {
                Some(cont.flip())
            } else {
                None
            }
        })
    }

    /// Find every collider that intersects with the given point.
    /// Returns a key to the collider, and if it's attached to a body,
    /// also a key to the body.
    ///
    /// Takes a mutable reference because bounding volume hierarchy
    /// traversal uses a mutable shared stack.
    /// This is subject to change if I figure out a good way to
    /// do this with interior mutability. (TODO)
    pub fn query_point(
        &mut self,
        point: uv::DVec2,
    ) -> impl '_ + Iterator<Item = (ColliderKey, Option<BodyKey>)> {
        // TODO: using this requires dropping the iterator.
        // restructure this such that references to the collider and body
        // can be acquired during iteration
        let entity_set = &self.entity_set;
        self.bvh.test_point(point).filter_map(move |coll_key| {
            let coll = entity_set.get_collider(coll_key)?;
            let body_key = entity_set.coll_bodies.get(coll_key.0).copied();
            let body = body_key.and_then(|k| entity_set.get_body(k));
            let pose = match body {
                Some(body) => body.pose * coll.pose,
                None => coll.pose,
            };
            if collision::query::point_collider_bool(point, pose, *coll) {
                Some((coll_key, body_key))
            } else {
                None
            }
        })
    }

    /// Get all colliders that intersect with the given shape.
    /// Returns a key to the collider, and if it's attached to a body,
    /// also a key to the body.
    pub fn query_shape<'p, 'g: 'p>(
        &'p mut self,
        pose: PhysicsPose,
        shape: ColliderShape,
        mask: CollisionLayerMask,
    ) -> impl 'p + Iterator<Item = (ColliderKey, Option<BodyKey>)> {
        let entity_set = &self.entity_set;
        self.bvh
            .test_aabb(shape.aabb(pose))
            .filter_map(move |coll_key| {
                let coll = entity_set.get_collider(coll_key)?;
                if !mask.get(coll.layer) {
                    return None;
                }
                let body_key = entity_set.coll_bodies.get(coll_key.0).copied();
                let body = body_key.and_then(|k| entity_set.get_body(k));
                let their_pose = match body {
                    Some(body) => body.pose * coll.pose,
                    None => coll.pose,
                };
                let result = collision::shape_shape::intersection_check(
                    [pose, their_pose],
                    [shape, coll.shape],
                );
                if result.is_zero() {
                    None
                } else {
                    Some((coll_key, body_key))
                }
            })
    }

    /// Find the first solid collider intersected by the given ray.
    ///
    /// By convention, if the ray starts inside an object, it will miss that object.
    /// This way you can have a ray start e.g. at the center of a player's collider
    /// without having to worry about offsetting it just right.
    /// If you need to also know if the ray starts inside something, use
    /// [`query_point_body`][Self::query_point_body] in addition to this.
    #[inline]
    pub fn raycast(&mut self, ray: Ray) -> Option<CastHit> {
        self.spherecast(0.0, ray)
    }

    /// Find the first solid collider intersected by a sphere when swept along the given ray.
    ///
    /// This currently follows the same logic as [`raycast`][Self::raycast] if the sphere
    /// starts inside an object. With spherecasts it's quite easy to accidentally pass
    /// through an object that's up close. I'm not sure what the cleanest way to handle this
    /// is (TODO think about this), but for now you can use [`query_shape`][Self::query_shape]
    /// with a circle, similarly to how you would check a point when raycasting.
    pub fn spherecast(&mut self, radius: f64, ray: Ray) -> Option<CastHit> {
        // BVH traversal returns colliders in spatial order by their AABBs,
        // but this may not return the actual closest thing first if there are
        // small things near something large and diagonal.
        // we need to keep traversing the BVH until we get something farther than currently found t
        let mut closest_hit: Option<CastHit> = None;
        for leaf in self.bvh.sweep_aabb(radius, ray) {
            if leaf.t >= ray.length || matches!(closest_hit, Some(closest) if leaf.t >= closest.t) {
                return closest_hit;
            }

            let coll = match self.entity_set.get_collider(leaf.coll_key) {
                Some(c) => c,
                None => continue,
            };
            if !coll.is_solid() {
                continue;
            }
            let body = self
                .entity_set
                .coll_bodies
                .get(leaf.coll_key.0)
                .and_then(|bk| self.entity_set.get_body(*bk));
            let pose = match body {
                Some(body) => body.pose * coll.pose,
                None => coll.pose,
            };

            let hit = match collision::query::spherecast_collider(ray, radius, pose, *coll) {
                Some(hit) if hit.t <= ray.length => hit,
                _ => continue,
            };
            let already_found_closer = matches!(closest_hit, Some(closest) if closest.t <= hit.t);
            if already_found_closer {
                continue;
            }
            closest_hit = Some(CastHit {
                collider: leaf.coll_key,
                point: hit.point,
                normal: hit.normal,
                t: hit.t,
            });
        }
        closest_hit
    }

    /// For debug visualization
    /// (currently unused as the old visualizing pipelines don't work anymore
    /// and I haven't needed them since)
    #[allow(dead_code)]
    pub(crate) fn islands(&self) -> impl '_ + Iterator<Item = impl '_ + Iterator<Item = &'_ Body>> {
        self.working_bufs.islands.iter().map(move |island| {
            (island.body_range_start..island.body_range_start + island.body_count).filter_map(
                move |bi| {
                    self.entity_set
                        .bodies
                        .get_by_slot(self.working_bufs.sorted_second_pass.bodies[bi] as u32)
                        .map(|(_, b)| b)
                },
            )
        })
    }
}
