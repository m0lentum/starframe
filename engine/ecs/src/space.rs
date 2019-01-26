use crate::componentcontainer::ComponentContainer;
use crate::event::*;
use crate::storage::{ComponentStorage, CreateWithCapacity, VecStorage};
use crate::system::{ComponentFilter, StatefulSystem, System};
use crate::IdType;

use anymap::AnyMap;
use hibitset::{BitSet, BitSetLike};

/// An Entity-Component-System environment.
pub struct Space {
    alive_objects: BitSet,
    enabled_objects: BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    containers: AnyMap,
    global_data: AnyMap,
}

impl Space {
    /// Create a Space with a given maximum capacity.
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            enabled_objects: BitSet::with_capacity(capacity as u32),
            generations: vec![0; capacity],
            next_obj_id: 0,
            capacity,
            containers: AnyMap::new(),
            global_data: AnyMap::new(),
        }
    }

    /// Add a component container to a space. The first type parameter determines the
    /// type of the component and the second the type of storage to hold it.
    /// This returns &mut Self, so it can be chained like a builder.
    /// # Example
    /// ```
    /// let mut space = Space::with_capacity(100);
    /// space.add_container::<Position, VecStorage<_>>();
    /// ```
    pub fn add_container<T, S>(&mut self) -> &mut Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.containers
            .insert(ComponentContainer::new::<S>(self.capacity));

        self
    }

    /// Store the initial state of a stateful system in this space.
    /// This must be done before attempting to run such a system.
    /// This returns &mut Self, so it can be chained like a builder.
    pub fn init_stateful_system<'a, S: StatefulSystem<'a> + 'static>(
        &mut self,
        system: S,
    ) -> &mut Self {
        self.global_data.insert(system);

        self
    }

    /// Reserves an object id for use and marks it as alive.
    /// # Panics
    /// Panics if the Space is full.
    pub(crate) fn create_object(&mut self) -> IdType {
        self.try_create_object()
            .expect("Tried to add an object to a full space")
    }

    /// Like create_object, but returns None instead of panicking if the Space is full.
    pub(crate) fn try_create_object(&mut self) -> Option<IdType> {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id);
            Some(id)
        } else {
            // find a dead object
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    self.create_object_at(id as IdType);
                    Some(id as IdType)
                }
                _ => None,
            }
        }
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
                self.add_container::<T, S>();
                self.try_open_container_mut::<T>()
                    .unwrap()
                    .insert(id, gen, comp);
            }
        }
    }

    fn create_object_at(&mut self, id: IdType) {
        self.alive_objects.add(id as u32);
        self.enabled_objects.add(id as u32);
        self.generations[id] += 1;
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

    /// Destroys every object in the Space.
    pub fn destroy_all(&mut self) {
        self.alive_objects.clear();
    }

    /// Disable an object. This means it will not receive updates from most Systems.
    /// However, Systems still have access to it and may choose to do something with it.
    pub fn disable_object(&mut self, id: IdType) {
        LifecycleEvent::Disable(id).handle(self);
    }

    pub(crate) fn actually_disable_object(&mut self, id: IdType) {
        self.enabled_objects.remove(id as u32);
    }

    /// Re-enable a disabled object. It will receive updates from all Systems again.
    pub fn enable_object(&mut self, id: IdType) {
        LifecycleEvent::Enable(id).handle(self);
    }

    pub(self) fn actually_enable_object(&mut self, id: IdType) {
        self.enabled_objects.add(id as u32);
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
    /// Panics if the System being run requires a component that doesn't have a container in this Space.
    pub fn run_system<'a, S: System<'a>>(&mut self, system: S) {
        self.actually_run_system(system)
            .expect("Attempted to run a System without all Components present")
            .run_all(self);
    }

    /// Like run_system, but returns None instead of panicking if a required component is missing.
    pub fn try_run_system<'a, S: System<'a>>(&mut self, system: S) -> Option<()> {
        self.actually_run_system(system).map(|mut evts| {
            evts.run_all(self);
        })
    }

    /// Actually runs a system, giving it a queue to put events in if it wants to.
    fn actually_run_system<'a, S: System<'a>>(&self, system: S) -> Option<EventQueue> {
        let mut queue = EventQueue::new();
        let result = S::Filter::run_filter(self, |cs| system.run_system(cs, self, &mut queue));
        result.map(|()| queue)
    }

    /// Run a StatefulSystem. These are like Systems but can store information between updates.
    /// # Panics
    /// Panics if the StatefulSystem being run has not been initialized or requires a component
    /// that doesn't have a container in the Space.
    pub fn run_stateful_system<'a, S: StatefulSystem<'a> + 'static>(&mut self) {
        let system = self
            .global_data
            .get_mut::<S>()
            .expect("Attempted to run an uninitialized StatefulSystem");

        // without this the Space is mutably borrowed by system
        // it is safe because the StatefulSystem has no way to access itself through the
        // immutable reference to the Space that it receives
        let system_detached = unsafe { (system as *mut S).as_mut().unwrap() };

        self.actually_run_stateful(system_detached)
            .expect("Attempted to run a StatefulSystem without all Components present")
            .run_all(self);
    }

    fn actually_run_stateful<'a, S: StatefulSystem<'a>>(
        &self,
        system: &mut S,
    ) -> Option<EventQueue> {
        let mut queue = EventQueue::new();
        let result = S::Filter::run_filter(self, |cs| system.run_system(cs, self, &mut queue));
        result.map(|()| queue)
    }

    /// Helper function to make running stuff through a ComponentFilter more intuitive.
    pub fn run_filter<'a, F: ComponentFilter<'a>>(&self, f: impl FnOnce(&mut [F])) -> Option<()> {
        F::run_filter(self, f)
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
            l.listener.run_listener(&evt, &mut queue)
        });

        queue.run_all(self);
    }

    /// Run every listener associated with the given event.
    /// Use this for events that are not associated with a specific object.
    pub fn run_all_listeners<E: SpaceEvent + 'static>(&mut self, evt: &E) {
        let mut queue = EventQueue::new();

        EventPropagator(evt).propagate(self, &mut queue);

        queue.run_all(self);
    }

    /// Get a reference to the bitset of alive objects in this space.
    /// Used by the ComponentFilter derive macro.
    pub fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    /// Get a reference to the bitset of enabled objects in this space.
    /// Used by the ComponentFilter derive macro.
    pub fn get_enabled(&self) -> &BitSet {
        &self.enabled_objects
    }

    /// Get the generation value of a given object.
    /// Used by the ComponentFilter derive macro.
    pub fn get_gen(&self, id: IdType) -> u8 {
        self.generations[id]
    }

    /// Get access to a single ComponentContainer if it exists in this Space, otherwise return None.
    /// Used by the ComponentFilter derive macro.
    pub fn try_open_container<T: 'static>(&self) -> Option<&ComponentContainer<T>> {
        self.containers.get::<ComponentContainer<T>>()
    }

    /// Get mutable access to a single ComponentContainer if it exists in this Space, otherwise return None.
    fn try_open_container_mut<T: 'static>(&mut self) -> Option<&mut ComponentContainer<T>> {
        self.containers.get_mut::<ComponentContainer<T>>()
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
            Disable(id) => {
                if space.get_enabled().contains(*id as u32) {
                    space.run_listener(*id, self);
                    space.actually_disable_object(*id);
                }
            }
            Enable(id) => {
                if !space.get_enabled().contains(*id as u32) {
                    space.actually_enable_object(*id);
                    space.run_listener(*id, self);
                }
            }
        }
    }
}

