use itertools::izip;
use thunderdome as td;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::math as m;

//

pub mod collision;
use collision::bvh::Bvh;
pub use collision::{
    Collider, ColliderPolygon, ColliderShape, ColliderType, CollisionLayerMask, Contact,
    ContactResult, PhysicsMaterial, Ray,
};

pub(super) mod constraint;
pub use constraint::{Constraint, ConstraintBuilder, ConstraintLimit, ConstraintType};

pub mod forcefield;
pub use forcefield::ForceField;

pub(super) mod body;
pub use body::{Body, ColliderInfo, Mass};

pub mod rope;

mod component_graph;
use component_graph::ComponentGraph;
pub use component_graph::{BodyKey, ColliderKey};

mod constraint_graph;
use constraint_graph::*;

mod solver;

//
// public types
//

/// Velocity of an object.
///
// Equivalent to a Vec3 but with names for the translational and rotational part.
#[derive(Copy, Clone, Debug)]
pub struct Velocity {
    /// Linear velocity in metres per second.
    pub linear: m::Vec2,
    /// Angular velocity in radians per second.
    pub angular: f64,
}

impl Default for Velocity {
    fn default() -> Self {
        Velocity {
            linear: m::Vec2::zero(),
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
    pub fn point_velocity(&self, offset: m::Vec2) -> m::Vec2 {
        let tangent = m::left_normal(offset) * self.angular;
        self.linear + tangent
    }

    #[inline]
    pub fn apply_to_pose(&self, dt: f64, mut pose: m::Pose) -> m::Pose {
        let scaled = *self * dt;
        pose.append_translation(scaled.linear);
        pose.prepend_rotation(m::Angle::Rad(scaled.angular).into());
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
    pub normal: m::Unit<m::Vec2>,
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

/// Result of a [`raycast`][self::Physics::raycast] or [`spherecast`][self::Physics::spherecast].
#[derive(Clone, Copy, Debug)]
pub struct CastHit {
    /// The entity containing the collider that was hit.
    pub collider: hecs::Entity,
    /// The point in world space where the ray or swept shape intersected the collider.
    ///
    /// This is always a point on the hit collider's surface.
    /// To get the center point of the swept shape on impact, use `ray.point_at_t(hit.t)`.
    pub point: m::Vec2,
    /// The surface normal of the collider at the point of impact.
    pub normal: m::Unit<m::Vec2>,
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
    user_constraints: Vec<Constraint>,
    sorted_constraints: Vec<Constraint>,
    sorted_rope_views: Vec<solver::RopeView>,
    sorted_coll_pairs: Vec<[ColliderKey; 2]>,

    // bodies, sorted in island order
    bodies: Vec<Body>,
    // map from body keys to their position in the sorted buffer
    body_order: Vec<usize>,
    rope_next_particles: Vec<Option<usize>>,
    rope_prev_particles: Vec<Option<usize>>,
    rope_lateral_corrections: Vec<Option<m::Vec2>>,

    old_poses: Vec<m::Pose>,
    pre_contact_poses: Vec<m::Pose>,
    old_velocities: Vec<Velocity>,
    ext_f_accelerations: Vec<m::Vec2>,

    constraint_body_pairs: Vec<(usize, Option<usize>)>,
    coll_pair_keys: Vec<[ColliderKey; 2]>,
    contacts: Vec<ContactResult>,
    last_contacts: Vec<ContactResult>,
    contact_lambdas: Vec<f64>,
}
struct SortedIndices {
    bodies: Vec<usize>,
    ropes: Vec<usize>,
    constraints: Vec<usize>,
    coll_pairs: Vec<usize>,
}
impl SortedIndices {
    fn new() -> Self {
        Self {
            bodies: Vec::new(),
            ropes: Vec::new(),
            constraints: Vec::new(),
            coll_pairs: Vec::new(),
        }
    }
    fn clear(&mut self) {
        self.bodies.clear();
        self.ropes.clear();
        self.constraints.clear();
        self.coll_pairs.clear();
    }
}
impl Default for WorkingBuffers {
    fn default() -> Self {
        Self {
            sorted_first_pass: SortedIndices::new(),
            sorted_second_pass: SortedIndices::new(),
            island_assigned: Vec::new(),
            islands: Vec::new(),
            island_group_sizes: Vec::new(),

            user_constraints: Vec::new(),
            sorted_constraints: Vec::new(),
            sorted_rope_views: Vec::new(),
            sorted_coll_pairs: Vec::new(),

            bodies: Vec::new(),
            body_order: Vec::new(),
            rope_next_particles: Vec::new(),
            rope_prev_particles: Vec::new(),
            rope_lateral_corrections: Vec::new(),

            old_poses: Vec::new(),
            pre_contact_poses: Vec::new(),
            old_velocities: Vec::new(),
            ext_f_accelerations: Vec::new(),

            constraint_body_pairs: Vec::new(),
            coll_pair_keys: Vec::new(),
            contacts: Vec::new(),
            last_contacts: Vec::new(),
            contact_lambdas: Vec::new(),
        }
    }
}
impl WorkingBuffers {
    fn new() -> Self {
        Self::default()
    }
}

/// Key type to look up a constraint stored in the physics world.
#[derive(Clone, Copy, Debug)]
pub struct ConstraintKey(td::Index);

//
// physics proper
//

/// Constants used to adjust various features of the physics solver.
///
/// Start with `Default::default()` and adjust as needed.
#[derive(Clone, Copy, Debug)]
pub struct TuningConstants {
    /// The number of substeps per frame.
    ///
    /// Higher is more expensive and, up to a point, more accurate.
    /// At a certain point floating point inaccuracy will begin to create significant error.
    pub substeps: usize,
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
            substeps: 10,
            sleep_vel_threshold: 0.001,
            fall_asleep_frames: 10,
            max_expected_acceleration: 10.0,
            #[cfg(feature = "parallel")]
            min_bodies_per_thread: 64,
        }
    }
}

pub struct PhysicsWorld {
    pub consts: TuningConstants,
    pub mask_matrix: collision::CollisionMaskMatrix,
    user_constraints: td::Arena<Constraint>,
    ropes: Vec<rope::Rope>,
    pub(crate) bvh: Bvh,
    component_graph: ComponentGraph,
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
            user_constraints: td::Arena::new(),
            bvh: Bvh::new(),
            component_graph: ComponentGraph::new(),
            constraint_graph: ConstraintGraph {
                first_nodes_per_body: Vec::new(),
                last_nodes_per_body: Vec::new(),
                nodes: Vec::new(),
            },
            sleeping_islands: Vec::new(),
            working_bufs: WorkingBuffers::new(),
            contacts: Vec::new(),
            ropes: Vec::new(),
        }
    }

