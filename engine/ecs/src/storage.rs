use crate::IdType;

use std::ptr::write;

/// Generic storage to be used internally by ComponentContainer.
/// None of these methods should ever be called directly by the user.
pub trait ComponentStorage<T> {
    /// This will always be called by the Space with its maximum number of objects upon creating the storage.
    /// Therefore, storage constructors should make the underlying
    /// collection empty and reserve memory for the whole thing in reserve().
    fn reserve(&mut self, cap: IdType);

    /// Get an immutable reference to a component.
    ///
    /// # Safety
    ///
    /// This is unsafe because it may result in undefined behavior if
    /// there is no component for the requested id.
    unsafe fn get(&self, id: IdType) -> &T;

    /// Get a mutable reference to a component.
    ///
    /// # Safety
    ///
    /// This is unsafe because it may result in undefined behavior if
    /// there is no component for the requested id.
    unsafe fn get_mut(&mut self, id: IdType) -> &mut T;

    /// Get a mutable raw pointer to a component.
    /// This is used to allow Systems access to multiple references within the container at one time.
    ///
    /// # Safety
    ///
    /// This is unsafe for the same reason as get and get_mut, and additionally because
    /// it is not guaranteed that this won't alias.
    unsafe fn get_mut_raw(&mut self, id: IdType) -> *mut T {
        self.get_mut(id) as *mut T
    }

    /// Insert a new component at position id.
    fn insert(&mut self, id: IdType, comp: T);
}

/// A sparse vector container. Components are stored in a single Vec
/// indexed by their id. Unused positions in the vector are left unallocated.
pub struct VecStorage<T> {
    content: Vec<T>,
}

impl<T> VecStorage<T> {
    pub fn new() -> Self {
        VecStorage {
            content: Vec::new(),
        }
    }
}

impl<T> ComponentStorage<T> for VecStorage<T> {
    fn reserve(&mut self, cap: IdType) {
        self.content.reserve_exact(cap);
        unsafe {
            self.content.set_len(cap);
        }
    }

    unsafe fn get(&self, id: IdType) -> &T {
        assert!(id < self.content.len());
        &self.content[id]
    }

    unsafe fn get_mut(&mut self, id: IdType) -> &mut T {
        assert!(id < self.content.len());
        &mut self.content[id]
    }

    fn insert(&mut self, id: IdType, comp: T) {
        assert!(id < self.content.len());
        let ptr: *mut T = &mut self.content[id];
        unsafe {
            write(ptr, comp);
        }
    }
}
