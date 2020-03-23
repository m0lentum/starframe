//! Data structures for component storage inside of `Container`s.

/// Trait allowing generic access to storage types.
pub trait Storage {
    type Item;
    fn with_capacity(capacity: usize) -> Self;
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
    fn with_capacity(capacity: usize) -> Self {
        let mut indices = Vec::new();
        indices.resize_with(capacity, || None);
        let items = Vec::with_capacity(capacity);

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
    fn with_capacity(capacity: usize) -> Self {
        let mut items = Vec::new();
        items.resize_with(capacity, || None);

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