    /// Add a user-defined constraint to the system.
    /// Returns a key that can be used to remove it later.
    #[inline]
    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintKey {
        ConstraintKey(self.user_constraints.insert(constraint))
    }

    /// Access a constraint, if it still exists.
    #[inline]
    pub fn get_constraint(&self, key: ConstraintKey) -> Option<&Constraint> {
        self.user_constraints.get(key.0)
    }

    /// Mutably access a constraint, if it still exists.
    #[inline]
    pub fn get_constraint_mut(&mut self, key: ConstraintKey) -> Option<&mut Constraint> {
        self.user_constraints.get_mut(key.0)
    }

    /// Remove a constraint from the system. Returns the constraint if it still existed.
    ///
    /// Constraints can also disappear on their own if the objects they're associated with
    /// are destroyed, so it's not guaranteed the constraint will exist
    /// even if it hasn't been explicitly removed before.
    #[inline]
    pub fn remove_constraint(&mut self, key: ConstraintKey) -> Option<Constraint> {
        self.user_constraints.remove(key.0)
    }

    /// Insert a dynamic body into the world.
    #[inline]
    pub fn insert_body(&mut self, body: Body) -> BodyKey {
        self.component_graph.insert_body(body)
    }

    /// Access a [`Body`][self::Body] in the physics world, if it still exists.
    #[inline]
    pub fn get_body(&self, body: BodyKey) -> Option<&Body> {
        self.component_graph.bodies.get(body.0)
    }

