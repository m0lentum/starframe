//! Tools for creating and manipulating physically simulated ropes.

use crate::{
    graph::{self, LayerView, LayerViewMut},
    graphics::Shape,
    math as m,
    physics::{collision::ROPE_LAYER, Body, Collider, Mass, Material},
};

/// A rope built out of connected particles.
///
/// The representation of a rope in the graph is a loop that starts at a Rope node,
/// goes through every particle Body in the rope in order.
/// Every particle is connected back to the Rope node.
#[derive(Clone, Copy, Debug)]
pub struct Rope {
    pub spacing: f64,
    pub thickness: f64,
    pub compliance: f64,
    pub bending_max_angle: f64,
    pub bending_compliance: f64,
    pub damping: f64,
    pub material: Material,
    pub particle_mass: f64,
}
impl Default for Rope {
    fn default() -> Self {
        Self {
            spacing: 0.1,
            thickness: 0.12,
            compliance: 0.0000001,
            bending_max_angle: m::Angle::Deg(30.0).rad(),
            bending_compliance: 0.2,
            damping: 20.0,
            material: Material {
                static_friction_coef: None,
                dynamic_friction_coef: Some(1.5),
                restitution_coef: 0.0,
            },
            particle_mass: 0.02,
        }
    }
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
    (mut l_body, l_pose, l_collider, mut l_rope, l_shape): (
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

    let mut rope_node = l_rope.insert(rope);
    let [first_body, last_body] = build_rope_line(
        &mut rope_node,
        start,
        step,
        particle_count,
        (l_body.subview_mut(), l_pose, l_collider, l_shape),
    );
    // outgoing connection from the rope node.
    // incoming connections from each particle are created by `build_rope_line`
    rope_node.connect_oneway(&mut l_body.get_mut_unchecked(first_body));

    RopeProperties {
        particle_count,
        rope_node: rope_node.key(),
        first_particle: first_body,
        last_particle: last_body,
    }
}

/// Add `count` particles to the end of an existing rope in a line.
pub fn extend_rope_line(
    rope_node: &mut graph::NodeRefMut<Rope>,
    dir: m::Unit<m::Vec2>,
    count: usize,
    (mut l_body, l_pose, l_collider, l_shape): (
        LayerViewMut<Body>,
        LayerViewMut<m::Pose>,
        LayerViewMut<Collider>,
        LayerViewMut<Shape>,
    ),
) -> RopeProperties {
    let l_body_sub = l_body.subview();
    let mut rope_iter = RopeIter::new(rope_node.subview(), &l_body_sub).enumerate();
    let first_particle = rope_iter.next().expect("Rope had no particles").1;
    let (last_particle_idx, last_particle) = rope_iter.last().unwrap_or((0, first_particle));

    let last_particle_pos = last_particle
        .get_neighbor(&l_pose.subview())
        .expect("Rope particle had no Pose")
        .c
        .translation;
    let step = *dir * rope_node.c.spacing;
    let first_new_pos = last_particle_pos + step;

    let first_particle = first_particle.key();
    let last_particle = last_particle.key();
    drop(l_body_sub);

    let [first_new, last_new] = build_rope_line(
        rope_node,
        first_new_pos,
        step,
        count,
        (l_body.subview_mut(), l_pose, l_collider, l_shape),
    );

    let mut last_old = l_body.get_mut_unchecked(last_particle);
    last_old.connect_oneway_same_layer(first_new);

    RopeProperties {
        particle_count: last_particle_idx + 1 + count,
        rope_node: rope_node.key(),
        first_particle,
        last_particle: last_new,
    }
}

/// Spawn `count` particles in a line, connect them, and return keys to the first and last one.
fn build_rope_line(
    rope_node: &mut graph::NodeRefMut<Rope>,
    start: m::Vec2,
    step: m::Vec2,
    count: usize,
    (mut l_body, mut l_pose, mut l_collider, mut l_shape): (
        LayerViewMut<Body>,
        LayerViewMut<m::Pose>,
        LayerViewMut<Collider>,
        LayerViewMut<Shape>,
    ),
) -> [graph::NodeKey<Body>; 2] {
    let body_proto = Body {
        velocity: Default::default(),
        mass: Mass::from(rope_node.c.particle_mass),
        moment_of_inertia: Mass::Infinite,
    };
    let collider_proto = Collider::new_circle(rope_node.c.thickness / 2.0)
        .with_layer(ROPE_LAYER)
        .with_material(rope_node.c.material);
    // temporary visualisation with Shapes until I get something more bespoke for this
    let shape_proto = Shape::Circle {
        r: rope_node.c.thickness / 2.0,
        points: 8,
        color: [0.729, 0.855, 0.333, 1.0],
    };

    let mut first_body = l_body.insert(body_proto);
    let mut first_pose = l_pose.insert(m::Pose::new(start, Default::default()));
    let mut first_coll = l_collider.insert(collider_proto);
    let mut first_shape = l_shape.insert(shape_proto);
    first_body.connect(&mut first_pose);
    first_body.connect(&mut first_coll);
    first_pose.connect(&mut first_coll);
    first_pose.connect(&mut first_shape);
    first_body.connect_oneway(rope_node);
    let first_body = first_body.key();
    let mut next_pos: m::Vec2 = start + step;
    let mut prev_body: graph::NodeKey<Body> = first_body;
    for _ in 1..count {
        let mut body = l_body.insert(body_proto);
        let mut pose = l_pose.insert(m::Pose::new(next_pos, Default::default()));
        let mut coll = l_collider.insert(collider_proto);
        let mut shape = l_shape.insert(shape_proto);
        body.connect(&mut pose);
        pose.connect(&mut coll);
        body.connect(&mut coll);
        pose.connect(&mut shape);
        body.connect_oneway(rope_node);
        let body = body.key();
        l_body
            .get_mut_unchecked(prev_body)
            .connect_oneway_same_layer(body);

        next_pos += step;
        prev_body = body;
    }
    [first_body, prev_body]
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
