use moleengine::{
    ecs::{
        event::{EventListener, EventQueue, SpaceEvent},
        space::{LifecycleEvent, Space},
    },
    physics2d::CollisionEvent,
    util::Transform,
};

#[derive(Clone, Copy)]
pub struct TestChainEvent;

impl SpaceEvent for TestChainEvent {
    fn handle(&self, space: &mut Space) {
        space.run_all_listeners(self);
    }
}

#[derive(Clone, Copy)]
pub struct ChainEventListener;

impl EventListener<TestChainEvent> for ChainEventListener {
    fn run_listener(&mut self, _evt: &TestChainEvent, _space: &Space, _queue: &mut EventQueue) {
        println!("Chain event");
    }
}

#[derive(Clone, Copy)]
pub struct TestCollisionListener;

impl EventListener<CollisionEvent> for TestCollisionListener {
    fn run_listener(&mut self, evt: &CollisionEvent, space: &Space, _q: &mut EventQueue) {
        space.write_component(evt.source, |tr: &mut Transform| {
            tr.rotate(2.0);
        });
    }
}

#[derive(Clone, Copy)]
pub struct LifecycleListener;

impl EventListener<LifecycleEvent> for LifecycleListener {
    fn run_listener(&mut self, evt: &LifecycleEvent, _space: &Space, queue: &mut EventQueue) {
        match evt {
            LifecycleEvent::Destroy(id) => println!("Object got deleted: {}!", id),
            LifecycleEvent::Disable(id) => println!("Object got disabled: {}!", id),
            LifecycleEvent::Enable(id) => println!("Object got enabled: {}!", id),
        }

        queue.push(Box::new(TestChainEvent));
    }
}
