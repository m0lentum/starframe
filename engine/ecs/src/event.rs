use crate::space::Space;

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
    fn run(&mut self, evt: &E, queue: &mut EventQueue);
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

    /// Run all the events in the queue and drain it so they can't be run again.
    pub(crate) fn run_all(&mut self, space: &mut Space) {
        for evt in self.content.drain(..) {
            evt.handle(space);
        }
    }
}

/// Wrapper used to store EventListeners as components in a more readable manner.
/// In most cases users need not touch this type.
pub struct EventListenerComponent<E: SpaceEvent> {
    pub listener: Box<dyn EventListener<E>>,
}
