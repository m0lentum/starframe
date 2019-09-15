use super::{
    componentcontainer::ComponentContainer,
    event::*,
    storage::{ComponentStorage, CreateWithCapacity, VecStorage},
    system::{ComponentQuery, System},
    ObjectRecipe,
    IdType,
};

use anymap::AnyMap;
use hibitset::{BitSet, BitSetLike};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::collections::HashMap;

/// An Entity-Component-System environment.
pub struct Space {
    alive_objects: BitSet,
    enabled_objects: BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    containers: AnyMap,
    pools: HashMap<&'static str, ObjectPool>,
    global_state: LockedAnyMap,
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
            pools: HashMap::new(),
            global_state: LockedAnyMap::new(),
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

    /// Store some globally accessible state behind a RwLock in this space.
    /// This method can be chained like a builder together with add_container.
    pub fn init_global_state<S: 'static>(&mut self, state: S) -> &mut Self {
        self.global_state.insert(state);

        self
    }

    /// Reserves an object id for use and marks it as alive.
    /// # Panics
    /// Panics if the Space is full.
    pub fn create_object(&mut self) -> ObjectHandle {
        self.try_create_object()
            .expect("Tried to add an object to a full space")
    }

    /// Like create_object, but returns None instead of panicking if the Space is full.
    pub fn try_create_object(&mut self) -> Option<ObjectHandle> {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id);
            Some(ObjectHandle { id, space: self })
        } else {
            // find a dead object
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    self.create_object_at(id as IdType);
                    Some(ObjectHandle {
                        id: id as IdType,
                        space: self,
                    })
                }
                _ => None,
            }
        }
    }

    /// Spawn an object using an ObjectRecipe.
    pub fn spawn(&mut self, recipe: impl ObjectRecipe) {
        recipe.spawn(&mut self.create_object());
    }

    /// Spawn objects described in a RON file into this Space.
    #[cfg(feature = "ron-recipes")]
    pub fn read_ron_file<R>(&mut self, file: std::fs::File) -> Result<(), ron::de::Error>
    where
        R: DeserializeRecipes,
    {
        let mut reader = std::io::BufReader::new(file);
        let mut bytes = Vec::new();
        use std::io::Read;
        reader.read_to_end(&mut bytes)?;

        let mut deser = ron::de::Deserializer::from_bytes(bytes.as_slice())?;
        R::deserialize_into_space(&mut deser, self)
    }

    /// Creates a pool (see ObjectPool) of `count` objects created from the given recipe.
    /// Objects in the pool are disabled by default and can be enabled using `spawn_from_pool(key)`

    // TODO: redo pools with the new recipe

    //pub fn create_pool(&mut self, key: &'static str, count: IdType, recipe: ObjectRecipe) {
    //    let pool = {
    //        let recipe = recipe.start_disabled();
    //
    //            let mut ids = Vec::with_capacity(count);
    //            for _ in 0..count {
    //                ids.push(
    //                    recipe
    //                        .try_apply(self)
    //                        .expect("Could not create pool: Space was full"),
    //                );
    //            }
    //
    //            ObjectPool { ids }
    //        };
    //        self.pools.insert(key, pool);
    //    }

    /// Spawns an object from the pool with the given key and returns its id if successful.
    /// # Panics
    /// Panics if the given key is not associated with any pool
    /// or if a game object inside the pool has been destroyed.
    pub fn spawn_from_pool(&mut self, key: &'static str) -> Option<IdType> {
        let pool = self
            .pools
            .get(key)
            .unwrap_or_else(|| panic!("No pool exists with the key {}", key));

        if let Some(&id) = pool.ids.iter().find(|&&id| {
            assert!(self.is_alive(id), "A pooled game object has been destroyed");
            !self.is_enabled(id)
        }) {
            self.enable_object(id);
            Some(id)
        } else {
            None // no unused objects in pool
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

    /// Destroys every object in the Space. Also destroys all object pools.
    pub fn destroy_all(&mut self) {
        self.alive_objects.clear();
        self.pools.clear();
        for gen in &mut self.generations {
            *gen = 0;
        }
    }

    /// Disable an object. This means it will not receive updates from most Systems.
    /// However, Systems still have access to it and may choose to do something with it.
    pub fn disable_object(&mut self, id: IdType) {
        LifecycleEvent::Disable(id).handle(self); // fire the event so event listeners can do things if they wish
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
    /// Can be used to extract information as an Option or just do what you want to do within the closure.
    /// This should be used sparingly since it needs to get access to a ComponentContainer every time,
    /// and is mainly used from EventListeners and for setting values on objects spawned from a pool.
    pub fn read_component<T: 'static, R>(&self, id: IdType, f: impl FnOnce(&T) -> R) -> Option<R> {
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

    /// Like read_component, but gives you a mutable reference to the component.
    pub fn write_component<T: 'static, R>(
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

    /// Execute a closure if this Space has the desired type of global state data.
    pub fn read_global_state<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let access = self.global_state.read::<T>()?;
        Some(f(&access))
    }

    /// Like read_global_state, but with a mutable reference.
    pub fn write_global_state<T: 'static, R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut access = self.global_state.write::<T>()?;
        Some(f(&mut access))
    }

    /// Run a single System on all objects with containers that match the System's types.
    /// For more information see the moleengine_ecs_codegen crate.
    /// # Panics
    /// Panics if the System being run requires a component that doesn't have a container in this Space.
    pub fn run_system<'a, S: System<'a>>(&mut self, system: &mut S) {
        self.actually_run_system(system)
            .expect("Attempted to run a System without all Components present")
            .run_all(self);
    }

    /// Like run_system, but returns None instead of panicking if a required component is missing.
    pub fn try_run_system<'a, S: System<'a>>(&mut self, system: &mut S) -> Option<()> {
        self.actually_run_system(system).map(|mut evts| {
            evts.run_all(self);
        })
    }

    /// Like try_run_system, but instead of firing generated events immediately, returns them.
    /// This is useful because it allows us to run Systems through an immutable Space reference,
    /// which in turn lets us run them in parallel or chain them from within one another.
    pub fn try_run_system_pass_events<'a, S: System<'a>>(
        &self,
        system: &mut S,
    ) -> Option<EventQueue> {
        self.actually_run_system(system)
    }

    /// Actually runs a system, giving it a queue to put events in if it wants to.
    fn actually_run_system<'a, S: System<'a>>(&self, system: &mut S) -> Option<EventQueue> {
        let mut queue = EventQueue::new();
        let result = S::Query::run_query(self, |cs| system.run_system(cs, self, &mut queue));
        result.map(|()| queue)
    }

    /// Helper function to make running stuff through a ComponentQuery more intuitive.
    pub fn run_query<'a, F: ComponentQuery<'a>>(&self, f: impl FnOnce(&mut [F])) -> Option<()> {
        F::run_query(self, f)
    }

    /// Convenience method to make running new events from within events more intuitive.
    /// Equivalent to `event.handle(space)`.
    pub fn handle_event(&mut self, evt: impl SpaceEvent) {
        evt.handle(self);
    }

    /// Handle a series of SpaceEvents in sequential order.
    pub fn handle_events(&mut self, mut events: EventQueue) {
        events.run_all(self);
    }

    /// Run a listener associated with a specific object and a specific event type.
    pub fn run_listener<E: SpaceEvent + 'static>(&mut self, id: IdType, evt: &E) {
        let mut queue = EventQueue::new();

        self.write_component(id, |l: &mut EventListenerComponent<E>| {
            l.run_listener(&evt, &self, &mut queue)
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
    /// Used by the ComponentQuery derive macro.
    pub fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    /// Returns whether or not the an object with the given id is currently alive.
    pub fn is_alive(&self, id: IdType) -> bool {
        self.alive_objects.contains(id as u32)
    }

    /// Get a reference to the bitset of enabled objects in this space.
    /// Used by the ComponentQuery derive macro.
    pub fn get_enabled(&self) -> &BitSet {
        &self.enabled_objects
    }

    /// Returns whether or not an object with the given id is currently enabled.
    pub fn is_enabled(&self, id: IdType) -> bool {
        self.enabled_objects.contains(id as u32)
    }

    /// Get the generation value of a given object.
    /// Used by the ComponentQuery derive macro.
    pub fn get_gen(&self, id: IdType) -> u8 {
        self.generations[id]
    }

    /// Get access to a single ComponentContainer if it exists in this Space, otherwise return None.
    /// Used by the ComponentQuery derive macro.
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

/// An interface that allows you to add components to an object after creating it.
pub struct ObjectHandle<'a> {
    id: IdType,
    space: &'a mut Space,
}

impl<'a> ObjectHandle<'a> {
    /// Add the given component to the Space and associate it with the object.
    /// See Space::create_object for a usage example.
    pub fn add<T: 'static>(&mut self, component: T) -> &mut Self {
        self.space.create_component(self.id, component);

        self
    }

    /// Add the given EventListener to the Space and associate it with the object.
    /// Internally these are stored as components.
    pub fn add_listener<E: SpaceEvent + 'static>(
        &mut self,
        listener: Box<dyn EventListener<E>>,
    ) -> &mut Self {
        self.add_safe::<_, VecStorage<_>>(EventListenerComponent(listener))
    }

    /// Have the object initially disabled. You'll probably want some mechanism that
    /// enables it later (an object pool, usually).
    pub fn disable(&mut self) -> &mut Self {
        self.space.actually_disable_object(self.id);

        self
    }

    /// Like `add`, but adds a storage to the Space first if one doesn't exist yet.
    /// Creating the containers explicitly before adding objects is strongly encouraged
    /// (i.e. don't use this unless you have a good reason!).
    pub fn add_safe<T, S>(&mut self, component: T) -> &mut Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.space.create_component_safe::<T, S>(self.id, component);

        self
    }

    /// Get the id given to this object by the Space. This is rarely useful.
    pub fn id(&self) -> IdType {
        self.id
    }
}

