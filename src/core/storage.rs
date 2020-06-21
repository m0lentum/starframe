//! Data structures for component storage inside of `Container`s.

use super::container::ContainerInit;

/// Trait allowing generic access to storage types.
pub trait Storage {
    type Item;
    fn new(init: super::container::ContainerInit) -> Self;
    fn insert(&mut self, id: usize, component: Self::Item);
    fn get(&self, id: usize) -> Option<&Self::Item>;
    fn get_mut(&mut self, id: usize) -> Option<&mut Self::Item>;
}

/// The most commonly used Storage type.
/// Stores components in a densely packed array with an additional
/// sparse array mapping object ids to indices in the dense array.
pub struct DenseVecStorage<T> {
    indices: Vec<Option<usize>>,
    items: Vec<T>,
}
impl<T> Storage for DenseVecStorage<T> {
    type Item = T;
    fn new(init: ContainerInit) -> Self {
        let mut indices = Vec::new();
        indices.resize_with(init.capacity, || None);
        let items = Vec::with_capacity(init.capacity);

        DenseVecStorage { indices, items }
    }
    fn insert(&mut self, id: usize, component: T) {
        if let Some(i) = self.indices[id] {
            self.items[i] = component;
        } else {
            self.indices[id] = Some(self.items.len());
            self.items.push(component);
        }
    }
    fn get(&self, id: usize) -> Option<&T> {
        self.indices[id].map(move |i| &self.items[i])
    }
    fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.indices[id].map(move |i| &mut self.items[i])
    }
}

/// A single sparse array indexed with object ids.
/// Useful when almost every object in a Space has a component,
/// such as for `Transform`s.
pub struct VecStorage<T> {
    items: Vec<Option<T>>,
}
impl<T> Storage for VecStorage<T> {
    type Item = T;
    fn new(init: ContainerInit) -> Self {
        let mut items = Vec::new();
        items.resize_with(init.capacity, || None);

        VecStorage { items }
    }
    fn insert(&mut self, id: usize, component: T) {
        self.items[id] = Some(component);
    }
    fn get(&self, id: usize) -> Option<&T> {
        self.items[id].as_ref()
    }
    fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.items[id].as_mut()
    }
}

use std::collections::HashMap;
/// A hashmap-based storage. Good when there are very few objects with this component.
pub struct HashMapStorage<T> {
    items: HashMap<usize, T>,
}
impl<T> Storage for HashMapStorage<T> {
    type Item = T;
    fn new(_init: ContainerInit) -> Self {
        // ignore capacity to save memory; this won't be used to hold that many things
        HashMapStorage {
            items: HashMap::new(),
        }
    }
    fn insert(&mut self, id: usize, component: T) {
        self.items.insert(id, component);
    }
    fn get(&self, id: usize) -> Option<&T> {
        self.items.get(&id)
    }
    fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.items.get_mut(&id)
    }
}

/// Storage for tags that don't contain any data.
pub struct NullStorage(());
impl Storage for NullStorage {
    type Item = ();
    fn new(_init: ContainerInit) -> Self {
        NullStorage(())
    }
    fn insert(&mut self, _id: usize, _comp: ()) {}
    fn get(&self, _id: usize) -> Option<&()> {
        Some(&self.0)
    }
    fn get_mut(&mut self, _id: usize) -> Option<&mut ()> {
        Some(&mut self.0)
    }
}
