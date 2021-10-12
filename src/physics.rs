use itertools::izip;
use slotmap as sm;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::{
    graph::{self, LayerView, LayerViewMut},
    math as m,
};

//

pub mod collision;
use collision::HGrid;
pub use collision::{Collider, ColliderType, Contact, ContactResult, Material};

pub(crate) mod bitmatrix;

mod constraint;
pub use constraint::*;

pub mod forcefield;
pub use forcefield::ForceField;

mod body;
pub use body::*;

mod rope;
pub use rope::*;

mod constraint_graph;
use constraint_graph::*;

mod solver;
use solver::ColliderContext;

//

#[cfg(feature = "tracy")]
static COLLIDERS_PLOT: tracy_client::Plot = tracy_client::create_plot!("colliders");
#[cfg(feature = "tracy")]
static PAIRS_PLOT: tracy_client::Plot = tracy_client::create_plot!("collider pairs tested");
#[cfg(feature = "tracy")]
static CONTACTS_PLOT: tracy_client::Plot = tracy_client::create_plot!("contacts");

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

sm::new_key_type! {
    pub struct ConstraintHandle;
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
    // a "checksum" of all constraint graph edges in this island for topology checking.
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
    sorted_coll_pairs: Vec<[solver::ColliderWithContext; 2]>,

    node_ref_map: Vec<usize>,
    rope_next_particles: Vec<Option<usize>>,
    rope_prev_particles: Vec<Option<usize>>,
    rope_lateral_corrections: Vec<Option<m::Vec2>>,

    old_poses: Vec<m::Pose>,
    pre_contact_poses: Vec<m::Pose>,
    poses: Vec<m::Pose>,
    old_velocities: Vec<Velocity>,
    velocities: Vec<Velocity>,
    ext_f_accelerations: Vec<m::Vec2>,

    constraint_body_pairs: Vec<(usize, Option<usize>)>,
    colliders: Vec<solver::ColliderWithContext>,
    coll_pair_idxs: Vec<[usize; 2]>,
    contacts: Vec<ContactResult>,
    contacts_during_frame: Vec<bool>,
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
impl WorkingBuffers {
    fn new() -> Self {
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

            node_ref_map: Vec::new(),
            rope_next_particles: Vec::new(),
            rope_prev_particles: Vec::new(),
            rope_lateral_corrections: Vec::new(),

            old_poses: Vec::new(),
            pre_contact_poses: Vec::new(),
            poses: Vec::new(),
            old_velocities: Vec::new(),
            velocities: Vec::new(),
            ext_f_accelerations: Vec::new(),

            constraint_body_pairs: Vec::new(),
            colliders: Vec::new(),
            coll_pair_idxs: Vec::new(),
            contacts: Vec::new(),
            contacts_during_frame: Vec::new(),
            contact_lambdas: Vec::new(),
        }
    }
}

//
// physics proper
//

/// Constants used to adjust various features of the physics solver.
///
/// Start with `Default::default()` and adjust as needed.
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
    /// Should be more than 1 to avoid
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
            min_bodies_per_thread: 64,
        }
    }
}

pub struct Physics {
    pub consts: TuningConstants,
    pub mask_matrix: collision::MaskMatrix,
    user_constraints: sm::DenseSlotMap<ConstraintHandle, Constraint>,
    pub(crate) spatial_index: HGrid,
    constraint_graph: ConstraintGraph,
    sleeping_islands: Vec<SleepingIsland>,
    working_bufs: WorkingBuffers,
}

impl Physics {
    pub fn new(consts: TuningConstants, grid_params: collision::HGridParams) -> Self {
        Physics {
            consts,
            mask_matrix: Default::default(),
            user_constraints: sm::DenseSlotMap::with_key(),
            spatial_index: HGrid::new(grid_params),
            constraint_graph: ConstraintGraph {
                first_nodes_per_body: Vec::new(),
                last_nodes_per_body: Vec::new(),
                nodes: Vec::new(),
            },
            sleeping_islands: Vec::new(),
            working_bufs: WorkingBuffers::new(),
        }
    }

    /// Add a user-defined constraint to the system. Returns a handle that can be used to remove it later.
    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintHandle {
        self.user_constraints.insert(constraint)
    }

    /// Access a constraint if it still exists.
    pub fn get_constraint(&self, handle: ConstraintHandle) -> Option<&Constraint> {
        self.user_constraints.get(handle)
    }

    /// Mutably access a constraint if it still exists.
    pub fn get_constraint_mut(&mut self, handle: ConstraintHandle) -> Option<&mut Constraint> {
        self.user_constraints.get_mut(handle)
    }

