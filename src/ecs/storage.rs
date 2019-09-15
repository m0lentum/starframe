use super::IdType;

use std::ptr::write;

/// Generic storage to be used internally by ComponentContainer.
/// None of these methods should ever be called directly by the user.
pub trait ComponentStorage<T> {
    /// Get an immutable reference to a component.
    /// # Safety
    /// This is unsafe because it may result in undefined behavior if
    /// there is no component for the requested id.
    unsafe fn get(&self, id: IdType) -> &T;

    /// Get a mutable reference to a component.
    /// # Safety
    /// This is unsafe because it may result in undefined behavior if
    /// there is no component for the requested id.
    unsafe fn get_mut(&mut self, id: IdType) -> &mut T;

    /// Get an immutable raw pointer to a component.
    /// This is used to allow ComponentQueries to circumvent some borrowing rules
    /// and create a slice of references.
    /// # Safety
    /// This is unsafe for the same reason as get and get_mut.
    unsafe fn get_raw(&self, id: IdType) -> *const T {
        self.get(id) as *const T
    }

    /// Get a mutable raw pointer to a component.
    /// This is used to allow Systems access to multiple references within the container at one time.
    /// # Safety
    /// This is unsafe for the same reason as get and get_mut, and additionally because
    /// it is not guaranteed that this won't alias.
    unsafe fn get_mut_raw(&mut self, id: IdType) -> *mut T {
        self.get_mut(id) as *mut T
    }

    /// Insert a new component at position id.
    fn insert(&mut self, id: IdType, comp: T);
}

/// A Storage must be able to create itself with a given capacity.
/// This trait is separate from ComponentStorage so that ComponentStorage
/// can be used as a trait object.
pub trait CreateWithCapacity {
    fn with_capacity(cap: IdType) -> Self;
}

/// Trait describing the default storage type for a type when used as a component.
pub trait DefaultStorage: Sized + 'static {
    type DefaultStorage: ComponentStorage<Self> + CreateWithCapacity + 'static;
}

/// A sparse vector container. Components are stored in a single Vec
/// indexed by their id. Unused positions in the vector are left uninitialized.
pub struct VecStorage<T> {
    content: Vec<T>,
}

impl<T> CreateWithCapacity for VecStorage<T> {
    fn with_capacity(cap: IdType) -> Self {
        let mut content = Vec::new();
        content.reserve_exact(cap);
        unsafe {
            content.set_len(cap);
        }
        VecStorage { content }
    }
}

impl<T> ComponentStorage<T> for VecStorage<T> {
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
