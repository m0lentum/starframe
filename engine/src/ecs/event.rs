use super::space::Space;
use super::system::*;

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
/// For now, these can only be stored as components to an object in the Space.
pub trait EventListener<E: SpaceEvent> {
    /// The logic to run when a SpaceEvent happens.
    /// Pushing new events onto the EventQueue given as a parameter results in
    /// those events being run later in sequential order.
    fn run_listener(&mut self, evt: &E, space: &Space, queue: &mut EventQueue);
}

/// A collection of SpaceEvents, used to allow event listeners and systems to generate new events
/// without need to give them mutable access to a Space.
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

    /// Move everything from another queue into this one, leaving it empty.
    pub fn append(&mut self, other: &mut EventQueue) {
        self.content.append(&mut other.content);
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
