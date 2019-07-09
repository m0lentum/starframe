use super::{Collider, Collision, RigidBody};
use crate::{
    ecs::{event::EventQueue, system::*, Space},
    util::Transform,
};
use nalgebra::Vector2;

#[derive(ComponentFilter)]
pub struct RigidBodyFilter<'a> {
    tr: &'a mut Transform,
    body: &'a mut RigidBody,
    coll: Option<&'a Collider>,
}

// can I get all integrators to work with this signature?
// e.g. verlet needs two past positions but most need pos and vel
pub trait Integrator {
    fn step(timestep: f32, tr: &mut Transform, rb: &mut RigidBody);
}

// there will be many broad phase options (grid, hgrid, quadtree etc.)
// and generating possibly colliding pairs is their only purpose.
// making these pluggable was what inpired this whole pipeline idea
pub trait BroadPhase {
    fn pairs<'a>(items: &'a [RigidBodyFilter<'a>]) -> &'a [BodyPair<'a>];
}

pub struct BodyPair<'a>(RigidBodyFilter<'a>, RigidBodyFilter<'a>);

// I don't think there are meaningfully different narrow phase algorithms,
// so this probably won't actually need to be a trait
pub trait NarrowPhase {
    fn contacts<'a>(pairs: &'a [BodyPair<'a>]) -> &'a [Collision];
}

// potentially many implementations for this: Gauss-Seidel, Jacobi etc.
// will need to study more to make a call on this one
pub trait ConstraintSolver {
    fn solve<'a>(items: &'a mut [RigidBodyFilter<'a>], contacts: &'a Collision);
}

// idea: all implementations of one step of the physics process should be interchangeable
// maybe also ability to add extra steps?
// e.g. gravity, wind, other non-contact forces as custom extra steps
pub struct CollisionPipeline<I, B, N, S>
where
    I: Integrator,
    B: BroadPhase,
    N: NarrowPhase,
    S: ConstraintSolver,
{
    integrator: I,
    constant_force: Option<Vector2<f32>>,
    broad_phase: B,
    narrow_phase: N,
    solver: S,
}

impl<'a, I, B, N, S> System<'a> for CollisionPipeline<I, B, N, S>
where
    I: Integrator,
    B: BroadPhase,
    N: NarrowPhase,
    S: ConstraintSolver,
{
    type Filter = RigidBodyFilter<'a>;

    fn run_system(self, items: &mut [Self::Filter], space: &Space, queue: &mut EventQueue) {
        // motion
        // constant / spatially varying forces before / during motion?
        // if the integrator is complex (e.g. RK4) the forces will need to be fed to it

        // broad phase
        // generating contacts does not require mutable access.
        // maybe this should be done with an immutable filter so other things can run in parallel?
        // transforms are used in a lot of places so this is potentially quite beneficial

        // narrow phase

        // constraint solve
        // potentially also other constraints than collision
        // how generic can this be?

        // events
    }
}