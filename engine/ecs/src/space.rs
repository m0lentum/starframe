use crate::componentcontainer::ComponentContainer;
use crate::event::*;
use crate::storage::{ComponentStorage, CreateWithCapacity, VecStorage};
use crate::system::{ComponentFilter, System};
use crate::IdType;

use hibitset::{BitSet, BitSetLike};
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// An Entity-Component-System environment.
pub struct Space {
    alive_objects: BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    containers: HashMap<TypeId, Box<dyn Any>>,
    on_full: Box<FnMut()>,
}

impl Space {
    /// Create a Space with a given maximum capacity.
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            generations: vec![0; capacity],
            next_obj_id: 0,
            capacity: capacity,
            containers: HashMap::new(),
            on_full: Box::new(default_full_error),
        }
    }

    /// Add a component container to a space. The first type parameter determines the
    /// type of the component and the second the type of storage to hold it.
    /// # Example
    /// ```
    /// let mut space = Space::with_capacity(100);
    /// space.create_container::<Position, VecStorage<_>>();
    /// ```
    pub fn create_container<T, S>(&mut self)
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.containers.insert(
            TypeId::of::<T>(),
            Box::new(ComponentContainer::new::<S>(self.capacity)),
        );
    }

    /// Add a component container to a space in a Builder-like fashion.
    /// Otherwise, works exactly like Space::create_container.
    /// See Space::create_object for a usage example.
    pub fn with_container<T, S>(mut self) -> Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.containers.insert(
            TypeId::of::<T>(),
            Box::new(ComponentContainer::new::<S>(self.capacity)),
        );

        self
    }

    /// Give your own function to call in case the Space is full when creating an object,
    /// for example to show the player an error message on screen.
    /// By default a message is printed into standard error.
    pub fn with_custom_full_error<F: FnMut() + 'static>(mut self, f: F) -> Self {
        self.on_full = Box::new(f);

        self
    }

    /// Reserves an object id for use and marks it as alive.
    /// Returns an ObjectBuilder struct that you can use to add Components.
    /// Note that the ObjectBuilder borrows the Space so it must be dropped before creating another.
    /// # Example
    /// ```
    /// let mut space = Space::with_capacity(100)
    ///    .with_component::<Position, VecStorage<_>>()
    ///    .with_component::<Velocity, VecStorage<_>>();
    ///
    /// space
    ///    .create_object()
    ///    .with(Position { x: 0.0, y: 0.0 })
    ///    .with(Velocity { x: 1.0, y: 0.5 });
    /// ```
    pub fn create_object(&mut self) -> ObjectBuilder {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id)
        } else {
            // find a dead object
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => self.create_object_at(id as IdType),
                _ => {
                    (self.on_full)();
                    ObjectBuilder {
                        id: None,
                        space: self,
                    }
                }
            }
        }
    }

    fn create_object_at(&mut self, id: IdType) -> ObjectBuilder {
        self.alive_objects.add(id as u32);
        self.generations[id] += 1;

        ObjectBuilder {
            id: Some(id),
            space: self,
        }
    }

    /// Send a LifecycleEvent::Destroy to mark an object as dead. Does not actually destroy it, but
    /// none of its components will receive updates anymore and they can be replaced with something new.
    pub fn destroy_object(&mut self, id: IdType) {
        LifecycleEvent::Destroy(id).handle(self);
    }

    /// Actually destroy an object. This is used internally by LifecycleEvent::Destroy.
    pub(crate) fn actually_destroy_object(&mut self, id: IdType) {
        self.alive_objects.remove(id as u32);
    }

    /// Checks whether an object has a specific type of component.
    /// Mainly used by EventListeners, since Systems have their own way of doing this.
    pub fn has_component<T: 'static>(&self, id: IdType) -> bool {
        match self.try_open_container::<T>() {
            Some(cont) => {
                self.alive_objects.contains(id as u32)
                    && cont.get_users().contains(id as u32)
                    && self.generations[id] == cont.get_gen(id)
            }
            None => false,
        }
    }

    /// Execute a closure if the given object has the desired component. Otherwise returns None.
    /// Can be used to extract information as an Option or just do  what you want to do within the closure.
    /// This should be used sparingly since it needs to get access to a ComponentContainer every time,
    /// and is mainly used from EventListeners.
    pub fn do_with_component<T: 'static, R>(
        &self,
        id: IdType,
        f: impl FnOnce(&T) -> R,
    ) -> Option<R> {
        let cont = self.try_open_container::<T>()?;
        if cont.get_users().contains(id as u32)
            && self.alive_objects.contains(id as u32)
            && self.generations[id] == cont.get_gen(id)
        {
            unsafe { Some(f(cont.read().get(id))) }
        } else {
            None
        }
    }

    /// Like do_with_component, but gives you a mutable reference to the component.
    pub fn do_with_component_mut<T: 'static, R>(
        &self,
        id: IdType,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let cont = self.try_open_container::<T>()?;
        if cont.get_users().contains(id as u32)
            && self.alive_objects.contains(id as u32)
            && self.generations[id] == cont.get_gen(id)
        {
            unsafe { Some(f(cont.write().get_mut(id))) }
        } else {
            None
        }
    }

    /// Run a single System on all objects with containers that match the System's types.
    /// For more information see the moleengine_ecs_codegen crate.
    /// # Panics
    /// Panics if the System being run requires a Component that doesn't exist in this Space.
    pub fn run_system<'a, S: System<'a>>(&mut self) {
        self.run_system_internal::<S>()
            .expect("Attempted to run a System without all Components present")
            .run_all(self);
    }

    /// Like run_system, but returns None instead of panicking if a required component is missing.
    pub fn try_run_system<'a, S: System<'a>>(&mut self) -> Option<()> {
        self.run_system_internal::<S>().map(|mut evts| {
            evts.run_all(self);
            ()
        })
    }

    /// Actually runs a system, giving it a queue to put events in if it wants to.
    pub(crate) fn run_system_internal<'a, S: System<'a>>(&self) -> Option<EventQueue> {
        let mut queue = EventQueue::new();
        let result = S::Filter::run(self, |cs| S::operate(cs, self, &mut queue));
        result.map(|()| queue)
    }

    /// Helper function to make running stuff through a ComponentFilter more intuitive.
    pub fn run_filter<'a, F: ComponentFilter<'a>>(&self, f: impl FnMut(&mut [F])) -> Option<()> {
        F::run(self, f)
    }

    /// Convenience method to make running new events from within events more intuitive.
    /// Equivalent to `event.handle(space)`.
    pub fn handle_event(&mut self, evt: impl SpaceEvent) {
        evt.handle(self);
    }

    /// Handle a series of SpaceEvents in sequential order.
    pub fn handle_events(&mut self, events: Vec<Box<dyn SpaceEvent>>) {
        for event in events {
            event.handle(self);
        }
    }

    /// Run a listener associated with a specific object and a specific event type.
    pub fn run_listener<E: SpaceEvent + 'static>(&mut self, id: IdType, evt: &E) {
        let mut queue = EventQueue::new();

        self.do_with_component_mut(id, |l: &mut EventListenerComponent<E>| {
            l.listener.run(&evt, &mut queue)
        });

        queue.run_all(self);
    }

    /// Run every listener associated with the given event.
    /// Use this for events that are not associated with a specific object.
    pub fn run_all_listeners<E: SpaceEvent + 'static>(&mut self, evt: &E) {
        let mut queue = EventQueue::new();

        if let Some(listeners_cont) = self.try_open_container::<EventListenerComponent<E>>() {
            let mut listeners = listeners_cont.write();
            let users = hibitset::BitSetAnd(&self.alive_objects, listeners_cont.get_users());
            for id in users.iter().filter(|id| {
                self.generations[*id as IdType] == listeners_cont.get_gen(*id as IdType)
            }) {
                let l_c = unsafe { listeners.get_mut(id as IdType) };
                l_c.listener.run(&evt, &mut queue);
            }
        }

        queue.run_all(self);
    }

    pub(crate) fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    pub(crate) fn get_gen(&self, id: IdType) -> u8 {
        self.generations[id]
    }

    /// Create a component for an object. The component can be of any type,
    /// but there has to be a ComponentContainer for it in this Space.
    /// # Panics
    /// Panics if there is no ComponentContainer for this type in this Space.
    pub(crate) fn create_component<T: 'static>(&mut self, id: IdType, comp: T) {
        let gen = self.generations[id];
        let container = self
            .try_open_container_mut::<T>()
            .expect("Attempted to create a component that doesn't have a container");
        container.insert(id, gen, comp);
    }

    /// Create a component for an object. If there is no container for it, create one of type S.
    pub(crate) fn create_component_safe<T, S>(&mut self, id: IdType, comp: T)
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        let gen = self.generations[id];
        match self.try_open_container_mut::<T>() {
            Some(cont) => cont.insert(id, gen, comp),
            None => {
                self.create_container::<T, S>();
                self.try_open_container_mut::<T>()
                    .unwrap()
                    .insert(id, gen, comp);
            }
        }
    }

    /// Get access to a single ComponentContainer if it exists in this Space, otherwise return None.
    pub(crate) fn try_open_container<T: 'static>(&self) -> Option<&ComponentContainer<T>> {
        Self::get_container::<T>(&self.containers)
    }

    /// Get mutable access to a single ComponentContainer if it exists in this Space, otherwise return None.
    pub(crate) fn try_open_container_mut<T: 'static>(
        &mut self,
    ) -> Option<&mut ComponentContainer<T>> {
        Self::get_container_mut::<T>(&mut self.containers)
    }

    /// Used internally to get a type-safe reference to a container.
    /// Panics if the container has not been created.
    fn get_container<T: 'static>(
        containers: &HashMap<TypeId, Box<dyn Any>>,
    ) -> Option<&ComponentContainer<T>> {
        let raw = containers.get(&TypeId::of::<T>())?;
        raw.downcast_ref::<ComponentContainer<T>>()
    }

    /// Used internally to get a type-safe mutable reference to a container.
    /// Panics if the container has not been created.
    fn get_container_mut<T: 'static>(
        containers: &mut HashMap<TypeId, Box<dyn Any>>,
    ) -> Option<&mut ComponentContainer<T>> {
        let raw = containers.get_mut(&TypeId::of::<T>())?;
        raw.downcast_mut::<ComponentContainer<T>>()
    }
}

