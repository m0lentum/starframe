//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    graph::{self, UnsafeNode},
    graphics::Shape,
    math as m,
    physics::{Body, Collider, Mass, Material},
};

/// A rope built out of connected particles.
#[derive(Clone, Copy, Debug)]
pub struct Rope {
    pub spacing: f64,
    pub thickness: f64,
    pub compliance: f64,
    pub damping: f64,
    pub material: Material,
}

/// Information returned from the creation of a rope, useful for e.g. constraining the rope's ends.
#[derive(Clone, Copy, Debug)]
pub struct RopeProperties {
    pub particle_count: usize,
    pub rope_node: graph::Node<Rope>,
    pub first_particle: graph::Node<Body>,
    pub last_particle: graph::Node<Body>,
}

/// Spawn a rope in the shape of the line, adjusting spacing so that a particle lands on both
/// the start and end points.
pub fn spawn_rope_line(
    mut rope: Rope,
    start: m::Vec2,
    end: m::Vec2,
    particle_mass: f64,
    l_body: &mut graph::Layer<Body>,
    l_pose: &mut graph::Layer<m::Pose>,
    l_collider: &mut graph::Layer<Collider>,
    l_rope: &mut graph::Layer<Rope>,
    l_shape: &mut graph::Layer<Shape>,
    graph: &mut graph::Graph,
) -> RopeProperties {
    let dist = end - start;
    let dist_mag = dist.mag();
    let segment_count = (dist_mag / rope.spacing).round() as usize;
    rope.spacing = dist_mag / segment_count as f64;
    let dir = dist / dist_mag;
    let step = rope.spacing * dir;
    let half_step = step / 2.0;

    let particle_body_proto = Body {
        velocity: Default::default(),
        mass: Mass::from(particle_mass),
        moment_of_inertia: Mass::Infinite,
    };
    let collider_proto = Collider::new_capsule(rope.spacing, rope.thickness / 2.0);
    let segment_body_proto = Body {
        velocity: Default::default(),
        // treat mass as concentrated on the particles
        // -> each segment gets half of both end particles' mass
        mass: particle_body_proto.mass,
        // moment of inertia of a system of two point masses
        moment_of_inertia: Mass::from((particle_mass / 2.0) * rope.spacing.powi(2)),
    };
    let segment_orientation = m::Rotor2::from_rotation_between(m::Vec2::unit_x(), dir);
    // temporary visualisation with Shapes until I get something more bespoke for this
    let segment_shape_proto = Shape::Capsule {
        hl: rope.spacing / 2.0,
        r: rope.thickness / 2.0,
        points_per_cap: 4,
        color: [0.729, 0.855, 0.333, 1.0],
    };

    // Graph structure: rope node connects to first particle in the rope,
    // each particle connects to the next, and the last one connects back to
    // the rope node. Between every two particles there is another body representing
    // the segment between the particles, which gets a Collider and most constraints operate on it.
    //
    // Rope distance constraints are first solved on the particles,
    // then segment poses and lengths are updated from new particle positions,
    // other constraints are solved on the segments and finally particle positions are updated
    // again from the segments.
    let rope_node = l_rope.insert(rope, graph);
    let first_particle_body = l_body.insert(particle_body_proto, graph);
    let first_particle_pose =
        l_pose.insert(m::PoseBuilder::new().with_position(start).into(), graph);
    graph.connect(&first_particle_body, &first_particle_pose);
    graph.connect_oneway(&rope_node, &first_particle_body);
    let first_particle_body = first_particle_body.pos();

    let mut next_particle_pos: m::Vec2 = start + step;
    let mut prev_particle: graph::NodePosition = first_particle_body.pos();
    for _ in 1..=segment_count {
        let segment_body = l_body.insert(segment_body_proto, graph);
        let segment_coll = l_collider.insert(collider_proto, graph);
        let segment_pose = l_pose.insert(
            m::Pose::new(next_particle_pos - half_step, segment_orientation),
            graph,
        );
        graph.connect(&segment_body, &segment_coll);
        graph.connect(&segment_pose, &segment_body);
        graph.connect(&segment_pose, &segment_coll);
        graph.connect_oneway_unchecked(&prev_particle, &segment_body);
        let segment_shape = l_shape.insert(segment_shape_proto, graph);
        graph.connect(&segment_shape, &segment_pose);
        let segment_body = segment_body.pos();

        let particle_body = l_body.insert(particle_body_proto, graph);
        let particle_pose =
            l_pose.insert(m::Pose::new(next_particle_pos, Default::default()), graph);
        graph.connect(&particle_body, &particle_pose);
        graph.connect_oneway_unchecked(&segment_body, &particle_body);

        next_particle_pos += step;
        prev_particle = particle_body.pos();
    }
    graph.connect_oneway_unchecked(&prev_particle, &rope_node);

    RopeProperties {
        particle_count: segment_count + 1,
        rope_node: graph::NodeRef::as_node(&rope_node, graph),
        first_particle: graph::NodeRef::as_node(&l_body.get_unchecked(first_particle_body), graph),
        last_particle: graph::NodeRef::as_node(&l_body.get_unchecked(prev_particle), graph),
    }
}

/// An iterator over the particles in a particular rope, in order from start to end.
pub struct RopeIter<'a> {
    rope_node: graph::NodeRef<'a, Rope>,
    curr_body: Option<graph::NodeRef<'a, Body>>,
    l_body: &'a graph::Layer<Body>,
    graph: &'a graph::Graph,
}

impl<'a> RopeIter<'a> {
    pub fn new(
        rope_node: graph::NodeRef<'a, Rope>,
        l_body: &'a graph::Layer<Body>,
        graph: &'a graph::Graph,
    ) -> Self {
        Self {
            rope_node,
            curr_body: None,
            l_body,
            graph,
        }
    }
}

impl<'a> Iterator for RopeIter<'a> {
    type Item = graph::NodeRef<'a, Body>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(curr) = &self.curr_body {
            self.curr_body = self.graph.get_neighbor(curr, self.l_body)
        } else {
            self.curr_body = self.graph.get_neighbor(&self.rope_node, self.l_body);
        }
        self.curr_body
    }
}
