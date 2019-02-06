use crate::Collision;
use moleengine::ecs::{space::LifecycleEvent, system::*};
use moleengine::util::Transform;

/// Marker component that indicates an object should be used to
#[derive(Clone, Copy)]
pub struct CollisionVisualizer;

/// System that places PointVisualizer objects at intersections of bodies in the space.
pub struct CollisionVisualizerSystem;

#[derive(ComponentFilter)]
pub struct CollisionVisualizerFilter<'a> {
    #[id]
    id: IdType,
    #[enabled]
    enabled: bool,
    tr: &'a mut Transform,
    _marker: &'a CollisionVisualizer,
}

impl<'a> StatefulSystem<'a> for CollisionVisualizerSystem {
    type Filter = CollisionVisualizerFilter<'a>;
    type State = Vec<Collision>;

    fn run_system(
        self,
        colls: &mut Self::State,
        items: &mut [Self::Filter],
        _space: &Space,
        queue: &mut EventQueue,
    ) {
        let mut items = items.iter_mut();
        for coll in colls {
            if let Some(item) = items.next() {
                item.tr.set_translation(coll.manifold.0.coords);
                if !item.enabled {
                    queue.push(Box::new(LifecycleEvent::Enable(item.id)));
                }
            }
            if let Some(p) = coll.manifold.1 {
                if let Some(item) = items.next() {
                    item.tr.set_translation(p.coords);
                    if !item.enabled {
                        queue.push(Box::new(LifecycleEvent::Enable(item.id)));
                    }
                }
            }
        }

        // disable remaining items
        for item in items {
            queue.push(Box::new(LifecycleEvent::Disable(item.id)));
        }
    }
}
