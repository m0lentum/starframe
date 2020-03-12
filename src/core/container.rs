use hibitset as hb;

use super::space::MasterKey;

#[derive(Clone, Copy)]
pub struct Init {
    pub(crate) capacity: usize,
}

pub struct Container<T: 'static> {
    users: hb::BitSet,
    storage: Vec<Option<T>>, // TODO: bring back storages
}

pub trait AsDyn {
    fn users(&mut self) -> &mut hb::BitSet;
}

pub type DynRefs<'a> = Vec<&'a mut dyn AsDyn>;

impl<T: 'static> AsDyn for Container<T> {
    fn users(&mut self) -> &mut hb::BitSet {
        &mut self.users
    }
}

impl<T: 'static> Container<T> {
    pub fn new(init: Init) -> Self {
        let mut storage = Vec::new();
        storage.resize_with(init.capacity, || None);
        Container {
            users: hb::BitSet::with_capacity(init.capacity as u32),
            storage,
        }
    }

    pub fn insert(&mut self, key: MasterKey, comp: T) {
        self.users.add(key.id as u32);
        self.storage[key.id] = Some(comp);
    }

    pub fn get(&self, id: usize) -> Option<&T> {
        self.storage[id].as_ref()
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.storage[id].as_mut()
    }

    pub fn iter<'a>(&'a self) -> IterBuilder<&'a T, &'a hb::BitSet, impl FnMut(usize) -> &'a T> {
        IterBuilder {
            bits: &self.users,
            get: move |id| self.get(id).expect("Bug!!!"),
        }
    }

    pub fn iter_mut<'a>(
        &'a mut self,
    ) -> IterBuilder<&'a mut T, &'a hb::BitSet, impl FnMut(usize) -> &'a mut T> {
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

pub struct IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(usize) -> Item,
{
    bits: Bits,
    get: Get,
}
impl<Item, Bits, Get> IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(usize) -> Item,
{
    pub fn and<OI, OB: hb::BitSetLike, OG: FnMut(usize) -> OI>(
        self,
        other: IterBuilder<OI, OB, OG>,
    ) -> IterBuilder<(Item, OI), hb::BitSetAnd<Bits, OB>, impl FnMut(usize) -> (Item, OI)> {
        let mut gets = (self.get, other.get);
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, other.bits),
            get: move |id| ((gets.0)(id), (gets.1)(id)),
        }
    }

    pub fn not<OI, OB: hb::BitSetLike, OG: FnMut(usize) -> OI>(
        self,
        other: IterBuilder<OI, OB, OG>,
    ) -> IterBuilder<Item, hb::BitSetAnd<Bits, hb::BitSetNot<OB>>, impl FnMut(usize) -> Item> {
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, hb::BitSetNot(other.bits)),
            get: self.get,
        }
    }
}
impl<Item, Bits, Get> IntoIterator for IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(usize) -> Item,
{
    type Item = Item;
    type IntoIter = Iter<Item, Bits, Get>;
    fn into_iter(self) -> Self::IntoIter {
        Iter {
            bit_iter: self.bits.iter(),
            get: self.get,
        }
    }
}

pub struct Iter<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(usize) -> Item,
{
    bit_iter: hb::BitIter<Bits>,
    get: Get,
}
impl<Item, Bits, Get> Iterator for Iter<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(usize) -> Item,
{
    type Item = Item;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.bit_iter.next()? as usize;
        Some((self.get)(id))
    }
}