    /// Remove a constraint from the system. Returns the constraint if it still existed.
    ///
    /// Constraints can also disappear on their own if the objects they're associated with
    /// are destroyed, so it's not guaranteed the constraint will exist
    /// even if it hasn't been explicitly removed before.
    pub fn remove_constraint(&mut self, handle: ConstraintHandle) -> Option<Constraint> {
        self.user_constraints.remove(handle)
    }

    /// Remove all constraints.
    pub fn clear_constraints(&mut self) {
        self.user_constraints.clear();
    }

    /// Detect collisions, solve constraint forces and move bodies.
    pub fn tick(
        &mut self,
        frame_dt: f64,
        forcefield: &impl ForceField,
        (mut l_pose, mut l_body, l_collider, l_rope): (
            LayerViewMut<m::Pose>,
            LayerViewMut<Body>,
            LayerView<Collider>,
            LayerView<Rope>,
        ),
    ) {
        let _main_span = tracy_span!("physics tick", "tick");

        let l_pose_immut = l_pose.as_view();
        let l_body_immut = l_body.as_view();

        let dt = frame_dt / self.consts.substeps as f64;
        let inv_dt = 1.0 / dt;
        let inv_dt_sq = inv_dt * inv_dt;

        let bufs = &mut self.working_bufs;

        // remove constraints where one or both participating bodies have been destroyed
        self.user_constraints.retain(|_, c| {
            l_body_immut.get(c.owner).is_some()
                && c.target
                    .map(|t| l_body_immut.get(t).is_some())
                    .unwrap_or(true)
        });
        bufs.user_constraints.clear();
        bufs.user_constraints.extend(self.user_constraints.values());

        //
        // Prepare the spatial index
        //

        let spi_span = tracy_span!("build spatial index", "tick");

        // constant for padding bounding volumes to fit movement during substeps,
        // collisions may be missed if higher accelerations occur
        let max_expected_accel_over_frame = self.consts.max_expected_acceleration * frame_dt;

        self.spatial_index.prepare(l_collider.components.len());
        bufs.coll_pair_idxs.clear();
        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep.
        for coll in l_collider.iter() {
            let pose = coll
                .get_neighbor(&l_pose_immut)
                .expect("A Collider didn't have a Pose");
            let aabb = match coll.get_neighbor(&l_body_immut) {
                Some(b) => coll
                    .c
                    .aabb(pose.c)
                    .extended(b.c.velocity.linear * frame_dt)
                    .padded(max_expected_accel_over_frame),
                None => coll.c.aabb(pose.c),
            };

            let coll_idx = coll.key().idx;
            bufs.coll_pair_idxs.extend(
                self.spatial_index
                    .test_and_insert(coll.key(), aabb, coll.c.layer, &self.mask_matrix)
                    .map(move |other| [coll_idx, other.idx]),
            )
        }

        #[cfg(feature = "tracy")]
        {
            COLLIDERS_PLOT.point(l_collider.iter().count() as f64);
            PAIRS_PLOT.point(bufs.coll_pair_idxs.len() as f64);
        }

        drop(spi_span);

        //
        // Build constraint graph
        //

        let constr_graph_span = tracy_span!("build constraint graph", "tick");

        self.constraint_graph.clear();
        self.constraint_graph.resize(l_body_immut.components.len());

        // rope constraints
        for rope_node in l_rope.iter() {
            let rope_node_idx = rope_node.key().idx;
            let mut iter = RopeIter::new(rope_node, &l_body_immut)
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
            let owner = constr.owner.idx;
            match constr.target {
                Some(target) => {
                    let target = target.idx;
                    self.constraint_graph.insert(
                        owner,
                        Edge::Constraint {
                            body_idx: target,
                            constr_idx,
                        },
                    );
                    self.constraint_graph.insert(
                        target,
                        Edge::Constraint {
                            body_idx: owner,
                            constr_idx,
                        },
                    );
                }
                None => self
                    .constraint_graph
                    .insert(owner, Edge::StaticConstraint { constr_idx }),
            }
        }
        // potential contacts from spatial index.
        // this doesn't necessarily cull as much as actually checking collisions,
        // but that would require redoing this every substep which would be costly.
        for (pair_idx, pair) in bufs.coll_pair_idxs.iter().enumerate() {
            let colls = pair.map(|ci| l_collider.get_unchecked_by_item_idx(ci));
            match colls.map(|c| c.get_neighbor(&l_body_immut).map(|b| b.key().idx)) {
                [Some(b1), Some(b2)] => {
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
        bufs.island_assigned
            .resize(l_body_immut.components.len(), false);
        bufs.islands.clear();
        bufs.sorted_first_pass.clear();
        bufs.sorted_second_pass.clear();

        let island_span = tracy_span!("build islands", "tick");

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
                        island.id.edge_sum += root_body_idx;
                    }
                    Edge::StaticContact { pair_idx } => {
                        bufs.sorted_first_pass.coll_pairs.push(*pair_idx);
                        island.pair_count += 1;

                        island.id.edge_sum += root_body_idx;
                    }
                }
            }
        }

