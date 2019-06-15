use super::{space::LifecycleEvent, system::*, IdType, Space};

/// An event that causes something to happen within a Space.
pub trait SpaceEvent {
    /// The global logic to execute when this event happens.
    /// NOTE: It is up to the implementer to decide which EventListeners receive
    /// the event. Use Space::run_listener to run a listener associated with a
    /// specific object or Space::run_all_listeners to run it for every listener
    /// in the Space.
    fn handle(&self, space: &mut Space);
}

/// Something that does something when a SpaceEvent happens.
/// These can only be stored as components to an object in the Space.
pub trait EventListener<E: SpaceEvent> {
    /// The logic to run when a SpaceEvent happens.
    /// Pushing new events onto the EventQueue given as a parameter results in
    /// those events being run later in sequential order.
    fn run_listener(&mut self, evt: &E, space: &Space, queue: &mut EventQueue);
}

/// A collection of SpaceEvents accumulated during a System running.
/// If the System is run with `Space::run_system`,
/// the queue is consumed and the events fired automatically when the System finishes.
/// If you wish to delay firing of events you can use `Space::run_system_pass_events`,
/// which returns the generated events.
pub struct EventQueue {
    content: Vec<Box<dyn SpaceEvent>>,
}

impl EventQueue {
    /// Create an empty EventQueue.
    pub(crate) fn new() -> Self {
        EventQueue {
            content: Vec::new(),
        }
    }

    /// Push an event to the back of the queue.
    pub fn push(&mut self, evt: Box<dyn SpaceEvent>) {
        self.content.push(evt);
    }

    /// Move everything from another queue into this one, leaving the other queue empty.
    pub fn append(&mut self, other: &mut EventQueue) {
        self.content.append(&mut other.content);
    }

    /// Convenience function to queue the given object to be destroyed.
    /// This is equivalent to `push(Box::new(LifecycleEvent::Destroy(id)))`.
    pub fn destroy_object(&mut self, id: IdType) {
        self.content.push(Box::new(LifecycleEvent::Destroy(id)));
    }

    /// Convenience function to queue the given object to be disabled.
    /// This is equivalent to `push(Box::new(LifecycleEvent::Disable(id)))`.
    pub fn disable_object(&mut self, id: IdType) {
        self.content.push(Box::new(LifecycleEvent::Disable(id)));
    }

    /// Convenience function to queue the given object to be enabled.
    /// This is equivalent to `push(Box::new(LifecycleEvent::Enable(id)))`.
    pub fn enable_object(&mut self, id: IdType) {
        self.content.push(Box::new(LifecycleEvent::Enable(id)));
    }

    /// Run all the events in the queue and drain it so they can't be run again.
    pub(crate) fn run_all(&mut self, space: &mut Space) {
        for evt in self.content.drain(..) {
            evt.handle(space);
        }
    }
}

/// Wrapper used to store EventListeners as components in a more readable manner.
/// In most cases users need not touch this type.
#[derive(shrinkwraprs::Shrinkwrap)]
#[shrinkwrap(mutable)]
pub struct EventListenerComponent<E: SpaceEvent>(pub Box<dyn EventListener<E>>);

pub(crate) struct EventPropagator<'a, E: SpaceEvent + 'static>(pub &'a E);

impl<'a, E: SpaceEvent> EventPropagator<'a, E> {
    pub fn propagate(&self, space: &Space, queue: &mut EventQueue) {
        space.run_filter(|items: &mut [EventListenerFilter<E>]| {
            for item in items {
                item.l.run_listener(self.0, space, queue);
            }
        });
    }
}

#[derive(ComponentFilter)]
struct EventListenerFilter<'a, E: SpaceEvent + 'static> {
    l: &'a mut EventListenerComponent<E>,
}
