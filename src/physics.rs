use crate::{
    event::{Event, EventSink},
    graph::{self, Graph, Layer, UnsafeNode},
    math as m,
};

use itertools::izip;
use slotmap as sm;
use tinyvec::{tiny_vec, TinyVec};

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

mod solver;
use solver::ColliderContext;

//

#[cfg(feature = "tracy")]
static COLLIDERS_PLOT: tracy_client::Plot = tracy_client::create_plot!("colliders");
#[cfg(feature = "tracy")]
static PAIRS_PLOT: tracy_client::Plot = tracy_client::create_plot!("collider pairs tested");
#[cfg(feature = "tracy")]
static CONTACTS_PLOT: tracy_client::Plot = tracy_client::create_plot!("contacts");

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
    /// Get the linear velocity of a point offset from the center of mass.
    pub fn point_velocity(&self, offset: m::Vec2) -> m::Vec2 {
        let tangent = m::left_normal(offset) * self.angular;
        self.linear + tangent
    }

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

/// Events produced by the physics system when two physics objects collide.
///
/// Note: for now this doesn't tell you anything except which body the contact was with.
/// This isn't great, but that's the only thing I've needed so far.
/// More info will be added when needed in a project.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    /// The collider that this body was in contact with.
    pub other_collider: graph::Node<Collider>,
}

sm::new_key_type! {
    pub struct ConstraintHandle;
}

//
// constraint graph types
//

// storing types of edge so we can later partition constraints for islands
#[derive(Clone, Copy, Debug)]
enum Edge {
    Rope {
        body_idx: usize,
        rope_node_idx: usize,
    },
    Constraint {
        body_idx: usize,
        constr_idx: usize,
    },
    Contact {
        body_idx: usize,
        pair_idx: usize,
    },
    // marking possible contacts and constraints with static objects as well
    // so that we can get this knowledge into the island solver
    StaticConstraint {
        constr_idx: usize,
    },
    StaticContact {
        pair_idx: usize,
    },
}
// default just to use with TinyVec
impl Default for Edge {
    fn default() -> Self {
        Edge::StaticContact { pair_idx: 0 }
    }
}
// the cost of allocs is heavy if there are often more edges per object than this,
// so it should be set as low as makes it statistically unlikely to go above it.
// for now it's a constant set empirically by checking allocs in Tracy,
// possibly should be user-controllable.
const EDGES_IN_STACK: usize = 24;
type ConstraintGraphEdges = TinyVec<[Edge; EDGES_IN_STACK]>;
type ConstraintGraph = Vec<ConstraintGraphEdges>;

pub struct Physics {
    pub substeps: usize,
    pub mask_matrix: collision::MaskMatrix,
    user_constraints: sm::DenseSlotMap<ConstraintHandle, Constraint>,
    pub(crate) spatial_index: HGrid,
    constraint_graph: ConstraintGraph,
    working_bufs: WorkingBuffers,
}

