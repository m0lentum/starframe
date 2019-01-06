use crate::componentcontainer::ComponentContainer;
use crate::storage::{ComponentStorage, CreateWithCapacity};
use crate::system::{System, SystemRunner};
use crate::IdType;

use hibitset::{BitSet, BitSetLike};
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// An Entity-Component-System environment.
pub struct Space {
    alive_objects: BitSet,
    next_obj_id: IdType,
    capacity: IdType,
    components: HashMap<TypeId, Box<dyn Any>>,
    on_full: Box<FnMut()>,
}

impl Space {
    /// Create a Space with a given maximum capacity.
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            next_obj_id: 0,
            capacity: capacity,
            components: HashMap::new(),
            on_full: Box::new(default_full_error),
        }
    }

    /// Add a component container to a space in a Builder-like fashion.
    /// See Space::create_object for a usage example.
    pub fn with_container<T, S>(mut self) -> Self
    where
        T: 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.components.insert(
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
            self.alive_objects.add(id as u32);

            self.next_obj_id += 1;

            ObjectBuilder {
                id: Some(id),
                space: self,
            }
        } else {
            // find a dead object
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    println!("Replaced dead object {}", id);
                    self.alive_objects.add(id);
                    ObjectBuilder {
                        id: Some(id as IdType),
                        space: self,
                    }
                }
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

    /// Mark an object as dead. Does not actually destroy it, but
    /// none of its components will receive updates anymore and they
    /// can be replaced by something new.
    pub fn destroy_object(&mut self, id: IdType) {
        self.alive_objects.remove(id as u32);
    }

    /// Run a System on all objects with components that match the System's types.
    /// For more information see the moleengine_ecs-codegen crate.
    /// # Panics
    /// Panics if the System being run requires a Component that doesn't exist in this Space.
    pub fn run_system<S: System>(&self) {
        let result = S::Runner::run(self);
        assert!(
            result.is_some(),
            "Attempted to run a System without all Components present"
        );
    }

    /// Like run_system, but fails silently instead of panicking if required Components are missing.
    /// Usually you would prefer to panic when trying to access stuff that doesn't exist,
    /// because silently failing is rarely a desired behavior,
    /// but this is useful to run prepackaged bundles of Systems (such as renderers for various graphic types)
    /// without requiring all of them to have their related Components present.
    pub fn run_optional_system<S: System>(&self) {
        S::Runner::run(self);
    }

    pub(crate) fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    /// Create a component for an object. The component can be of any type,
    /// but there has to be a ComponentContainer for it in this Space.
    /// # Panics
    /// Panics if there is no ComponentContainer for this type in this Space.
    pub(crate) fn create_component<T: 'static>(&mut self, id: IdType, comp: T) {
        let container = self
            .try_open_container_mut::<T>()
            .expect("Attempted to create a component that doesn't have a container");
        container.insert(id, comp);
    }

    /// Get access to a single ComponentContainer if it exists in this Space, otherwise return None.
    pub(crate) fn try_open_container<T: 'static>(&self) -> Option<&ComponentContainer<T>> {
        Self::get_container::<T>(&self.components)
    }

    /// Get mutable access to a single ComponentContainer if it exists in this Space, otherwise return None.
    pub(crate) fn try_open_container_mut<T: 'static>(
        &mut self,
    ) -> Option<&mut ComponentContainer<T>> {
        Self::get_container_mut::<T>(&mut self.components)
    }

    /// Used internally to get a type-safe reference to a container.
    /// Panics if the container has not been created.
    fn get_container<T: 'static>(
        components: &HashMap<TypeId, Box<dyn Any>>,
    ) -> Option<&ComponentContainer<T>> {
        let raw = components.get(&TypeId::of::<T>())?;
        raw.downcast_ref::<ComponentContainer<T>>()
    }

    /// Used internally to get a type-safe mutable reference to a container.
    /// Panics if the container has not been created.
    fn get_container_mut<T: 'static>(
        components: &mut HashMap<TypeId, Box<dyn Any>>,
    ) -> Option<&mut ComponentContainer<T>> {
        let raw = components.get_mut(&TypeId::of::<T>())?;
        raw.downcast_mut::<ComponentContainer<T>>()
    }
}

fn default_full_error() {
    eprintln!("Error: Attempted to create an object in a full Space");
}

/// Builder type to create game objects with a concise syntax.
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

    // Extract the id from the Builder. This doesn't have much practical use.
    pub fn into_id(self) -> Option<IdType> {
        self.id
    }
}
