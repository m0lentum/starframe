//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    graph::{self, LayerView, LayerViewMut},
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
    pub rope_node: graph::NodeKey<Rope>,
    pub first_particle: graph::NodeKey<Body>,
    pub last_particle: graph::NodeKey<Body>,
}

/// Spawn a rope in the shape of the line, adjusting spacing so that a particle lands on both
/// the start and end points.
#[allow(clippy::type_complexity)]
pub fn spawn_rope_line(
    mut rope: Rope,
    start: m::Vec2,
    end: m::Vec2,
    particle_mass: f64,
    (mut l_body, mut l_pose, mut l_collider, mut l_rope, mut l_shape): (
        LayerViewMut<Body>,
        LayerViewMut<m::Pose>,
        LayerViewMut<Collider>,
        LayerViewMut<Rope>,
        LayerViewMut<Shape>,
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

    let mut rope_node = l_rope.insert(rope);
    let mut first_body = l_body.insert(body_proto);
    let mut first_pose = l_pose.insert(m::Pose::new(start, Default::default()));
    let mut first_coll = l_collider.insert(collider_proto);
    let mut first_shape = l_shape.insert(shape_proto);
    first_body.connect(&mut first_pose);
    first_body.connect(&mut first_coll);
    first_pose.connect(&mut first_coll);
    first_pose.connect(&mut first_shape);
    rope_node.connect_oneway(&mut first_body);
    let first_body = first_body.key();

    let mut next_pos: m::Vec2 = start + step;
    let mut prev_body: graph::NodeKey<Body> = first_body;
    for _ in 1..particle_count {
        let mut body = l_body.insert(body_proto);
        let mut pose = l_pose.insert(m::Pose::new(next_pos, Default::default()));
        let mut coll = l_collider.insert(collider_proto);
        let mut shape = l_shape.insert(shape_proto);
        body.connect(&mut pose);
        pose.connect(&mut coll);
        body.connect(&mut coll);
        pose.connect(&mut shape);
        let body = body.key();
        l_body
            .get_mut_unchecked(prev_body)
            .connect_oneway_same_layer(body);

        next_pos += step;
        prev_body = body;
    }
    l_body
        .get_mut_unchecked(prev_body)
        .connect_oneway(&mut rope_node);

    RopeProperties {
        particle_count,
        rope_node: rope_node.key(),
        first_particle: first_body,
        last_particle: prev_body,
    }
}

/// An iterator over the particles in a particular rope, in order from start to end.
pub struct RopeIter<'a> {
    rope_node: graph::NodeRef<'a, Rope>,
    has_started: bool,
    curr_body: Option<graph::NodeRef<'a, Body>>,
    l_body: &'a LayerView<'a, Body>,
}

impl<'a> RopeIter<'a> {
    pub fn new(rope_node: graph::NodeRef<'a, Rope>, l_body: &'a LayerView<'a, Body>) -> Self {
        Self {
            rope_node,
            has_started: false,
            curr_body: None,
            l_body,
        }
    }
}

impl<'a> Iterator for RopeIter<'a> {
    type Item = graph::NodeRef<'a, Body>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(curr) = &self.curr_body {
            self.curr_body = curr.get_neighbor(self.l_body)
        } else if !self.has_started {
            self.has_started = true;
            self.curr_body = self.rope_node.get_neighbor(self.l_body);
        }
        self.curr_body
    }
}