/// Cached buffers to avoid allocating a bunch of memory every frame.
/// Explanations in `tick` where populated
struct WorkingBuffers {
    // indices sorted by island for efficient island graph formation
    // without individual Vecs for each island
    sorted_body_idxs: Vec<usize>,
    sorted_rope_idxs: Vec<usize>,
    sorted_constr_idxs: Vec<usize>,
    sorted_pair_idxs: Vec<usize>,
    island_assigned: Vec<bool>,
    islands: Vec<Island>,

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
    contact_lambdas: Vec<f64>,
}
impl WorkingBuffers {
    fn new() -> Self {
        Self {
            sorted_body_idxs: Vec::new(),
            sorted_rope_idxs: Vec::new(),
            sorted_constr_idxs: Vec::new(),
            sorted_pair_idxs: Vec::new(),
            island_assigned: Vec::new(),
            islands: Vec::new(),

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
            contact_lambdas: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct Island {
    id: usize,
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
}

impl Physics {
    pub fn new(grid_params: collision::HGridParams) -> Self {
        Physics {
            substeps: 10,
            mask_matrix: Default::default(),
            user_constraints: sm::DenseSlotMap::with_key(),
            spatial_index: HGrid::new(grid_params),
            constraint_graph: Vec::new(),
            working_bufs: WorkingBuffers::new(),
        }
    }

    /// Set the number of substeps per frame.
    pub fn with_substeps(mut self, substeps: usize) -> Self {
        self.substeps = substeps;
        self
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
    #[allow(clippy::type_complexity)]
    pub fn tick(
        &mut self,
        frame_dt: f64,
        forcefield: &impl ForceField,
        (graph, l_pose, l_body, l_collider, l_rope, l_evt_sink): (
            &Graph,
            &mut Layer<m::Pose>,
            &mut Layer<Body>,
            &Layer<Collider>,
            &Layer<Rope>,
            &mut Layer<EventSink>,
        ),
    ) {
        let _main_span = tracy_span!("physics tick", "tick");

        let dt = frame_dt / self.substeps as f64;
        let inv_dt = 1.0 / dt;
        let inv_dt_sq = inv_dt * inv_dt;

        let bufs = &mut self.working_bufs;

        // remove constraints where one or both participating bodies have been destroyed
        self.user_constraints.retain(|_, c| {
            c.owner.check(graph).is_some()
                && c.target.map(|t| t.check(graph).is_some()).unwrap_or(true)
        });
        bufs.user_constraints.clear();
        bufs.user_constraints.extend(self.user_constraints.values());

        //
        // Prepare the spatial index
        //

        let spi_span = tracy_span!("build spatial index", "tick");

        // constant for padding bounding volumes to fit movement during substeps,
        // collisions may be missed if higher accelerations occur
        const MAX_EXPECTED_ACCEL: f64 = 10.0;
        let max_expected_accel_over_frame = MAX_EXPECTED_ACCEL * frame_dt;

        self.spatial_index.prepare(l_collider.content.len());
        bufs.coll_pair_idxs.clear();
        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep.
        for coll in l_collider.iter(graph) {
            let pose = graph
                .get_neighbor(&coll, l_pose)
                .expect("A Collider didn't have a Pose");
            let aabb = match graph.get_neighbor(&coll, l_body) {
                Some(b) => coll
                    .aabb(&*pose)
                    .extended(b.velocity.linear * frame_dt)
                    .padded(max_expected_accel_over_frame),
                None => coll.aabb(&*pose),
            };

            let coll_idx = coll.pos().item_idx;
            bufs.coll_pair_idxs.extend(
                self.spatial_index
                    .test_and_insert(
                        collision::hgrid::StoredNode {
                            idx: coll.pos().item_idx,
                            gen: graph.get_generation(&coll),
                        },
                        aabb,
                        coll.layer,
                        &self.mask_matrix,
                    )
                    .map(move |other| [coll_idx, other.idx]),
            )
        }

        #[cfg(feature = "tracy")]
        {
            COLLIDERS_PLOT.point(l_collider.iter(graph).count() as f64);
            PAIRS_PLOT.point(bufs.coll_pair_idxs.len() as f64);
        }

        drop(spi_span);

        //
        // Build constraint graph
        //

        let constr_graph_span = tracy_span!("build constraint graph", "tick");

        self.constraint_graph.clear();
        self.constraint_graph
            .resize(l_body.content.len(), tiny_vec!([Edge; EDGES_IN_STACK]));

        // rope constraints
        for rope_node in l_rope.iter(graph) {
            let rope_node_idx = rope_node.pos().item_idx;
            let mut iter = RopeIter::new(rope_node, l_body, graph)
                .map(|node| node.pos().item_idx)
                .peekable();
            while let Some(particle) = iter.next() {
                if let Some(&next_particle) = iter.peek() {
                    self.constraint_graph[particle].push(Edge::Rope {
                        body_idx: next_particle,
                        rope_node_idx,
                    });
                    self.constraint_graph[next_particle].push(Edge::Rope {
                        body_idx: particle,
                        rope_node_idx,
                    });
                }
            }
        }
        // custom constraints
        for (constr_idx, constr) in bufs.user_constraints.iter().enumerate() {
            let owner = constr.owner.pos().item_idx;
            match constr.target {
                Some(target) => {
                    let target = target.pos().item_idx;
                    self.constraint_graph[owner].push(Edge::Constraint {
                        body_idx: target,
                        constr_idx,
                    });
                    self.constraint_graph[target].push(Edge::Constraint {
                        body_idx: owner,
                        constr_idx,
                    });
                }
                None => self.constraint_graph[owner].push(Edge::StaticConstraint { constr_idx }),
            }
        }
        // potential contacts from spatial index.
        // this doesn't necessarily cull as much as actually checking collisions,
        // but that would require redoing this every substep which would be costly.
        for (pair_idx, pair) in bufs.coll_pair_idxs.iter().enumerate() {
            let colls = pair.map(|ci| l_collider.get_unchecked_by_item_idx(ci));
            match colls.map(|c| graph.get_neighbor(&c, l_body).map(|b| b.pos().item_idx)) {
                [Some(b1), Some(b2)] => {
                    self.constraint_graph[b1].push(Edge::Contact {
                        body_idx: b2,
                        pair_idx,
                    });
                    self.constraint_graph[b2].push(Edge::Contact {
                        body_idx: b1,
                        pair_idx,
                    });
                }
                [Some(b1), None] => {
                    self.constraint_graph[b1].push(Edge::StaticContact { pair_idx });
                }
                [None, Some(b2)] => {
                    self.constraint_graph[b2].push(Edge::StaticContact { pair_idx });
                }
                [None, None] => {}
            }
        }

        drop(constr_graph_span);

        //
        // Generate islands from graph
        //

        bufs.island_assigned.clear();
        bufs.island_assigned.resize(l_body.content.len(), false);
        bufs.islands.clear();
        bufs.sorted_body_idxs.clear();
        bufs.sorted_rope_idxs.clear();
        bufs.sorted_constr_idxs.clear();
        bufs.sorted_pair_idxs.clear();

        let island_span = tracy_span!("build islands", "tick");

        struct SearchContext<'a> {
            island: &'a mut Island,
            island_assigned: &'a mut [bool],
            sorted_body_idxs: &'a mut Vec<usize>,
            sorted_rope_idxs: &'a mut Vec<usize>,
            sorted_constr_idxs: &'a mut Vec<usize>,
            sorted_pair_idxs: &'a mut Vec<usize>,
            constr_graph: &'a [ConstraintGraphEdges],
        }
        fn search(body_idx: usize, ctx: &mut SearchContext<'_>) {
            if ctx.island_assigned[body_idx] {
                return;
            }
            ctx.island_assigned[body_idx] = true;
            ctx.sorted_body_idxs.push(body_idx);
            ctx.island.body_count += 1;
            for edge in ctx.constr_graph[body_idx].iter() {
                match edge {
                    Edge::Rope {
                        body_idx,
                        rope_node_idx,
                    } => {
                        if !ctx.sorted_rope_idxs[ctx.island.rope_range_start
                            ..ctx.island.rope_range_start + ctx.island.rope_count]
                            .iter()
                            .any(|&idx| idx == *rope_node_idx)
                        {
                            ctx.sorted_rope_idxs.push(*rope_node_idx);
                            ctx.island.rope_count += 1;
                        }

                        search(*body_idx, ctx);
                    }
                    Edge::Constraint {
                        body_idx,
                        constr_idx,
                    } => {
                        ctx.sorted_constr_idxs.push(*constr_idx);
                        ctx.island.constr_count += 1;

                        search(*body_idx, ctx);
                    }
                    Edge::Contact { body_idx, pair_idx } => {
                        ctx.sorted_pair_idxs.push(*pair_idx);
                        ctx.island.pair_count += 1;

                        search(*body_idx, ctx);
                    }
                    Edge::StaticConstraint { constr_idx } => {
                        ctx.sorted_constr_idxs.push(*constr_idx);
                        ctx.island.constr_count += 1;
                    }
                    Edge::StaticContact { pair_idx } => {
                        ctx.sorted_pair_idxs.push(*pair_idx);
                        ctx.island.pair_count += 1;
                    }
                }
            }
        }

        for body in l_body.iter(graph) {
            let bi = body.pos().item_idx;
            if bufs.island_assigned[bi] {
                continue;
            }
            let mut island = Island {
                id: bi,
                body_range_start: bufs.sorted_body_idxs.len(),
                body_count: 0,
                rope_range_start: bufs.sorted_rope_idxs.len(),
                rope_count: 0,
                constr_range_start: bufs.sorted_constr_idxs.len(),
                constr_count: 0,
                pair_range_start: bufs.sorted_pair_idxs.len(),
                pair_count: 0,
            };
            search(
                bi,
                &mut SearchContext {
                    island: &mut island,
                    island_assigned: &mut bufs.island_assigned,
                    sorted_body_idxs: &mut bufs.sorted_body_idxs,
                    sorted_rope_idxs: &mut bufs.sorted_rope_idxs,
                    sorted_constr_idxs: &mut bufs.sorted_constr_idxs,
                    sorted_pair_idxs: &mut bufs.sorted_pair_idxs,
                    constr_graph: &self.constraint_graph,
                },
            );
            bufs.islands.push(island);
        }

        drop(island_span);

        //
        // Populate working buffers
        //

        // refs in island order, rest of the buffers based on these
        //
        // would be nice to have this as part of workingbuffers to avoid a few allocs
        // but we can't persist references across frames
        // and it would take some unsafe shenanigans to hold on to these
        let body_refs: Vec<graph::NodeRef<Body>> = bufs
            .sorted_body_idxs
            .iter()
            .map(|&bi| l_body.get_unchecked_by_item_idx(bi))
            .collect();
        // node_ref_map maps from the position of a node in the graph layer
        // to the position of a node in body_refs
        // we don't need to clear it because gaps will just never be touched
        bufs.node_ref_map.resize(l_body.content.len(), 0);
        for (ref_pos, node) in body_refs.iter().enumerate() {
            bufs.node_ref_map[node.pos().item_idx] = ref_pos;
        }

        bufs.sorted_rope_views.clear();
        let node_ref_map = &bufs.node_ref_map;
        bufs.sorted_rope_views
            .extend(bufs.sorted_rope_idxs.iter().map(|idx| {
                let rope_node = l_rope.get_unchecked_by_item_idx(*idx);
                let first_particle = graph
                    .get_neighbor(&rope_node, l_body)
                    .expect("A Rope didn't have any particles");
                solver::RopeView {
                    info: *rope_node,
                    start: node_ref_map[first_particle.pos().item_idx],
                }
            }));

        bufs.sorted_constraints.clear();
        let user_constraints = &bufs.user_constraints;
        bufs.sorted_constraints.extend(
            bufs.sorted_constr_idxs
                .iter()
                .map(|&ci| user_constraints[ci]),
        );

        // store indices into neighboring particles for rope nodes
        bufs.rope_next_particles.clear();
        bufs.rope_next_particles.resize(body_refs.len(), None);
        bufs.rope_prev_particles.clear();
        bufs.rope_prev_particles.resize(body_refs.len(), None);
        for rope_node in l_rope.iter(graph) {
            let node_ref_map = &bufs.node_ref_map;
            let mut iter = RopeIter::new(rope_node, l_body, graph)
                .map(|node| node_ref_map[node.pos().item_idx])
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
        bufs.old_poses.extend(
            body_refs
                .iter()
                .map(|b| *(graph.get_neighbor(b, l_pose)).expect("A Body didn't have a Pose")),
        );
        // poses after velocity and constraints are applied, used for rope normal correction
        bufs.pre_contact_poses.clear();
        bufs.pre_contact_poses.extend_from_slice(&bufs.old_poses);
        // actual poses used in most calculations
        bufs.poses.clear();
        bufs.poses.extend_from_slice(&bufs.old_poses);
        // old velocities used for restitution
        bufs.old_velocities.clear();
        bufs.old_velocities
            .extend(body_refs.iter().map(|body| body.velocity));

        bufs.velocities.clear();
        bufs.velocities.extend_from_slice(&bufs.old_velocities);
        // accelerations from external forces used as a speed limit for restitution
        bufs.ext_f_accelerations.clear();
        bufs.ext_f_accelerations
            .resize(body_refs.len(), m::Vec2::default());

        bufs.colliders.clear();
        bufs.colliders.resize(
            l_collider.content.len(),
            // meaningless default to fill the gaps where colliders aren't actually alive,
            // we will not access these
            solver::ColliderWithContext {
                node_idx: usize::MAX,
                coll: Collider::new_circle(0.0),
                ctx: ColliderContext::Static(m::Pose::default()),
            },
        );
        for coll in l_collider.iter(graph) {
            let node_idx = coll.pos().item_idx;
            bufs.colliders[node_idx] = solver::ColliderWithContext {
                node_idx,
                coll: *coll,
                ctx: match graph.get_neighbor_unchecked(&coll, l_body) {
                    Some(b) => ColliderContext::Body(bufs.node_ref_map[b.pos().item_idx]),
                    None => {
                        ColliderContext::Static(match graph.get_neighbor_unchecked(&coll, l_pose) {
                            Some(pose) => *pose,
                            None => m::Pose::default(),
                        })
                    }
                },
            };
        }

        bufs.constraint_body_pairs.clear();
        let node_ref_map = &bufs.node_ref_map;
        bufs.constraint_body_pairs
            .extend(bufs.sorted_constraints.iter().map(|c| {
                (
                    node_ref_map[c.owner.pos().item_idx],
                    c.target.map(|t| node_ref_map[t.pos().item_idx]),
                )
            }));

        bufs.sorted_coll_pairs.clear();
        let coll_pair_idxs = &bufs.coll_pair_idxs;
        let colliders = &bufs.colliders;
        bufs.sorted_coll_pairs.extend(
            bufs.sorted_pair_idxs
                .iter()
                .map(|pi| coll_pair_idxs[*pi].map(|ci| colliders[ci])),
        );
        // store latest contacts for use in the velocity step
        bufs.contacts.clear();
        bufs.contacts
            .resize(bufs.sorted_coll_pairs.len(), ContactResult::Zero);
        // store contact forces for friction purposes
        bufs.contact_lambdas.clear();
        bufs.contact_lambdas
            .resize(bufs.sorted_coll_pairs.len(), 0.0);

        //
        // group islands into as many groups as we have threads
        //

        // constant for testing, TODO: use a threadpool and get the thread count of that
        let thread_count = 4;

        // for now, just putting an equal number of islands in each group.
        // this could be optimized further by making sure each group gets
        // roughly the same number of bodies
        let chunk_size = if bufs.islands.len() <= thread_count {
            1
        } else {
            (bufs.islands.len() + thread_count - 1) / thread_count
        };
        let island_groups = bufs.islands.chunks(chunk_size);

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
        let mut cont_lambda_s = bufs.contact_lambdas.as_mut_slice();

        let mut island_start_idx = 0;

        for group in island_groups {
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
                contact_lambdas,
            });

            island_start_idx += body_count;
        }

        //
        // Actual physics step
        //

        for _substep in 0..self.substeps {
            let _substep_span = tracy_span!("substep", "tick");
            for island_view in &mut island_group_views {
                solver::solve(forcefield, island_view);

                for (colls, contact) in izip!(&*island_view.coll_pairs, &*island_view.contacts) {
                    if !matches!(contact, ContactResult::Zero) {
                        let colls =
                            colls.map(|coll| l_collider.get_unchecked_by_item_idx(coll.node_idx));
                        if let Some(mut sink) =
                            graph.get_neighbor_mut_unchecked(&colls[0], l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_collider: graph::NodeRef::as_node(&colls[1], graph),
                            }));
                        }
                        if let Some(mut sink) =
                            graph.get_neighbor_mut_unchecked(&colls[1], l_evt_sink)
                        {
                            sink.push(Event::Contact(ContactEvent {
                                other_collider: graph::NodeRef::as_node(&colls[0], graph),
                            }));
                        }
                    }
                }
            }
        }

