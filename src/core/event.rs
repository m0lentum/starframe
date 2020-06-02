use super::space::Id;

pub enum Event {
    Lifecycle(LifecycleEvent),
    TimerTick,
    //Collision(crate::physics::CollisionEvent),
}

pub enum LifecycleEvent {
    Destroyed,
    Created,
}

pub struct EventQueue {
    events: Vec<(Id, Event)>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, id: Id, evt: Event) {
        self.events.push((id, evt));
    }

    pub fn drain(&mut self) -> std::vec::Drain<(Id, Event)> {
        self.events.drain(..)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