    /// Mutably access a [`Body`][self::Body] in the physics world, if it still exists.
    #[inline]
    pub fn get_body_mut(&mut self, body: BodyKey) -> Option<&mut Body> {
        self.component_graph.bodies.get_mut(body.0)
    }

    /// Attach a collider to a dynamic body.
    #[inline]
    pub fn attach_collider(&mut self, body: BodyKey, coll: Collider) -> ColliderKey {
        self.component_graph.attach_collider(body, coll)
    }

    /// Access a [`Collider`][self::Collider] in the physics world, if it still exists.
    #[inline]
    pub fn get_collider(&self, coll: ColliderKey) -> Option<&Collider> {
        self.component_graph.colliders.get(coll.0)
    }

    /// Mutably access a [`Collider`][self::Collider] in the physics world, if it still exists.
    #[inline]
    pub fn get_collider_mut(&self, coll: ColliderKey) -> Option<&mut Collider> {
        self.component_graph.colliders.get_mut(coll.0)
    }

    /// Remove all constraints and reset internal state.
    pub fn clear(&mut self) {
        self.component_graph.clear();
        self.user_constraints.clear();
        self.sleeping_islands.clear();
        self.contacts.clear();
        self.working_bufs = WorkingBuffers::default();
    }

    /// Advance the simulation forward by `frame_dt` seconds.
    pub fn tick(&mut self, frame_dt: f64, time_scale: Option<f64>, forcefield: &impl ForceField) {
        let _main_span = tracy_client::span!("physics tick");

        // time scaling is done by adjusting both dt and actual substep count executed.
        // trying to keep dt as close to constant as possible to avoid any nasty inconsistencies
        let substeps;
        let dt;
        match time_scale {
            None => {
                substeps = self.consts.substeps;
                dt = frame_dt / substeps as f64;
            }
            Some(scale) => {
                substeps = (scale * self.consts.substeps as f64).ceil() as usize;
                // dt here must be such that `dt * substeps == time_scale * frame_dt
                dt = scale * frame_dt / substeps as f64;
            }
        }
        let inv_dt = 1.0 / dt;
        let inv_dt_sq = inv_dt * inv_dt;

        let bufs = &mut self.working_bufs;

        // remove constraints where one or both participating bodies have been destroyed
        self.user_constraints.retain(|_, c| {
            self.component_graph.bodies.get(c.owner).is_some()
                && c.target
                    .map(|t| self.component_graph.bodies.get(t).is_some())
                    .unwrap_or(true)
        });
        bufs.user_constraints.clear();
        bufs.user_constraints
            .extend(self.user_constraints.iter().map(|(_, v)| v));

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
        for (coll_key, coll) in self.component_graph.colliders.iter() {
            let coll_key = ColliderKey(coll_key);
            let body = self
                .component_graph
                .coll_bodies
                .get(coll_key.0)
                .and_then(|body_key| self.component_graph.bodies.get(body_key.0));
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
                            self.component_graph.colliders.get(other.0).unwrap().layer,
                        )
                    })
                    .map(move |other| [coll_key, other]),
            );
            self.bvh.insert(coll_key, aabb);
        }

        tracy_client::plot!("colliders", self.component_graph.colliders.len() as f64);
        tracy_client::plot!("collider pairs tested", bufs.coll_pair_keys.len() as f64);

        drop(spi_span);

        //
        // Build constraint graph
        //

        let constr_graph_span = tracy_client::span!("build constraint graph");

        self.constraint_graph.clear();
        self.constraint_graph
            .resize(self.component_graph.body_slot_count);

        // rope constraints
        for rope_node in l.rope.iter() {
            let rope_node_idx = rope_node.key().idx;
            let mut iter = rope_node
                .get_all_neighbors(&l_body_sub)
                .map(|node| node.key().idx)
                .peekable();
            while let Some(particle) = iter.next() {
                if let Some(&next_particle) = iter.peek() {
                    self.constraint_graph.insert(
                        particle,
                        Edge::Rope {
                            body_idx: next_particle,
                            rope_node_idx,
                        },
                    );
                    self.constraint_graph.insert(
                        next_particle,
                        Edge::Rope {
                            body_idx: particle,
                            rope_node_idx,
                        },
                    );
                }
            }
        }
        // custom constraints
        for (constr_idx, constr) in bufs.user_constraints.iter().enumerate() {
            match constr.target {
                Some(target) => {
                    self.constraint_graph.insert(
                        constr.owner,
                        Edge::Constraint {
                            body_idx: target,
                            constr_idx,
                        },
                    );
                    self.constraint_graph.insert(
                        target,
                        Edge::Constraint {
                            body_idx: constr.owner.0.slot(),
                            constr_idx,
                        },
                    );
                }
                None => self
                    .constraint_graph
                    .insert(constr.owner.0.slot(), Edge::StaticConstraint { constr_idx }),
            }
        }
        // potential contacts from spatial index.
        // this doesn't necessarily cull as much as actually checking collisions,
        // but that would require redoing this every substep which would be costly.
        for (pair_idx, pair) in bufs.coll_pair_keys.iter().enumerate() {
            let colls = pair.map(|ci| self.component_graph.colliders.get(ci).unwrap());
            match pair.map(|ci| self.component_graph.coll_bodies.get(ci).map(|b| b.0.slot())) {
                [Some(b1), Some(b2)] => {
                    if b1 == b2 {
                        // both colliders are part of the same compound collider,
                        // skip tests between them
                        continue;
                    }
                    self.constraint_graph.insert(
                        b1,
                        Edge::Contact {
                            body_idx: b2,
                            pair_idx,
                        },
                    );
                    self.constraint_graph.insert(
                        b2,
                        Edge::Contact {
                            body_idx: b1,
                            pair_idx,
                        },
                    );
                }
                [Some(b1), None] => {
                    self.constraint_graph
                        .insert(b1, Edge::StaticContact { pair_idx });
                }
                [None, Some(b2)] => {
                    self.constraint_graph
                        .insert(b2, Edge::StaticContact { pair_idx });
                }
                [None, None] => {}
            }
        }

        drop(constr_graph_span);

        //
        // Generate islands from graph
        //

        bufs.island_assigned.clear();
        bufs.island_assigned.resize(bufs.bodies.len(), false);
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
            for edge in constraint_graph.iter(root_body_idx) {
                match edge {
                    Edge::Rope {
                        body_idx,
                        rope_node_idx,
                    } => {
                        // sleeping causes problems with current rope implementation
                        // (indices are collected into buffers in a way that breaks with sleeping).
                        // this could be fixed but ropes are so unlikely to stop moving anyway
                        // that I'd rather just save some checking work and never even try to put them to sleep.
                        island.can_sleep = false;
                        if !bufs.sorted_first_pass.ropes
                            [island.rope_range_start..island.rope_range_start + island.rope_count]
                            .iter()
                            .any(|&idx| idx == *rope_node_idx)
                        {
                            bufs.sorted_first_pass.ropes.push(*rope_node_idx);
                            island.rope_count += 1;
                        }

                        // add 1 to root_body_idx so that we never get zero from this
                        // (which would essentially allow a constraint to be added
                        // to the island without changing its identity,
                        // causing bugs in the vicinity of body index 0)
                        island.id.edge_sum += (root_body_idx + 1) * (body_idx + 1);
                        search(*body_idx, island, constraint_graph, bufs);
                    }
                    Edge::Constraint {
                        body_idx,
                        constr_idx,
                    } => {
                        bufs.sorted_first_pass.constraints.push(*constr_idx);
                        island.constr_count += 1;

                        if !bufs.user_constraints[*constr_idx].can_sleep {
                            island.can_sleep = false;
                        }
                        island.id.edge_sum += (root_body_idx + 1) * (body_idx + 1);
                        search(*body_idx, island, constraint_graph, bufs);
                    }
                    Edge::Contact { body_idx, pair_idx } => {
                        bufs.sorted_first_pass.coll_pairs.push(*pair_idx);
                        island.pair_count += 1;

                        island.id.edge_sum += (root_body_idx + 1) * (body_idx + 1);
                        search(*body_idx, island, constraint_graph, bufs);
                    }
                    Edge::StaticConstraint { constr_idx } => {
                        bufs.sorted_first_pass.constraints.push(*constr_idx);
                        island.constr_count += 1;

                        if !bufs.user_constraints[*constr_idx].can_sleep {
                            island.can_sleep = false;
                        }
                        // no guarantee constr_idx is stable between frames,
                        // but we still need to stop sleeping when any constraint changes.
                        // adding a root_body_idx should do the job
                        island.id.edge_sum += root_body_idx + 1;
                    }
                    Edge::StaticContact { pair_idx } => {
                        bufs.sorted_first_pass.coll_pairs.push(*pair_idx);
                        island.pair_count += 1;

                        island.id.edge_sum += root_body_idx + 1;
                    }
                }
            }
        }

        for (body_key, body) in self.component_graph.bodies.iter() {
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
                        let (_, body) =
                            self.component_graph.bodies.get_by_slot(*bi as u32).unwrap();
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
        bufs.bodies
            .extend(bufs.sorted_second_pass.bodies.iter().map(|bi| {
                self.component_graph
                    .bodies
                    .get_by_slot(*bi as u32)
                    .unwrap()
                    .1
            }));
        // maps from the slot of a body in the thunderdome arena
        // to the index of a body in bufs.bodies.
        // we don't need to clear it because gaps will just never be touched
        bufs.body_order.clear();
        bufs.body_order
            .resize(self.component_graph.body_slot_count, 0);
        for (sorted_idx, body_slot) in bufs.sorted_second_pass.bodies.iter().enumerate() {
            bufs.body_order[*body_slot] = sorted_idx;
        }

        bufs.sorted_rope_views.clear();
        bufs.sorted_rope_views
            .extend(bufs.sorted_second_pass.ropes.iter().map(|idx| {
                let rope_node = l.rope.get_unchecked_by_item_idx(*idx);
                let first_particle = rope_node
                    .get_neighbor(&l_body_sub)
                    .expect("A Rope didn't have any particles");
                solver::RopeView {
                    info: *rope_node.c,
                    start: bufs.body_order[first_particle.key().idx],
                }
            }));

        bufs.sorted_constraints.clear();
        bufs.sorted_constraints.extend(
            bufs.sorted_second_pass
                .constraints
                .iter()
                .map(|&ci| bufs.user_constraints[ci]),
        );

        // store indices into neighboring particles for rope nodes
        bufs.rope_next_particles.clear();
        bufs.rope_next_particles.resize(bufs.bodies.len(), None);
        bufs.rope_prev_particles.clear();
        bufs.rope_prev_particles.resize(bufs.bodies.len(), None);
        for rope_node in l.rope.iter() {
            let node_ref_map = &bufs.body_order;
            let mut iter = rope_node
                .get_all_neighbors(&l_body_sub)
                .map(|node| node_ref_map[node.key().idx])
                .peekable();
            while let Some(particle) = iter.next() {
                if let Some(next_particle) = iter.peek() {
                    bufs.rope_next_particles[particle] = Some(*next_particle);
                    bufs.rope_prev_particles[*next_particle] = Some(particle);
                }
            }
        }
        // store lateral position corrections (bending resistance) for velocity correction
        bufs.rope_lateral_corrections.clear();
        bufs.rope_lateral_corrections
            .resize(bufs.sorted_second_pass.bodies.len(), None);

        bufs.old_poses.clear();
        bufs.old_poses.extend(bufs.bodies.iter().map(|b| b.pose));
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
            .resize(bufs.sorted_second_pass.bodies.len(), m::Vec2::default());

        bufs.constraint_body_pairs.clear();
        bufs.constraint_body_pairs
            .extend(bufs.sorted_constraints.iter().map(|c| {
                (
                    bufs.body_order[c.owner.0.slot() as usize],
                    c.target.map(|t| bufs.body_order[t.0.slot() as usize]),
                )
            }));

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
        // store contact forces for friction purposes
        bufs.contact_lambdas.clear();
        bufs.contact_lambdas
            .resize(bufs.sorted_coll_pairs.len(), 0.0);

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
            let ideal_body_count =
                (self.component_graph.bodies.len() + thread_count - 1) / thread_count;
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
        let mut pre_cont_poses_s = bufs.pre_contact_poses.as_mut_slice();
        let mut old_vels_s = bufs.old_velocities.as_mut_slice();
        let mut ext_f_acc_s = bufs.ext_f_accelerations.as_mut_slice();
        let mut rope_s = bufs.sorted_rope_views.as_mut_slice();
        let mut rope_next_p_s = bufs.rope_next_particles.as_mut_slice();
        let mut rope_prev_p_s = bufs.rope_prev_particles.as_mut_slice();
        let mut rope_lat_s = bufs.rope_lateral_corrections.as_mut_slice();
        let mut constr_s = bufs.sorted_constraints.as_slice();
        let mut constr_bodies_s = bufs.constraint_body_pairs.as_mut_slice();
        let mut coll_pairs_s = bufs.sorted_coll_pairs.as_mut_slice();
        let mut contacts_s = bufs.contacts.as_mut_slice();
        let mut last_contacts_s = bufs.last_contacts.as_mut_slice();
        let mut cont_lambda_s = bufs.contact_lambdas.as_mut_slice();

        let mut island_start_idx = 0;

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
            let rope_count = group.iter().map(|isl| isl.rope_count).sum();
            let constr_count = group.iter().map(|isl| isl.constr_count).sum();
            let pair_count = group.iter().map(|isl| isl.pair_count).sum();

            let (bodies, body_rest) = bodies_s.split_at_mut(body_count);
            bodies_s = body_rest;
            let (old_poses, old_pose_rest) = old_poses_s.split_at_mut(body_count);
            old_poses_s = old_pose_rest;
            let (pre_contact_poses, pcp_rest) = pre_cont_poses_s.split_at_mut(body_count);
            pre_cont_poses_s = pcp_rest;
            let (old_velocities, old_v_rest) = old_vels_s.split_at_mut(body_count);
            old_vels_s = old_v_rest;
            let (ext_f_accelerations, ext_f_rest) = ext_f_acc_s.split_at_mut(body_count);
            ext_f_acc_s = ext_f_rest;

            let (ropes, ropes_rest) = rope_s.split_at_mut(rope_count);
            rope_s = ropes_rest;
            // shift indices by start of layer
            for rope_view in ropes.iter_mut() {
                rope_view.start -= island_start_idx;
            }

            let (rope_next_particles, rope_next_rest) = rope_next_p_s.split_at_mut(body_count);
            rope_next_p_s = rope_next_rest;
            for np in rope_next_particles.iter_mut().filter_map(Option::as_mut) {
                *np -= island_start_idx;
            }

            let (rope_prev_particles, rope_prev_rest) = rope_prev_p_s.split_at_mut(body_count);
            rope_prev_p_s = rope_prev_rest;
            for pp in rope_prev_particles.iter_mut().filter_map(Option::as_mut) {
                *pp -= island_start_idx;
            }

            let (rope_lateral_corrections, rope_lat_rest) = rope_lat_s.split_at_mut(body_count);
            rope_lat_s = rope_lat_rest;
            let (constraints, constr_rest) = constr_s.split_at(constr_count);
            constr_s = constr_rest;
            let (constraint_body_pairs, constr_bod_rest) =
                constr_bodies_s.split_at_mut(constr_count);
            constr_bodies_s = constr_bod_rest;
            for (b1, b2) in constraint_body_pairs.iter_mut() {
                *b1 -= island_start_idx;
                if let Some(b2) = b2 {
                    *b2 -= island_start_idx;
                }
            }

            // map from collider to the body index in the slice given to the island
            let get_collider_body = |coll_key: ColliderKey| {
                let body_key = self.component_graph.coll_bodies.get(coll_key.0)?;
                let slot = body_key.0.slot() as usize;
                Some(bufs.body_order[slot] - island_start_idx)
            };

            let (coll_pairs, coll_p_rest) = coll_pairs_s.split_at_mut(pair_count);
            coll_pairs_s = coll_p_rest;
            let (contacts, contacts_rest) = contacts_s.split_at_mut(pair_count);
            contacts_s = contacts_rest;
            let (last_contacts, last_conts_rest) = last_contacts_s.split_at_mut(pair_count);
            last_contacts_s = last_conts_rest;
            let (contact_lambdas, cont_l_rest) = cont_lambda_s.split_at_mut(pair_count);
            cont_lambda_s = cont_l_rest;

            island_group_views.push(solver::DataView {
                dt,
                inv_dt,
                inv_dt_sq,
                get_collider_body: &get_collider_body,
                bodies,
                old_poses,
                pre_contact_poses,
                old_velocities,
                ext_f_accelerations,
                ropes,
                rope_next_particles,
                rope_prev_particles,
                rope_lateral_corrections,
                constraints,
                constraint_body_pairs,
                coll_pairs,
                contacts,
                last_contacts,
                contact_lambdas,
            });

            island_start_idx += body_count;
        }

        //
        // Actual physics step
        //

        #[cfg(feature = "parallel")]
        let island_iter = island_group_views.par_iter_mut();

        #[cfg(not(feature = "parallel"))]
        let island_iter = island_group_views.iter_mut();

        island_iter.for_each(|island_view| {
            for _substep in 0..substeps {
                let _substep_span = tracy_client::span!("substep");

                solver::solve(forcefield, island_view, &self.component_graph);
            }
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
                        entities: pair
                            .map(|c| l.collider.get_unchecked_by_item_idx(c.node_idx).key()),
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

        for (body_key, body) in self.component_graph.bodies.iter_mut() {
            let working_body = bufs.body_order[body_key.slot()];
            *body = working_body;
        }
    }

    /// Get all contacts that the given collider participated in during the last frame.
    ///
    /// All returned [`ContactInfo`][self::ContactInfo] objects are oriented such that the
    /// collider being searched for is the first item in `colliders` and `normal`
    /// faces away from it.
    pub fn contacts_for_collider(
        &self,
        coll: graph::NodeKey<Collider>,
    ) -> impl '_ + Iterator<Item = ContactInfo> {
        self.contacts.iter().filter_map(move |&cont| {
            if cont.entities[0] == coll {
                Some(cont)
            } else if cont.entities[1] == coll {
                Some(cont.flip())
            } else {
                None
            }
        })
    }

    /// Find every rigid body that intersects with the given point.
    ///
    /// Takes a mutable reference because bounding volume hierarchy
    /// traversal uses a mutable shared stack.
    /// This is subject to change if I figure out a good way to
    /// do this with interior mutability. (TODO)
    pub fn query_point_body<'p, 'g: 'p>(
        &'p mut self,
        point: m::Vec2,
        (l_pose, l_collider, l_body): &'g (
            graph::LayerView<'g, m::Pose>,
            graph::LayerView<'g, Collider>,
            graph::LayerView<'g, Body>,
        ),
    ) -> impl 'p
           + Iterator<
        Item = (
            graph::NodeRef<'g, m::Pose>,
            graph::NodeRef<'g, Collider>,
            graph::NodeRef<'g, Body>,
        ),
    > {
        self.bvh.test_point(point).filter_map(move |stored_coll| {
            let coll = match l_collider.get(stored_coll) {
                Some(coll) => coll,
                None => return None,
            };
            let body = coll.get_neighbor(l_body)?;
            let pose = body.get_neighbor(l_pose)?;
            if collision::query::point_collider_bool(point, *pose.c * coll.c.offset, *coll.c) {
                Some((pose, coll, body))
            } else {
                None
            }
        })
    }

    /// Get all colliders that intersect with the given shape.
    pub fn query_shape<'p, 'g: 'p>(
        &'p mut self,
        pose: m::Pose,
        shape: ColliderShape,
        mask: CollisionLayerMask,
        (l_pose, l_collider): &'g (
            graph::LayerView<'g, m::Pose>,
            graph::LayerView<'g, Collider>,
        ),
    ) -> impl '_ + Iterator<Item = graph::NodeRef<'g, Collider>> {
        self.bvh
            .test_aabb(shape.aabb(pose))
            .filter_map(move |coll_key| {
                let their_coll = l_collider.get(coll_key)?;
                if !mask.get(their_coll.c.layer) {
                    return None;
                }
                let their_pose = their_coll.get_neighbor(l_pose)?;
                let result = collision::shape_shape::intersection_check(
                    [pose, *their_pose.c * their_coll.c.offset],
                    [shape, their_coll.c.shape],
                );
                if result.is_zero() {
                    None
                } else {
                    Some(their_coll)
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
    pub fn raycast<'p>(
        &'p mut self,
        ray: Ray,
        max_distance: f64,
        layers: (graph::LayerView<m::Pose>, graph::LayerView<Collider>),
    ) -> Option<CastHit> {
        self.spherecast(0.0, ray, max_distance, layers)
    }

    /// Find the first solid collider intersected by a sphere when swept along the given ray.
    ///
    /// This currently follows the same logic as [`raycast`][Self::raycast] if the sphere
    /// starts inside an object. With spherecasts it's quite easy to accidentally pass
    /// through an object that's up close. I'm not sure what the cleanest way to handle this
    /// is (TODO think about this), but for now you can use [`query_shape`][Self::query_shape]
    /// with a circle, similarly to how you would check a point when raycasting.
    pub fn spherecast<'p>(
        &'p mut self,
        radius: f64,
        ray: Ray,
        max_distance: f64,
        (l_pose, l_collider): (graph::LayerView<m::Pose>, graph::LayerView<Collider>),
    ) -> Option<CastHit> {
        // BVH traversal returns colliders in spatial order by their AABBs,
        // but this may not return the actual closest thing first if there are
        // small things near something large and diagonal.
        // we need to keep traversing the BVH until we get something farther than currently found t
        let mut closest_hit: Option<CastHit> = None;
        for leaf in self.bvh.sweep_aabb(radius, ray, max_distance) {
            if leaf.t >= max_distance || matches!(closest_hit, Some(closest) if leaf.t >= closest.t)
            {
                return closest_hit;
            }

            let their_coll = match l_collider.get(leaf.coll_key) {
                Some(coll) => coll,
                None => continue,
            };
            if !their_coll.c.is_solid() {
                continue;
            }
            let their_pose = match their_coll.get_neighbor(&l_pose) {
                Some(pose) => pose,
                None => continue,
            };

            let hit = match collision::query::spherecast_collider(
                ray,
                radius,
                *their_pose.c,
                *their_coll.c,
            ) {
                Some(hit) if hit.t <= max_distance => hit,
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
    pub(crate) fn islands<'s, 'b: 's>(
        &'s self,
        l_body: &'b graph::LayerView<Body>,
    ) -> impl 's + Iterator<Item = impl 's + Iterator<Item = graph::NodeRef<Body>>> {
        self.working_bufs.islands.iter().map(move |island| {
            (island.body_range_start..island.body_range_start + island.body_count).map(move |bi| {
                l_body.get_unchecked_by_item_idx(self.working_bufs.sorted_second_pass.bodies[bi])
            })
        })
    }
}
