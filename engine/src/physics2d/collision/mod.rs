use crate::physics2d::Collider;

use crate::ecs::{event::SpaceEvent, space::Space, IdType};
use crate::util::Transform;
use nalgebra::{Point2, Unit, Vector2};

pub mod debug;
mod queries;
mod solver;
pub use solver::RigidBodySolver;

/// Information about a collision relative to one of the objects involved.
/// Two of these are generated for every colliding pair.
/// They also function as SpaceEvents and can be listened to.
/// # Event behavior
/// Only the listener for the involved object is called.
#[derive(Clone, Copy, Debug)]
pub struct Collision {
    pub source: IdType,
    pub other: IdType,
    pub normal: Unit<Vector2<f32>>,
    pub depth: f32,
    pub manifold: Manifold,
}

impl SpaceEvent for Collision {
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
}

/// Checks two transformed colliders for intersection. If one is found,
/// returns two Collisions, one relative to each of the participating objects.
pub fn intersection_check(
    obj1: IdType,
    tr1: &Transform,
    coll1: &Collider,
    obj2: IdType,
    tr2: &Transform,
    coll2: &Collider,
) -> Option<[Collision; 2]> {
    use Collider::*;
    match (coll1, coll2) {
        (Circle { r: r1 }, Circle { r: r2 }) => {
            queries::circle_circle(obj1, tr1, *r1, obj2, tr2, *r2)
        }
        (Circle { .. }, Rect { .. }) => None,
        (Rect { .. }, Circle { .. }) => None,
        (Rect { hw: hw1, hh: hh1 }, Rect { hw: hw2, hh: hh2 }) => {
            queries::rect_rect(obj1, tr1, *hw1, *hh1, obj2, tr2, *hw2, *hh2)
        }
    }
}