/// Builder type to create a one-shot object without the Clone requirement of ObjectRecipe.
/// `with` methods are analogous to ObjectRecipe's `add` methods.
/// Note that this actually executes its operations immediately and does not have
/// a finalizing method you need to call at the end.
/// This carries a mutable reference to the Space so it needs to be dropped before using the Space again.
/// # Example
/// ```
/// let mut space = Space::with_capacity(100)
///    .with_component::<Position, VecStorage<_>>()
///    .with_component::<Shape, VecStorage<_>>();
///
/// ObjectBuilder::create(&mut space)
///    .with(Position { x: 0.0, y: 0.0 })
///    .with(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]));
/// ```
pub struct ObjectBuilder<'a> {
    id: IdType,
    space: &'a mut Space,
}

impl<'a> ObjectBuilder<'a> {
    /// Create a new object in the given Space and attach a Builder to it.
    /// # Panics
    /// Panics if there is no room left in the Space.
    pub fn create(space: &'a mut Space) -> Self {
        ObjectBuilder {
            id: space.create_object(),
            space,
        }
    }

    /// Like `create` but returns None instead of panicking if the Space is full.
    pub fn try_create(space: &'a mut Space) -> Option<Self> {
        space
            .try_create_object()
            .map(move |id| ObjectBuilder { id, space })
    }

    /// Add the given component to the Space and associate it with the object.
    /// See Space::create_object for a usage example.
    pub fn with<T: 'static>(self, component: T) -> Self {
        self.space.create_component(self.id, component);

        self
    }

    /// Add the given EventListener to the Space and associate it with the object.
    /// Internally these are stored as components.
    pub fn with_listener<E: SpaceEvent + 'static>(
        self,
        listener: Box<dyn EventListener<E>>,
    ) -> Self {
        self.with_safe::<_, VecStorage<_>>(EventListenerComponent { listener })
    }

    /// Have the object initially disabled. You'll probably want some mechanism that
    /// enables it later (an object pool, usually).
    pub fn start_disabled(self) -> Self {
        self.space.actually_disable_object(self.id);

        self
    }

    /// Like `with`, but adds a storage to the Space first if one doesn't exist yet.
    /// Creating the containers explicitly before adding objects is strongly encouraged
    /// (i.e. don't use this unless you have a good reason!).
    pub fn with_safe<T, S>(self, component: T) -> Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.space.create_component_safe::<T, S>(self.id, component);

        self
    }

    /// Get the id given to this object by the Space. This is rarely useful.
    pub fn get_id(&self) -> IdType {
        self.id
    }
}