/// Objects that can read recipes from RON and apply them to a Space.
/// Implementations are auto-generated with the `ecs::recipes!` macro.
#[cfg(feature = "ron-recipes")]
pub trait DeserializeRecipes {
    fn deserialize_into_space<'a, 'de, D>(
        deserializer: D,
        space: &'a mut Space,
    ) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>;
}

/// An AnyMap but with everything under a RwLock for concurrent interior mutable access.
struct LockedAnyMap(AnyMap);

impl LockedAnyMap {
    pub fn new() -> Self {
        LockedAnyMap(AnyMap::new())
    }

    pub fn insert<T: 'static>(&mut self, thing: T) {
        self.0.insert(RwLock::new(thing));
    }

    pub fn read<T: 'static>(&self) -> Option<RwLockReadGuard<T>> {
        Some(self.0.get::<RwLock<T>>()?.read())
    }

    pub fn write<T: 'static>(&self) -> Option<RwLockWriteGuard<T>> {
        Some(self.0.get::<RwLock<T>>()?.write())
    }
}

/// An object pool is a group of identical game objects
/// that handles disabling and enabling said objects.
/// Pools should be used for objects which would otherwise be created and destroyed a lot, such as bullets.
/// When using a pool, disable your objects instead of destroying them so they can be respawned.
/// Otherwise accessing the pool will panic.
///
/// Pools should not be created directly. They are tied to specific Spaces and operated through
/// the Space's interface with `create_pool` and `spawn_from_pool`.
pub struct ObjectPool {
    pub(self) ids: Vec<IdType>,
}
