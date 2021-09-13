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

pub struct Physics {
    pub substeps: usize,
    pub mask_matrix: collision::MaskMatrix,
    user_constraints: sm::DenseSlotMap<ConstraintHandle, Constraint>,
    pub(crate) spatial_index: HGrid,
    working_bufs: WorkingBuffers,
}

/// Cached buffers to avoid allocating a bunch of memory every frame.
/// Explanations in `tick` where populated
struct WorkingBuffers {
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
    coll_ctxs: Vec<ColliderContext>,
    contacts: Vec<ContactResult>,
    contact_lambdas: Vec<f64>,
    island_ids: Vec<Option<usize>>,
    islands: Vec<Island>,
}
impl WorkingBuffers {
    fn new() -> Self {
        Self {
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
            coll_ctxs: Vec::new(),
            contacts: Vec::new(),
            contact_lambdas: Vec::new(),
            island_ids: Vec::new(),
            islands: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Island {
    id: usize,
    pub(crate) body_idxs: Vec<usize>,
    rope_idxs: Vec<usize>,
    constr_idxs: Vec<usize>,
    coll_pair_idxs: Vec<usize>,
}

impl Physics {
    pub fn new(grid_params: collision::HGridParams) -> Self {
        Physics {
            substeps: 10,
            mask_matrix: Default::default(),
            user_constraints: sm::DenseSlotMap::with_key(),
            spatial_index: HGrid::new(grid_params),
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

        //
        // set up user-defined constraints
        //

        // remove constraints where one or both participating bodies have been destroyed
        self.user_constraints.retain(|_, c| {
            c.owner.check(graph).is_some()
                && c.target.map(|t| t.check(graph).is_some()).unwrap_or(true)
        });

        //
        // Prepare the spatial index
        //

        let spi_span = tracy_span!("build spatial index", "tick");

        // constant for padding bounding volumes to fit movement during substeps,
        // collisions may be missed if higher accelerations occur
        const MAX_EXPECTED_ACCEL: f64 = 10.0;
        let max_expected_accel_over_frame = MAX_EXPECTED_ACCEL * frame_dt;

        self.spatial_index.prepare(l_collider.content.len());
        // generate potentially colliding pairs,
        // these will be used to re-detect collisions every substep.
        let coll_pairs: Vec<[graph::NodeRef<Collider>; 2]> = {
            let mut pairs = Vec::new();
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

                let spatial_index = &mut self.spatial_index;
                pairs.extend(
                    spatial_index
                        .test_and_insert(
                            collision::hgrid::StoredNode {
                                idx: coll.pos().item_idx,
                                gen: graph.get_generation(&coll),
                            },
                            aabb,
                            coll.layer,
                            &self.mask_matrix,
                        )
                        .map(move |other| [coll, l_collider.get_unchecked_by_item_idx(other.idx)]),
                );
            }
            pairs
        };

        #[cfg(feature = "tracy")]
        {
            COLLIDERS_PLOT.point(l_collider.iter(graph).count() as f64);
            PAIRS_PLOT.point(coll_pairs.len() as f64);
        }

        drop(spi_span);

        //
        // Build constraint graph
        //

        const EDGES_IN_STACK: usize = 5;
        type ConstraintGraphEdges = TinyVec<[Edge; EDGES_IN_STACK]>;
        type ConstraintGraph = Vec<ConstraintGraphEdges>;

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
            // marking possible contacts with static objects as well
            // so that we can get this knowledge into the island solver
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

        let constr_graph = {
            let _span = tracy_span!("build constraint graph", "tick");

            let mut g: ConstraintGraph =
                vec![tiny_vec!([Edge; EDGES_IN_STACK]); l_body.content.len()];

            // rope constraints
            for rope_node in l_rope.iter(graph) {
                let rope_node_idx = rope_node.pos().item_idx;
                let mut iter = RopeIter::new(rope_node, l_body, graph)
                    .map(|node| node.pos().item_idx)
                    .peekable();
                while let Some(particle) = iter.next() {
                    if let Some(&next_particle) = iter.peek() {
                        g[particle].push(Edge::Rope {
                            body_idx: next_particle,
                            rope_node_idx,
                        });
                        g[next_particle].push(Edge::Rope {
                            body_idx: particle,
                            rope_node_idx,
                        });
                    }
                }
            }
            // custom constraints
            for (constr_idx, constr) in self.user_constraints.values().enumerate() {
                if let Some(target) = constr.target {
                    let owner = constr.owner.pos().item_idx;
                    let target = target.pos().item_idx;
                    g[owner].push(Edge::Constraint {
                        body_idx: target,
                        constr_idx,
                    });
                    g[target].push(Edge::Constraint {
                        body_idx: owner,
                        constr_idx,
                    });
                }
            }
            // potential contacts from spatial index.
            // this doesn't necessarily cull as much as actually checking collisions,
            // but that would require redoing this every substep which would be costly.
            for (pair_idx, pair) in coll_pairs.iter().enumerate() {
                match pair.map(|c| graph.get_neighbor(&c, l_body).map(|b| b.pos().item_idx)) {
                    [Some(b1), Some(b2)] => {
                        g[b1].push(Edge::Contact {
                            body_idx: b2,
                            pair_idx,
                        });
                        g[b2].push(Edge::Contact {
                            body_idx: b1,
                            pair_idx,
                        });
                    }
                    [Some(b1), None] => {
                        g[b1].push(Edge::StaticContact { pair_idx });
                    }
                    [None, Some(b2)] => {
                        g[b2].push(Edge::StaticContact { pair_idx });
                    }
                    [None, None] => {}
                }
            }
            g
        };

        //
        // Generate islands from graph
        //

        bufs.island_ids.clear();
        bufs.island_ids.resize(l_body.content.len(), None);
        bufs.islands.clear();

        let island_span = tracy_span!("build islands", "tick");

        fn search(
            island: &mut Island,
            curr: usize,
            island_ids: &mut [Option<usize>],
            constr_graph: &[ConstraintGraphEdges],
        ) {
            if island_ids[curr].is_some() {
                return;
            }
            island_ids[curr] = Some(island.id);
            island.body_idxs.push(curr);
            for edge in constr_graph[curr].iter() {
                match edge {
                    Edge::Rope {
                        body_idx,
                        rope_node_idx,
                    } => {
                        if !island.rope_idxs.iter().any(|&idx| idx == *rope_node_idx) {
                            island.rope_idxs.push(*rope_node_idx);
                        }

                        search(island, *body_idx, island_ids, constr_graph);
                    }
                    Edge::Constraint {
                        body_idx,
                        constr_idx,
                    } => {
                        island.constr_idxs.push(*constr_idx);

                        search(island, *body_idx, island_ids, constr_graph);
                    }
                    Edge::Contact { body_idx, pair_idx } => {
                        island.coll_pair_idxs.push(*pair_idx);

                        search(island, *body_idx, island_ids, constr_graph);
                    }
                    Edge::StaticContact { pair_idx } => {
                        island.coll_pair_idxs.push(*pair_idx);
                    }
                }
            }
        }

        for body in l_body.iter(graph) {
            let bi = body.pos().item_idx;
            if bufs.island_ids[bi].is_some() {
                continue;
            }
            let mut island = Island {
                id: bi,
                body_idxs: Vec::new(),
                rope_idxs: Vec::new(),
                constr_idxs: Vec::new(),
                coll_pair_idxs: Vec::new(),
            };
            search(&mut island, bi, &mut bufs.island_ids, &constr_graph);
            bufs.islands.push(island);
        }

        drop(island_span);

        //
        // Populate working buffers
        //

        // body_refs in island order, rest of the buffers based on these
        let body_refs: Vec<graph::NodeRef<Body>> = bufs
            .islands
            .iter()
            .flat_map(|island| {
                island
                    .body_idxs
                    .iter()
                    .map(|&bi| l_body.get_unchecked_by_item_idx(bi))
            })
            .collect();

        // node_ref_map maps from the position of a node in the graph layer
        // to the position of a node in body_refs
        // we don't need to clear it because gaps will just never be touched
        bufs.node_ref_map.resize(l_body.content.len(), 0);
        for (ref_pos, node) in body_refs.iter().enumerate() {
            bufs.node_ref_map[node.pos().item_idx] = ref_pos;
        }
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

        // store latest contacts for use in the velocity step
        bufs.contacts.clear();
        bufs.contacts.resize(coll_pairs.len(), ContactResult::Zero);
        // store contact forces for friction purposes
        bufs.contact_lambdas.clear();
        bufs.contact_lambdas.resize(coll_pairs.len(), 0.0);

        bufs.coll_ctxs.resize(
            l_collider.content.len(),
            // meaningless default to fill the gaps where colliders aren't actually alive,
            // we will not access these
            ColliderContext::Static(m::Pose::default()),
        );
        for coll in l_collider.iter(graph) {
            bufs.coll_ctxs[coll.pos().item_idx] = match graph.get_neighbor_unchecked(&coll, l_body)
            {
                Some(b) => ColliderContext::Body(bufs.node_ref_map[b.pos().item_idx]),
                None => {
                    ColliderContext::Static(match graph.get_neighbor_unchecked(&coll, l_pose) {
                        Some(pose) => *pose,
                        None => m::Pose::default(),
                    })
                }
            };
        }
        bufs.rope_lateral_corrections.iter_mut().for_each(|c| {
            *c = None;
        });

        // assuming here that slotmap's iteration order doesn't change if the
        // contents don't change. it doesn't guarantee this in the docs so if
        // constraints start jumping from one object to another this is why
        bufs.constraint_body_pairs.clear();
        let node_ref_map = &bufs.node_ref_map;
        bufs.constraint_body_pairs
            .extend(self.user_constraints.values().map(|c| {
                (
                    node_ref_map[c.owner.pos().item_idx],
                    c.target.map(|t| node_ref_map[t.pos().item_idx]),
                )
            }));

        //
        // Slice buffers into island-specific views
        //

        let mut island_views: Vec<solver::DataView<'_>> = Vec::with_capacity(bufs.islands.len());

        let mut body_refs_s = body_refs.as_slice();
        let mut old_poses_s = bufs.old_poses.as_mut_slice();
        let mut pre_cont_poses_s = bufs.pre_contact_poses.as_mut_slice();
        let mut poses_s = bufs.poses.as_mut_slice();
        let mut old_vels_s = bufs.old_velocities.as_mut_slice();
        let mut vels_s = bufs.velocities.as_mut_slice();
        let mut ext_f_acc_s = bufs.ext_f_accelerations.as_mut_slice();
        let mut rope_next_p_s = bufs.rope_next_particles.as_mut_slice();
        let mut rope_prev_p_s = bufs.rope_prev_particles.as_mut_slice();
        let mut rope_lat_s = bufs.rope_lateral_corrections.as_mut_slice();

        let mut island_start_idx = 0;

        for island in &bufs.islands {
            let len = island.body_idxs.len();
            let (body_refs, br_rest) = body_refs_s.split_at(len);
            body_refs_s = br_rest;
            let (old_poses, old_pose_rest) = old_poses_s.split_at_mut(len);
            old_poses_s = old_pose_rest;
            let (pre_contact_poses, pcp_rest) = pre_cont_poses_s.split_at_mut(len);
            pre_cont_poses_s = pcp_rest;
            let (poses, pose_rest) = poses_s.split_at_mut(len);
            poses_s = pose_rest;
            let (old_velocities, old_v_rest) = old_vels_s.split_at_mut(len);
            old_vels_s = old_v_rest;
            let (velocities, vel_rest) = vels_s.split_at_mut(len);
            vels_s = vel_rest;
            let (ext_f_accelerations, ext_f_rest) = ext_f_acc_s.split_at_mut(len);
            ext_f_acc_s = ext_f_rest;

            let ropes: Vec<solver::RopeView> = island
                .rope_idxs
                .iter()
                .map(|idx| {
                    let rope_node = l_rope.get_unchecked_by_item_idx(*idx);
                    let first_particle = graph
                        .get_neighbor(&rope_node, l_body)
                        .expect("A Rope didn't have any particles");
                    solver::RopeView {
                        info: *rope_node,
                        start: node_ref_map[first_particle.pos().item_idx] - island_start_idx,
                    }
                })
                .collect();

            let (rope_next_particles, rope_next_rest) = rope_next_p_s.split_at_mut(len);
            rope_next_p_s = rope_next_rest;
            let (rope_prev_particles, rope_prev_rest) = rope_prev_p_s.split_at_mut(len);
            rope_prev_p_s = rope_prev_rest;
            // shift indices by start of layer
            for np in rope_next_particles.iter_mut().filter_map(Option::as_mut) {
                *np -= island_start_idx;
            }
            for pp in rope_prev_particles.iter_mut().filter_map(Option::as_mut) {
                *pp -= island_start_idx;
            }

            let (rope_lateral_corrections, rope_lat_rest) = rope_lat_s.split_at_mut(len);
            rope_lat_s = rope_lat_rest;

            island_views.push(solver::DataView {
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
            });

            island_start_idx += len;
        }

        //
        // Actual physics step
        //

        for _substep in 0..self.substeps {
            let _substep_span = tracy_span!("substep", "tick");
            for island_view in &mut island_views {
                solver::solve(forcefield, island_view);
            }

            // TODO: events
            //
            // for (colls, contact) in izip!(data.coll_pairs, data.contacts) {
            //     if !matches!(contact, ContactResult::Zero) {
            //         if let Some(mut sink) = graph.get_neighbor_mut_unchecked(&colls[0], l_evt_sink)
            //         {
            //             sink.push(Event::Contact(ContactEvent {
            //                 other_collider: graph::NodeRef::as_node(&colls[1], graph),
            //             }));
            //         }
            //         if let Some(mut sink) = graph.get_neighbor_mut_unchecked(&colls[1], l_evt_sink)
            //         {
            //             sink.push(Event::Contact(ContactEvent {
            //                 other_collider: graph::NodeRef::as_node(&colls[0], graph),
            //             }));
            //         }
            //     }
            // }
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

    // used by debug visualizer
    pub(crate) fn islands(&self) -> &[Island] {
        &self.working_bufs.islands
    }
}
