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

    pub fn iter<'a>(&'a self) -> Iter<'a, T> {
        Iter {
            bit_iter: (&self.users).iter(),
            container: self,
        }
    }

    pub fn build_iter<'a>(
        &'a self,
    ) -> IterBuilder<&'a Self, &'a T, &'a hb::BitSet, impl FnMut(IdType, &'a Self) -> &'a T> {
        IterBuilder {
            bitset: &self.users,
            containers: &self,
            get: |id, me| me.get(id).expect("Bug!!!"),
        }
    }

    pub fn build_iter_simple(&self) -> SimpleIterBuilder<&'_ hb::BitSet> {
        SimpleIterBuilder { bits: &self.users }
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

// sketch: just get ids, add gets later

pub struct SimpleIterBuilder<B: hb::BitSetLike> {
    bits: B,
}
impl<B: hb::BitSetLike> SimpleIterBuilder<B> {
    pub fn and<'a, T>(
        self,
        c: &'a Container<T>,
    ) -> SimpleIterBuilder<hb::BitSetAnd<B, &'a hb::BitSet>> {
        SimpleIterBuilder {
            bits: hb::BitSetAnd(self.bits, &c.users),
        }
    }

    pub fn build<Item, Getter: FnMut(IdType) -> Item>(
        self,
        get: Getter,
    ) -> SimpleIter<B, Item, Getter> {
        SimpleIter {
            bit_iter: self.bits.iter(),
            get,
        }
    }
}
pub struct SimpleIter<B, Item, Getter>
where
    B: hb::BitSetLike,
    Getter: FnMut(IdType) -> Item,
{
    bit_iter: hb::BitIter<B>,
    get: Getter,
}
impl<B, Item, Getter> Iterator for SimpleIter<B, Item, Getter>
where
    B: hb::BitSetLike,
    Getter: FnMut(IdType) -> Item,
{
    type Item = Item;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.bit_iter.next()? as IdType;
        Some((self.get)(id))
    }
}

// sketch: generics insanity

pub struct IterBuilder<Containers, Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(IdType, Containers) -> Item,
{
    bitset: Bits,
    containers: Containers,
    get: Get,
}
impl<Containers, Item, Bits, Get> IterBuilder<Containers, Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(IdType, Containers) -> Item,
{
    pub fn and<OC, OI, OB: hb::BitSetLike, OG: FnMut(IdType, OC) -> OI>(
        self,
        other: IterBuilder<OC, OI, OB, OG>,
    ) -> IterBuilder<
        (Containers, OC),
        (Item, OI),
        hb::BitSetAnd<Bits, OB>,
        impl FnMut(IdType, (Containers, OC)) -> (Item, OI),
    > {
        let mut gets = (self.get, other.get);
        IterBuilder {
            bitset: hb::BitSetAnd(self.bitset, other.bitset),
            containers: (self.containers, other.containers),
            get: move |id, (mine, theirs)| ((gets.0)(id, mine), (gets.1)(id, theirs)),
        }
    }

    pub fn build(self) -> BuiltIter<Containers, Item, Bits, Get> {
        BuiltIter {
            bit_iter: self.bitset.iter(),
            containers: self.containers,
            get: self.get,
        }
    }
}

pub struct BuiltIter<Containers, Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(IdType, Containers) -> Item,
{
    bit_iter: hb::BitIter<Bits>,
    containers: Containers,
    get: Get,
}
impl<Containers, Item, Bits, Get> Iterator for BuiltIter<Containers, Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(IdType, Containers) -> Item,
{
    type Item = Item;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.bit_iter.next()? as IdType;
        // might fuck this whole thing up
        // Some((self.get)(id, self.containers))
        None
    }
}

// sketch: macro magic

const ERROR_MSG: &'static str = "Bug in a container iterator!";

pub struct Iter<'a, T: 'static> {
    bit_iter: hb::BitIter<&'a hb::BitSet>,
    container: &'a Container<T>,
}
impl<'a, T: 'static> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.bit_iter.next()? as IdType;
        Some(self.container.get(id).expect(ERROR_MSG))
    }
}

macro_rules! joined_iter {
    ($iter_name:ident, $containers_name:ident, $bitset_type: ty, $($item_type:ident),*) => {
        #[allow(non_snake_case)]
        struct $containers_name<'a, $($item_type: 'static),*> {
            $($item_type: &'a Container<$item_type>),*
        }
        pub struct $iter_name<'a, $($item_type: 'static),*> {
            bit_iter: hb::BitIter<$bitset_type>,
            containers: $containers_name<'a, $($item_type),*>,
        }
        #[allow(non_snake_case)]
        impl<'a, $($item_type: 'static),*> $iter_name<'a, $($item_type),*> {
            pub fn new($($item_type: &'a Container<$item_type>),*) -> Self {
                let bitset = hb::BitSetAll;
                $(let bitset = hb::BitSetAnd(bitset, &$item_type.users);)*
                $iter_name {
                    bit_iter: bitset.iter(),
                    containers: $containers_name {
                        $($item_type),*
                    },
                }
            }
        }
        impl<'a, $($item_type: 'static),*> Iterator for $iter_name<'a, $($item_type),*> {
            type Item = ($(&'a $item_type),*);
            fn next(&mut self) -> Option<Self::Item> {
                let id = self.bit_iter.next()? as IdType;
                Some((
                    $(self.containers.$item_type.get(id).expect(ERROR_MSG),)*
                ))
            }
        }
    }
}
joined_iter! {
    Iter2, C2,
    // doesn't seem possible to define the bitset type dynamically here, unfortunately
    hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAll,
        &'a hb::BitSet>, &'a hb::BitSet>,
    T1, T2
}
joined_iter! {
    Iter3, C3,
    hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAll,
        &'a hb::BitSet>, &'a hb::BitSet>, &'a hb::BitSet>,
    T1, T2, T3
}
joined_iter! {
    Iter4, C4,
    hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAnd<hb::BitSetAll,
        &'a hb::BitSet>, &'a hb::BitSet>, &'a hb::BitSet>, &'a hb::BitSet>,
    T1, T2, T3, T4
}
