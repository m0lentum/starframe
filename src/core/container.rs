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

    pub fn iter<'a>(&'a self) -> IterBuilder<&'a T, &'a hb::BitSet, impl FnMut(IdType) -> &'a T> {
        IterBuilder {
            bits: &self.users,
            get: move |id| self.get(id).expect("Bug!!!"),
        }
    }

    pub fn iter_mut<'a>(
        &'a mut self,
    ) -> IterBuilder<&'a mut T, &'a hb::BitSet, impl FnMut(IdType) -> &'a mut T> {
        let storage = &mut self.storage;
        IterBuilder {
            bits: &self.users,
            get: move |id| {
                // the bitset iterator won't return the same id twice
                // so we can safely alias mutable references here
                let storage_ptr: *mut _ = storage;
                let storage_ref = unsafe { storage_ptr.as_mut().unwrap() };
                storage_ref[id].as_mut().expect("Bug!!!")
            },
        }
    }
}

pub struct IterBuilder<Item, Bits: hb::BitSetLike, Get: FnMut(IdType) -> Item> {
    bits: Bits,
    get: Get,
}
impl<Item, Bits: hb::BitSetLike, Get: FnMut(IdType) -> Item> IterBuilder<Item, Bits, Get> {
    pub fn and<OI, OB: hb::BitSetLike, OG: FnMut(IdType) -> OI>(
        self,
        other: IterBuilder<OI, OB, OG>,
    ) -> IterBuilder<(Item, OI), hb::BitSetAnd<Bits, OB>, impl FnMut(IdType) -> (Item, OI)> {
        let mut gets = (self.get, other.get);
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, other.bits),
            get: move |id| ((gets.0)(id), (gets.1)(id)),
        }
    }

    pub fn build(self) -> Iter<Item, Bits, Get> {
        Iter {
            bit_iter: self.bits.iter(),
            get: self.get,
        }
    }
}
pub struct Iter<Item, Bits: hb::BitSetLike, Get: FnMut(IdType) -> Item> {
    bit_iter: hb::BitIter<Bits>,
    get: Get,
}
impl<Item, Bits: hb::BitSetLike, Get: FnMut(IdType) -> Item> Iterator for Iter<Item, Bits, Get> {
    type Item = Item;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.bit_iter.next()? as IdType;
        Some((self.get)(id))
    }
}
