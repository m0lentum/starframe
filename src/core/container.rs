use hibitset as hb;

use super::IdType;

pub struct Container<T: 'static> {
    users: hb::BitSet,
    generations: Vec<u8>,
    storage: Vec<Option<T>>, // TODO: bring back storages
}

impl<T: 'static> Container<T> {
    pub fn with_capacity(capacity: IdType) -> Self {
        let mut storage = Vec::new();
        storage.resize_with(capacity, || None);
        Container {
            users: hb::BitSet::with_capacity(capacity as u32),
            generations: Vec::with_capacity(capacity),
            storage,
        }
    }

    pub fn insert(&mut self, id: IdType, gen: u8, comp: T) {
        self.users.add(id as u32);
        self.generations[id] = gen;
        self.storage[id] = Some(comp);
    }

    pub fn users(&self) -> &hb::BitSet {
        &self.users
    }
}