        //
        // apply results back to state from temp buffers
        //

        // drop body_refs so we can get mutable references
        let body_nodes: Vec<graph::NodePosition> =
            body_refs.into_iter().map(|br| br.pos()).collect();
        for (body, pose_result, vel_result) in izip!(body_nodes, &bufs.poses, &bufs.velocities) {
            let mut body = l_body.get_mut_unchecked(body);
            let mut pose = graph.get_neighbor_mut_unchecked(&body, l_pose).unwrap();
            body.velocity = *vel_result;
            *pose = *pose_result;
        }
    }

    /// Find every rigid body that intersects with the given point.
    pub fn query_point_body<'p, 'g: 'p>(
        &'p self,
        point: m::Vec2,
        l_pose: &'g graph::Layer<m::Pose>,
        l_collider: &'g graph::Layer<Collider>,
        l_body: &'g graph::Layer<Body>,
        graph: &'g graph::Graph,
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
                let coll = l_collider.get_unchecked_by_item_idx(stored_coll.idx);
                if graph.get_generation(&coll) != stored_coll.gen {
                    return None;
                }
                let rb = graph.get_neighbor(&coll, l_body)?;
                let pose = graph.get_neighbor(&rb, l_pose)?;
                if collision::query::point_collider_bool(point, &*pose, &*coll) {
                    Some((pose, coll, rb))
                } else {
                    None
                }
            })
    }

    /// For debug visualization
    pub(crate) fn islands<'s, 'b: 's>(
        &'s self,
        l_body: &'b graph::Layer<Body>,
    ) -> impl 's + Iterator<Item = impl 's + Iterator<Item = graph::NodeRef<Body>>> {
        self.working_bufs.islands.iter().map(move |island| {
            (island.body_range_start..island.body_range_start + island.body_count).map(move |bi| {
                l_body.get_unchecked_by_item_idx(self.working_bufs.sorted_body_idxs[bi])
            })
        })
    }
}
