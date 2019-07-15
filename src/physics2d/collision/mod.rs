use crate::ecs::{event::SpaceEvent, space::Space, IdType};
use nalgebra::{Point2, Unit, Vector2};

pub mod broadphase;
mod collider;
pub use collider::Collider;
pub use solver::CollisionSolver;
mod narrowphase;
mod solver;

pub use crate::util::Transform;

/// Event containing information about a collision relative to one of the objects involved.
/// # Listener behavior
/// Only the listener for the involved object is called.
#[derive(Clone, Copy, Debug)]
pub struct CollisionEvent {
    pub(crate) source: IdType,
    /// The id of the object that was collided with.
    pub other: IdType,
    /// The normal of the plane of collision, pointing towards this object.
    pub normal: Unit<Vector2<f32>>,
    /// The depth of penetration.
    pub depth: f32,
    /// The world-space coordinates of the exact point or points on the surface of this object where the collision occurred.
    pub manifold: Manifold,
}

impl SpaceEvent for CollisionEvent {
    fn handle(&self, space: &mut Space) {
        space.run_listener(self.source, self);
    }
}

/// The point(s) where a collision occurred, exactly on the surface of the related object.
#[derive(Clone, Copy, Debug)]
pub struct Manifold(pub Point2<f32>, pub Option<Point2<f32>>);

impl Manifold {
    /// If there are two points in the manifold, returns the center of the line segment between them.
    /// Otherwise returns the one point.
    pub fn center(&self) -> Point2<f32> {
        let p1 = self.0;
        if let Some(p2) = self.1 {
            Point2::new(0.5 * (p1.x + p2.x), 0.5 * (p1.y + p2.y))
        } else {
            p1
        }
    }

    /// Execute a closure for all points present in the manifold.
    pub fn for_each<F: FnMut(&Point2<f32>)>(&self, mut f: F) {
        f(&self.0);
        if let Some(p) = self.1 {
            f(&p);
        }
    }

    /// Transform a manifold with a closure.
    pub fn map<F: FnMut(Point2<f32>) -> Point2<f32>>(self, mut f: F) -> Self {
        Manifold(f(self.0), self.1.map(f))
    }
}
