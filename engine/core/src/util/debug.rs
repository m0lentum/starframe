use super::Transform;
use crate::ecs::{space::LifecycleEvent, system::*};
use nalgebra::Point2;

/// Marker component that indicates an object should be used to
#[derive(Clone, Copy)]
pub struct PointVisualizer;

/// System that places PointVisualizer objects at intersections of bodies in the space.
pub struct PointVisualizerSystem;

#[derive(ComponentFilter)]
pub struct PointVisualizerFilter<'a> {
    #[id]
    id: IdType,
    #[enabled]
    enabled: bool,
    tr: &'a mut Transform,
    _marker: &'a PointVisualizer,
}

impl<'a> StatefulSystem<'a> for PointVisualizerSystem {
    type Filter = PointVisualizerFilter<'a>;
    type State = Vec<nalgebra::Point2<f32>>;

    fn run_system(
        self,
        state: &mut Self::State,
        items: &mut [Self::Filter],
        _space: &Space,
        queue: &mut EventQueue,
    ) {
        let mut points = state.iter();
        for item in items {
            if let Some(p) = points.next() {
                item.tr.set_translation(p.coords);
                if !item.enabled {
                    queue.push(Box::new(LifecycleEvent::Enable(item.id)));
                }
            } else if item.enabled {
                queue.push(Box::new(LifecycleEvent::Disable(item.id)));
            }
        }
    }
}