        for body in l_body_immut.iter() {
            let bi = body.key().idx;
            if bufs.island_assigned[bi] {
                continue;
            }
            let mut island = Island {
                id: IslandId {
                    first_body: bi,
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
            search(bi, &mut island, &self.constraint_graph, bufs);
            bufs.islands.push(island);
        }

        //
        // sort islands by size and handle sleeping
        //

        for sleeping in &mut self.sleeping_islands {
            sleeping.continues_sleeping = false;
        }
        // remove sleeping islands from computation and set them to keep sleeping
        let sleeping_islands = &mut self.sleeping_islands;
        let sorted_first_pass = &bufs.sorted_first_pass;
        let sleep_vel_threshold = self.consts.sleep_vel_threshold;
        let fall_asleep_frames = self.consts.fall_asleep_frames;
        bufs.islands.retain(|isl| {
            if let Some(sleeping) = sleeping_islands.iter_mut().find(|slep| slep.id == isl.id) {
                // we need to check if anything started moving between frames due to user code
                if sorted_first_pass.bodies
                    [isl.body_range_start..isl.body_range_start + isl.body_count]
                    .iter()
                    .any(|bi| {
                        l_body_immut
                            .get_unchecked_by_item_idx(*bi)
                            .c
                            .velocity
                            .mag_sq()
                            >= sleep_vel_threshold
                    })
                {
                    return true;
                }

                sleeping.continues_sleeping = true;
                // keep island in computations if it hasn't slept for long enough
                sleeping.ticks_slept < fall_asleep_frames
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

        let buf_span = tracy_span!("populate buffers", "tick");

        // refs in island order, rest of the buffers based on these
        //
        // would be nice to have this as part of workingbuffers to avoid a few allocs
        // but we can't persist references across frames
        // and it would take some unsafe shenanigans to hold on to these
        let body_refs: Vec<graph::NodeRef<Body>> = bufs
            .sorted_second_pass
            .bodies
            .iter()
            .map(|&bi| l_body_immut.get_unchecked_by_item_idx(bi))
            .collect();
        // node_ref_map maps from the position of a node in the graph layer
        // to the position of a node in body_refs
        // we don't need to clear it because gaps will just never be touched
        bufs.node_ref_map.resize(l_body_immut.components.len(), 0);
        for (ref_pos, node) in body_refs.iter().enumerate() {
            bufs.node_ref_map[node.key().idx] = ref_pos;
        }

        bufs.sorted_rope_views.clear();
        let node_ref_map = &bufs.node_ref_map;
        bufs.sorted_rope_views
            .extend(bufs.sorted_second_pass.ropes.iter().map(|idx| {
                let rope_node = l_rope.get_unchecked_by_item_idx(*idx);
                let first_particle = rope_node
                    .get_neighbor(&l_body_immut)
                    .expect("A Rope didn't have any particles");
                solver::RopeView {
                    info: *rope_node.c,
                    start: node_ref_map[first_particle.key().idx],
                }
            }));

        bufs.sorted_constraints.clear();
        let user_constraints = &bufs.user_constraints;
        bufs.sorted_constraints.extend(
            bufs.sorted_second_pass
                .constraints
                .iter()
                .map(|&ci| user_constraints[ci]),
        );

        // store indices into neighboring particles for rope nodes
        bufs.rope_next_particles.clear();
        bufs.rope_next_particles.resize(body_refs.len(), None);
        bufs.rope_prev_particles.clear();
        bufs.rope_prev_particles.resize(body_refs.len(), None);
        for rope_node in l_rope.iter() {
            let node_ref_map = &bufs.node_ref_map;
            let mut iter = RopeIter::new(rope_node, &l_body_immut)
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
        bufs.rope_lateral_corrections.resize(body_refs.len(), None);

        bufs.old_poses.clear();
        bufs.old_poses.extend(body_refs.iter().map(|b| {
            *(b.get_neighbor(&l_pose_immut))
                .expect("A Body didn't have a Pose")
                .c
        }));
        // poses after velocity and constraints are applied, used for rope normal correction
        bufs.pre_contact_poses.clear();
        bufs.pre_contact_poses.extend_from_slice(&bufs.old_poses);
        // actual poses used in most calculations
        bufs.poses.clear();
        bufs.poses.extend_from_slice(&bufs.old_poses);
        // old velocities used for restitution
        bufs.old_velocities.clear();
        bufs.old_velocities
            .extend(body_refs.iter().map(|body| body.c.velocity));

        bufs.velocities.clear();
        bufs.velocities.extend_from_slice(&bufs.old_velocities);
        // accelerations from external forces used as a speed limit for restitution
        bufs.ext_f_accelerations.clear();
        bufs.ext_f_accelerations
            .resize(body_refs.len(), m::Vec2::default());

        bufs.colliders.clear();
        bufs.colliders.resize(
            l_collider.components.len(),
            // meaningless default to fill the gaps where colliders aren't actually alive,
            // we will not access these
            solver::ColliderWithContext {
                node_idx: usize::MAX,
                coll: Collider::new_circle(0.0),
                ctx: ColliderContext::Static(m::Pose::default()),
            },
        );
        for coll in l_collider.iter() {
            let node_idx = coll.key().idx;
            bufs.colliders[node_idx] = solver::ColliderWithContext {
                node_idx,
                coll: *coll.c,
                ctx: match coll.get_neighbor(&l_body_immut) {
                    Some(b) => ColliderContext::Body(bufs.node_ref_map[b.key().idx]),
                    None => ColliderContext::Static(match coll.get_neighbor(&l_pose_immut) {
                        Some(pose) => *pose.c,
                        None => m::Pose::default(),
                    }),
                },
            };
        }

        bufs.constraint_body_pairs.clear();
        let node_ref_map = &bufs.node_ref_map;
        bufs.constraint_body_pairs
            .extend(bufs.sorted_constraints.iter().map(|c| {
                (
                    node_ref_map[c.owner.idx],
                    c.target.map(|t| node_ref_map[t.idx]),
                )
            }));

        bufs.sorted_coll_pairs.clear();
        let coll_pair_idxs = &bufs.coll_pair_idxs;
        let colliders = &bufs.colliders;
        bufs.sorted_coll_pairs.extend(
            bufs.sorted_second_pass
                .coll_pairs
                .iter()
                .map(|pi| coll_pair_idxs[*pi].map(|ci| colliders[ci])),
        );
        // store latest contacts for use in the velocity step
        bufs.contacts.clear();
        bufs.contacts
            .resize(bufs.sorted_coll_pairs.len(), ContactResult::Zero);
        // collect pairs that had contacts for sending events after solving everything
        bufs.contacts_during_frame.clear();
        bufs.contacts_during_frame
            .resize(bufs.sorted_coll_pairs.len(), false);
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
            let ideal_body_count = (body_refs.len() + thread_count - 1) / thread_count;
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

        let mut body_refs_s = body_refs.as_slice();
        let mut old_poses_s = bufs.old_poses.as_mut_slice();
        let mut pre_cont_poses_s = bufs.pre_contact_poses.as_mut_slice();
        let mut poses_s = bufs.poses.as_mut_slice();
        let mut old_vels_s = bufs.old_velocities.as_mut_slice();
        let mut vels_s = bufs.velocities.as_mut_slice();
        let mut ext_f_acc_s = bufs.ext_f_accelerations.as_mut_slice();
        let mut rope_s = bufs.sorted_rope_views.as_mut_slice();
        let mut rope_next_p_s = bufs.rope_next_particles.as_mut_slice();
        let mut rope_prev_p_s = bufs.rope_prev_particles.as_mut_slice();
        let mut rope_lat_s = bufs.rope_lateral_corrections.as_mut_slice();
        let mut constr_s = bufs.sorted_constraints.as_slice();
        let mut constr_bodies_s = bufs.constraint_body_pairs.as_mut_slice();
        let mut coll_pairs_s = bufs.sorted_coll_pairs.as_mut_slice();
        let mut contacts_s = bufs.contacts.as_mut_slice();
        let mut cont_during_frame_s = bufs.contacts_during_frame.as_mut_slice();
        let mut cont_lambda_s = bufs.contact_lambdas.as_mut_slice();

        let mut island_start_idx = 0;

        let islands = &bufs.islands;
        for group in bufs
            .island_group_sizes
            .iter()
            .scan(0, |group_start, group_size| {
                let curr_group_start = *group_start;
                *group_start += *group_size;
                Some(&islands[curr_group_start..*group_start])
            })
        {
            let body_count = group.iter().map(|isl| isl.body_count).sum();
            let rope_count = group.iter().map(|isl| isl.rope_count).sum();
            let constr_count = group.iter().map(|isl| isl.constr_count).sum();
            let pair_count = group.iter().map(|isl| isl.pair_count).sum();

            let (body_refs, br_rest) = body_refs_s.split_at(body_count);
            body_refs_s = br_rest;
            let (old_poses, old_pose_rest) = old_poses_s.split_at_mut(body_count);
            old_poses_s = old_pose_rest;
            let (pre_contact_poses, pcp_rest) = pre_cont_poses_s.split_at_mut(body_count);
            pre_cont_poses_s = pcp_rest;
            let (poses, pose_rest) = poses_s.split_at_mut(body_count);
            poses_s = pose_rest;
            let (old_velocities, old_v_rest) = old_vels_s.split_at_mut(body_count);
            old_vels_s = old_v_rest;
            let (velocities, vel_rest) = vels_s.split_at_mut(body_count);
            vels_s = vel_rest;
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

            let (coll_pairs, coll_p_rest) = coll_pairs_s.split_at_mut(pair_count);
            coll_pairs_s = coll_p_rest;
            for pair in coll_pairs.iter_mut() {
                for coll in pair {
                    if let ColliderContext::Body(bi) = &mut coll.ctx {
                        *bi -= island_start_idx;
                    }
                }
            }
            let (contacts, contacts_rest) = contacts_s.split_at_mut(pair_count);
            contacts_s = contacts_rest;
            let (contacts_during_frame, cont_d_f_rest) =
                cont_during_frame_s.split_at_mut(pair_count);
            cont_during_frame_s = cont_d_f_rest;
            let (contact_lambdas, cont_l_rest) = cont_lambda_s.split_at_mut(pair_count);
            cont_lambda_s = cont_l_rest;

            island_group_views.push(solver::DataView {
                dt,
                inv_dt,
                inv_dt_sq,
                body_refs,
                old_poses,
                pre_contact_poses,
                poses,
                old_velocities,
                velocities,
                ext_f_accelerations,
                ropes,
                rope_next_particles,
                rope_prev_particles,
                rope_lateral_corrections,
                constraints,
                constraint_body_pairs,
                coll_pairs,
                contacts,
                contacts_during_frame,
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

        let substeps = self.consts.substeps;
        island_iter.for_each(|island_view| {
            for _substep in 0..substeps {
                let _substep_span = tracy_span!("substep", "tick");

                solver::solve(forcefield, island_view);
            }
        });

        #[cfg(feature = "tracy")]
        CONTACTS_PLOT.point(
            island_group_views
                .iter()
                .flat_map(|island_view| island_view.contacts_during_frame.iter())
                .filter(|c| **c)
                .count() as f64,
        );

        //
        // set islands where movement was below a threshold to sleep
        //

        let sleep_vel_threshold = self.consts.sleep_vel_threshold;
        for isl in &bufs.islands {
            if isl.can_sleep
                && bufs.velocities[isl.body_range_start..isl.body_range_start + isl.body_count]
                    .iter()
                    .all(|vel| vel.mag_sq() < sleep_vel_threshold)
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
        // apply results back to state from temp buffers
        //

        // drop body_refs and immutable views so we can get mutable references
        let body_nodes: Vec<graph::NodeKey<Body>> =
            body_refs.into_iter().map(|br| br.key()).collect();
        drop(l_body_immut);
        drop(l_pose_immut);

        for (body, pose_result, vel_result) in izip!(body_nodes, &bufs.poses, &bufs.velocities) {
            let mut body = l_body.get_mut_unchecked(body);
            let pose = body.get_neighbor_mut(&mut l_pose).unwrap();
            body.c.velocity = *vel_result;
            *pose.c = *pose_result;
        }
    }

    /// Find every rigid body that intersects with the given point.
    pub fn query_point_body<'p, 'g: 'p>(
        &'p self,
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
        self.spatial_index
            .test_point(point)
            .filter_map(move |stored_coll| {
                let coll = match l_collider.get(stored_coll) {
                    Some(coll) => coll,
                    None => return None,
                };
                let body = coll.get_neighbor(l_body)?;
                let pose = body.get_neighbor(l_pose)?;
                if collision::query::point_collider_bool(point, pose.c, coll.c) {
                    Some((pose, coll, body))
                } else {
                    None
                }
            })
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
