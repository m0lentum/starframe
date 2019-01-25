use crate::storage::{ComponentStorage, CreateWithCapacity};
use crate::IdType;
use hibitset::BitSet;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub(crate) type WriteAccess<'a, T> = RwLockWriteGuard<'a, Box<dyn ComponentStorage<T>>>;
pub(crate) type ReadAccess<'a, T> = RwLockReadGuard<'a, Box<dyn ComponentStorage<T>>>;

/// A generic container for components that keeps track of users.
/// Space handles all the updates for you - none of this should be directly accessed by the user.
pub struct ComponentContainer<T: 'static> {
    users: BitSet,
    generations: Vec<u8>,
    storage: RwLock<Box<dyn ComponentStorage<T>>>,
}

impl<T> ComponentContainer<T> {
    pub fn new<S>(capacity: IdType) -> Self
    where
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        ComponentContainer {
            storage: RwLock::new(Box::new(S::with_capacity(capacity))),
            generations: vec![0; capacity],
            users: BitSet::with_capacity(capacity as u32),
        }
    }

    pub fn insert(&mut self, id: IdType, gen: u8, comp: T) {
        self.users.add(id as u32);
        self.generations[id] = gen;
        self.write().insert(id, comp);
    }

    pub fn get_users(&self) -> &BitSet {
        &self.users
    }

    pub fn get_gen(&self, id: IdType) -> u8 {
        self.generations[id]
    }

    /// Get read access to the underlying storage.
    /// # Panics
    /// Panics if the storage is poisoned or the current thread already has a lock.
    pub fn read(&self) -> ReadAccess<'_, T> {
        self.storage
            .read()
            .expect("Read access to a ComponentContainer failed")
    }

    /// Get write access to the underlying storage.
    /// # Panics
    /// Panics if the storage is poisoned or the current thread already has a lock.
    pub fn write(&self) -> WriteAccess<'_, T> {
        self.storage
            .write()
            .expect("Write access to a ComponentContainer failed")
    }
}
