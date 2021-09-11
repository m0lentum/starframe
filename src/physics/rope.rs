//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    graph::{self, Graph, Layer, UnsafeNode},
    graphics::Shape,
    math as m,
    physics::{collision::ROPE_LAYER, Body, Collider, Mass, Material},
};

/// A rope built out of connected particles.
#[derive(Clone, Copy, Debug)]
pub struct Rope {
    pub spacing: f64,
    pub thickness: f64,
    pub compliance: f64,
    pub bending_max_angle: f64,
    pub bending_compliance: f64,
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
#[allow(clippy::type_complexity)]
pub fn spawn_rope_line(
    mut rope: Rope,
    start: m::Vec2,
    end: m::Vec2,
    particle_mass: f64,
    (graph, l_body, l_pose, l_collider, l_rope, l_shape): (
        &mut Graph,
        &mut Layer<Body>,
        &mut Layer<m::Pose>,
        &mut Layer<Collider>,
        &mut Layer<Rope>,
        &mut Layer<Shape>,
    ),
) -> RopeProperties {
    let dist = end - start;
    let dist_mag = dist.mag();
    let segment_count = (dist_mag / rope.spacing).round() as usize;
    let particle_count = segment_count + 1;
    rope.spacing = dist_mag / segment_count as f64;
    let dir = dist / dist_mag;
    let step = rope.spacing * dir;

    let body_proto = Body {
        velocity: Default::default(),
        mass: Mass::from(particle_mass),
        moment_of_inertia: Mass::Infinite,
    };
    let collider_proto = Collider::new_circle(rope.thickness / 2.0)
        .with_layer(ROPE_LAYER)
        .with_material(rope.material);
    // temporary visualisation with Shapes until I get something more bespoke for this
    let shape_proto = Shape::Circle {
        r: rope.thickness / 2.0,
        points: 8,
        color: [0.729, 0.855, 0.333, 1.0],
    };

    let rope_node = l_rope.insert(rope, graph);
    let first_body = l_body.insert(body_proto, graph);
    let first_pose = l_pose.insert(m::Pose::new(start, Default::default()), graph);
    let first_coll = l_collider.insert(collider_proto, graph);
    let first_shape = l_shape.insert(shape_proto, graph);
    graph.connect(&first_body, &first_pose);
    graph.connect(&first_body, &first_coll);
    graph.connect(&first_pose, &first_coll);
    graph.connect(&first_pose, &first_shape);
    graph.connect_oneway(&rope_node, &first_body);
    let first_body = first_body.pos();

    let mut next_pos: m::Vec2 = start + step;
    let mut prev_body: graph::NodePosition = first_body.pos();
    for _ in 1..particle_count {
        let body = l_body.insert(body_proto, graph);
        let pose = l_pose.insert(m::Pose::new(next_pos, Default::default()), graph);
        let coll = l_collider.insert(collider_proto, graph);
        let shape = l_shape.insert(shape_proto, graph);
        graph.connect(&body, &pose);
        graph.connect(&pose, &coll);
        graph.connect(&body, &coll);
        graph.connect(&pose, &shape);
        graph.connect_oneway_unchecked(&prev_body, &body);

        next_pos += step;
        prev_body = body.pos();
    }
    graph.connect_oneway_unchecked(&prev_body, &rope_node);

    RopeProperties {
        particle_count,
        rope_node: graph::NodeRef::as_node(&rope_node, graph),
        first_particle: graph::NodeRef::as_node(&l_body.get_unchecked(first_body), graph),
        last_particle: graph::NodeRef::as_node(&l_body.get_unchecked(prev_body), graph),
    }
}

/// An iterator over the particles in a particular rope, in order from start to end.
pub struct RopeIter<'a> {
    rope_node: graph::NodeRef<'a, Rope>,
    has_started: bool,
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
            has_started: false,
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
        } else if !self.has_started {
            self.has_started = true;
            self.curr_body = self.graph.get_neighbor(&self.rope_node, self.l_body);
        }
        self.curr_body
    }
}