fn default_full_error() {
    eprintln!("Error: Attempted to create an object in a full Space");
}

/// Builder type to create game objects with a concise syntax.
/// Note that this actually executes its operations immediately and does not have
/// a finalizing method you need to call at the end.
/// If the Space is full, the same syntax still works but nothing is created.
/// In this case the Space's full error handler will be called instead.
pub struct ObjectBuilder<'a> {
    id: Option<IdType>,
    space: &'a mut Space,
}

impl ObjectBuilder<'_> {
    /// Add the given component to the Space and associate it with the object.
    /// See Space::create_object for a usage example.
    pub fn with<T: 'static>(self, component: T) -> Self {
        match self.id {
            Some(id) => self.space.create_component(id, component),
            None => (),
        }

        self
    }

    /// Like `with`, but adds a storage to the Space first if one doesn't exist yet.
    pub fn with_safe<T, S>(self, component: T) -> Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        match self.id {
            Some(id) => self.space.create_component_safe::<T, S>(id, component),
            None => (),
        }

        self
    }

    /// Like `with_safe`, but adds a VecStorage specifically, so you don't
    /// need to specify which storage type to use.
    pub fn with_safe_default<T: 'static>(self, component: T) -> Self {
        self.with_safe::<_, VecStorage<_>>(component)
    }

    /// Add the given EventListener to the Space and associate it with the object.
    /// Internally these are stored as components.
    pub fn with_listener<E: SpaceEvent + 'static>(
        self,
        listener: Box<dyn EventListener<E>>,
    ) -> Self {
        self.with_safe::<_, VecStorage<_>>(EventListenerComponent { listener })
    }

    /// Get the id given to this object by the Space. This is rarely useful.
    pub fn get_id(&self) -> Option<IdType> {
        self.id
    }
}

/// Events which handle object lifecycles.
/// Variants should be self-explanatory.
/// # Listener behavior
/// Listeners are run only on objects associated with the event.
pub enum LifecycleEvent {
    Destroy(IdType),
    Disable(IdType),
    Enable(IdType),
}

impl SpaceEvent for LifecycleEvent {
    fn handle(&self, space: &mut Space) {
        use self::LifecycleEvent::*;
        match self {
            Destroy(id) => {
                space.run_listener(*id, self);
                space.actually_destroy_object(*id);
            }
            Disable(_id) => unimplemented! {},
            Enable(_id) => unimplemented! {},
        }
    }
}
