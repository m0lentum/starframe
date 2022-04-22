use super::Collider;
use crate::{math as m, physics::body};

/// Helper for building compound colliders.
/// Computes total area, moment of inertia and center of mass.
///
/// TODOC: how to actually make the compound collider using this info
pub struct CompoundColliderSetup<'a> {
    pub colliders: &'a [Collider],
}

impl<'a> CompoundColliderSetup<'a> {
    pub fn new(colliders: &'a [Collider]) -> Self {
        Self { colliders }
    }

    pub fn center_of_mass(&self) -> m::Vec2 {
        self.colliders
            .iter()
            .map(|c| c.shape.area() * c.offset.translation)
            .sum::<m::Vec2>()
            / self.colliders.len() as f64
    }

    /// Get the area and second moment of area for this set of colliders,
    /// measured around a given point. Usually this offset would be `self.center_of_mass()`.
    pub fn info_around_point(&self, center: m::Vec2) -> body::ColliderInfo {
        let (mut total_area, mut total_second_moment) = (0.0, 0.0);

        for coll in self.colliders {
            let area = coll.shape.area();
            total_area += area;
            let offset_from_center = coll.offset.translation - center;
            let moment = coll.shape.second_moment_of_area();
            // https://en.wikipedia.org/wiki/Parallel_axis_theorem#Second_moment_of_area
            total_second_moment += moment + area * offset_from_center.mag_sq();
        }

        body::ColliderInfo {
            area: total_area,
            second_moment_of_area: total_second_moment,
        }
    }
}
