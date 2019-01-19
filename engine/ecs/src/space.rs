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
    enabled_objects: BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    containers: HashMap<TypeId, Box<dyn Any>>,
}

impl Space {
    /// Create a Space with a given maximum capacity.
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            enabled_objects: BitSet::with_capacity(capacity as u32),
            generations: vec![0; capacity],
            next_obj_id: 0,
            capacity: capacity,
            containers: HashMap::new(),
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

    /// Reserves an object id for use and marks it as alive.
    /// # Panics
    /// Panics if the Space is full.
    pub(self) fn create_object(&mut self) -> IdType {
        self.try_create_object()
            .expect("Tried to add an object to a full space")
    }

    /// Like create_object, but returns None instead of panicking if the Space is full.
    pub(self) fn try_create_object(&mut self) -> Option<IdType> {
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
    pub(self) fn create_component<T: 'static>(&mut self, id: IdType, comp: T) {
        let gen = self.generations[id];
        let container = self
            .try_open_container_mut::<T>()
            .expect("Attempted to create a component that doesn't have a container");
        container.insert(id, gen, comp);
    }

    /// Create a component for an object. If there is no container for it, create one of type S.
    pub(self) fn create_component_safe<T, S>(&mut self, id: IdType, comp: T)
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
    pub(self) fn actually_destroy_object(&mut self, id: IdType) {
        self.alive_objects.remove(id as u32);
    }

    /// Disable an object. This means it will not receive updates from most Systems.
    /// However, Systems still have access to it and may choose to do something with it.
    pub fn disable_object(&mut self, id: IdType) {
        LifecycleEvent::Disable(id).handle(self);
    }

    pub(self) fn actually_disable_object(&mut self, id: IdType) {
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
        let result = S::Filter::run_filter(self, |cs| S::run_system(cs, self, &mut queue));
        result.map(|()| queue)
    }

    /// Helper function to make running stuff through a ComponentFilter more intuitive.
    pub fn run_filter<'a, F: ComponentFilter<'a>>(&self, f: impl FnMut(&mut [F])) -> Option<()> {
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

    pub(crate) fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    pub(crate) fn get_enabled(&self) -> &BitSet {
        &self.enabled_objects
    }

    pub(crate) fn get_gen(&self, id: IdType) -> u8 {
        self.generations[id]
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

/// A reusable builder type that creates objects in a Space with a given set of components.
/// Because this is reusable, all component types used must implement Clone.
/// To create an object from a recipe, call create() or try_create().
/// Recipes can also be cloned, so it's easy to create multiple different variants of one thing.
/// # Example
/// ```
/// let mut space = Space::with_capacity(100)
///     .with_component::<Position, VecStorage<_>>()
///     .with_component::<Shape, VecStorage<_>>();
///
/// let mut recipe = ObjectRecipe::new()
/// recipe
///     .add(Position{x: 1.0, y: 2.0})
///     .add(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]));
///
/// recipe.create(&mut space);
/// recipe.create(&mut space);
/// ```
pub struct ObjectRecipe {
    steps: Vec<Box<dyn ClonableStep>>,
}

impl Clone for ObjectRecipe {
    fn clone(&self) -> Self {
        let mut cloned_steps = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            cloned_steps.push(step.clone_step());
        }
        ObjectRecipe {
            steps: cloned_steps,
        }
    }
}

trait ClonableStep {
    fn clone_step(&self) -> Box<dyn ClonableStep>;
    fn call(&self, space: &mut Space, id: IdType);
}

impl<T> ClonableStep for T
where
    T: Fn(&mut Space, IdType) + Clone + 'static,
{
    fn clone_step(&self) -> Box<dyn ClonableStep> {
        Box::new(self.clone())
    }

    fn call(&self, space: &mut Space, id: IdType) {
        self(space, id);
    }
}

impl ObjectRecipe {
    pub fn new() -> Self {
        ObjectRecipe { steps: Vec::new() }
    }

    /// Add the given component to the recipe.
    pub fn add<T: Clone + 'static>(&mut self, component: T) -> &mut Self {
        self.steps
            .push(Box::new(move |space: &mut Space, id: IdType| {
                space.create_component(id, component.clone())
            }));

        self
    }

    /// Add the given EventListener to the recipe.
    /// Internally these are stored as components.
    pub fn add_listener<E, L>(&mut self, listener: L) -> &mut Self
    where
        E: SpaceEvent + 'static,
        L: EventListener<E> + Clone + 'static,
    {
        self.steps
            .push(Box::new(move |space: &mut Space, id: IdType| {
                space.create_component_safe::<_, VecStorage<_>>(
                    id,
                    EventListenerComponent {
                        listener: Box::new(listener.clone()),
                    },
                );
            }));

        self
    }

    /// Modify a component that already exists in this recipe using a clonable closure.
    /// # Example
    /// ```
    /// let mut thingy = ObjectRecipe::new();
    /// thingy
    ///     .add(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]))
    ///     .add(Position { x: 0.0, y: 0.0 });
    ///
    /// thingy.apply(&mut space);
    /// let offset = -5.0;
    /// thingy.modify(move |pos: &mut Position| pos.x = offset);
    /// thingy.apply(&mut space);
    /// ```
    pub fn modify<T, F>(&mut self, f: F) -> &mut Self 
    where
        T: 'static,
        F: Fn(&mut T) + Clone + 'static
    {
        self.steps
            .push(Box::new(move |space: &mut Space, id: IdType| {
                space.do_with_component_mut(id, f.clone());
            }));

        self
    }

    /// Have the object initially disabled. You'll probably want some mechanism that
    /// enables it later (an object pool, usually).
    pub fn start_disabled(&mut self) -> &mut Self {
        self.steps
            .push(Box::new(move |space: &mut Space, id: IdType| {
                space.actually_disable_object(id);
            }));

        self
    }

    /// Like `add`, but adds a storage to the Space first if one doesn't exist yet.
    /// Creating the containers explicitly before adding objects is strongly encouraged
    /// (i.e. don't use this unless you have a good reason!).
    pub fn add_safe<T, S>(&mut self, component: T) -> &mut Self
    where
        T: Clone + 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.steps
            .push(Box::new(move |space: &mut Space, id: IdType| {
                space.create_component_safe::<T, S>(id, component.clone());
            }));

        self
    }

    /// Use this recipe to create an object in a Space.
    /// Returns the id of the object created.
    /// # Panics
    /// Panics if there is no room left in the Space.
    pub fn apply(&self, space: &mut Space) -> IdType {
        let id = space.create_object();
        for step in &self.steps {
            step.call(space, id);
        }
        id
    }

    /// Like `create`, but returns None instead of panicking if the Space is full.
    pub fn try_apply(&self, space: &mut Space) -> Option<IdType> {
        let id = space.try_create_object();
        match id {
            Some(id) => {
                for step in &self.steps {
                    step.call(space, id);
                }
            }
            None => {}
        }
        id
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
            space: space,
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
