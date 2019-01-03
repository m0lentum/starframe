use crate::componentcontainer::ComponentContainer;
use crate::storage::ComponentStorage;
use crate::system::{System, SystemRunner};
use crate::IdType;

use hibitset::BitSet;
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub struct Space {
    alive_objects: BitSet,
    next_obj_id: IdType,
    capacity: IdType,
    components: HashMap<TypeId, Box<dyn Any>>,
}

impl Space {
    pub fn new(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            next_obj_id: 0,
            capacity: capacity,
            components: HashMap::new(),
        }
    }

    pub fn get_alive(&self) -> &BitSet {
        &self.alive_objects
    }

    /// Reserves an object id for use and marks it as alive.
    /// If the space is full, returns None.
    pub fn create_object(&mut self) -> Option<IdType> {
        if self.next_obj_id >= self.capacity {
            return None;
            // TODO: Find a dead object and replace it if one exists
        }

        let id = self.next_obj_id;
        self.alive_objects.add(id as u32);

        self.next_obj_id += 1;

        Some(id)
    }

    /// Mark an object as dead. Does not actually destroy it, but
    /// none of its components will receive updates anymore and they
    /// can be replaced by something new.
    pub fn destroy_object(&mut self, id: IdType) {
        self.alive_objects.remove(id as u32);
    }

    /// Create a component for an object. The component can be of any type,
    /// but there has to be a ComponentContainer for it in this Space.
    ///
    /// # Panics
    ///
    /// Panics if there is no ComponentContainer for this type in this Space.
    pub fn create_component<T: 'static>(&mut self, id: IdType, comp: T) {
        let container = Self::get_container_mut::<T>(&mut self.components);
        container.insert(id, comp);
    }

    /// Create a ComponentContainer for a component type (any type)
    /// with a storage type. The storage should be initialized as empty and
    /// given to the Space to finish setting it up.
    ///
    /// See moleengine::ecs::storage for details about available storage types.
    pub fn create_container<T, S>(&mut self, storage: S)
    where
        T: 'static,
        S: 'static + ComponentStorage<T>,
    {
        self.components.insert(
            TypeId::of::<T>(),
            Box::new(ComponentContainer::new(Box::new(storage), self.capacity)),
        );
    }

    /// Get access to a single ComponentContainer.
    pub(crate) fn open<T: 'static>(&self) -> &ComponentContainer<T> {
        Self::get_container::<T>(&self.components)
    }

    pub fn run_system<S: System>(&self) {
        S::Runner::run(self);
    }

    /// Used internally to get a type-safe reference to a container.
    /// Panics if the container has not been created.
    fn get_container<T: 'static>(
        components: &HashMap<TypeId, Box<dyn Any>>,
    ) -> &ComponentContainer<T> {
        let raw = components.get(&TypeId::of::<T>()).unwrap();
        raw.downcast_ref::<ComponentContainer<T>>().unwrap()
    }

    /// Used internally to get a type-safe mutable reference to a container.
    /// Panics if the container has not been created.
    fn get_container_mut<T: 'static>(
        components: &mut HashMap<TypeId, Box<dyn Any>>,
    ) -> &mut ComponentContainer<T> {
        let raw = components.get_mut(&TypeId::of::<T>()).unwrap();
        raw.downcast_mut::<ComponentContainer<T>>().unwrap()
    }
}
