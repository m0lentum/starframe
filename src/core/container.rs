use hb::BitSetLike;
use hibitset as hb;

use super::{space::MasterObjectHandle, IdType};

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
            generations: vec![0; capacity],
            storage,
        }
    }

    pub fn insert(&mut self, obj: &MasterObjectHandle, comp: T) {
        self.users.add(obj.id as u32);
        self.generations[obj.id] = obj.gen;
        self.storage[obj.id] = Some(comp);
    }

    // TODO: also check generation here
    pub fn get(&self, id: IdType) -> Option<&T> {
        self.storage[id].as_ref()
    }

    pub fn get_mut(&mut self, id: IdType) -> Option<&mut T> {
        self.storage[id].as_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (&self.users).iter().map(move |id| {
            self.get(id as IdType).expect(
                "A container's users bitset was out of sync with its content (this is a bug)",
            )
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let users_iter = (&self.users).iter();
        let storage = &mut self.storage;
        users_iter.map(move |id| {
            // convert to pointer and back to tell the compiler this is fine;
            // we know that the bitset iterator won't return the same index twice
            let ptr: *mut T = storage[id as IdType].as_mut().expect(
                "A container's users bitset was out of sync with its content (this is a bug)",
            );
            unsafe { ptr.as_mut().unwrap() }
        })
    }
}
