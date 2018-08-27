use ecs::space::IdType;
use ecs::storage::ComponentStorage;
use hibitset::BitSet;

/// A generic container for components that keeps track of users.
/// Space handles all the updates for you - none of this should be directly accessed by the user.
pub struct ComponentContainer<T> {
    pub storage: Box<ComponentStorage<T>>,
    pub users: BitSet,
}

impl<T> ComponentContainer<T> {
    pub fn new(storage: Box<ComponentStorage<T>>, capacity: IdType) -> Self {
        let mut new_container = ComponentContainer {
            storage: storage,
            users: BitSet::with_capacity(capacity as u32),
        };
        new_container.storage.reserve(capacity);

        new_container
    }

    pub fn insert(&mut self, id: IdType, comp: T) {
        self.users.add(id as u32);
        self.storage.insert(id, comp);
    }
}
