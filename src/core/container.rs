//! Storage and iterators for handling components of game objects.
//!
//! Heavily inspired by [`specs`](https://github.com/amethyst/specs).
use hibitset as hb;

use super::space::{CreationId, Id};
use super::storage::Storage;

const ITER_ERR_MSG: &'static str =
    "A component didn't exist where it should have. This is almost certainly an error in starframe.";

/// A container for a specific type of component,
/// typically belonging to a feature of a Space.
///
/// There are several `Storage` types to choose from (see the `core::storage` module);
/// which one is optimal depends mostly on how common the component type is.
/// Usually `DenseVecStorage` perfoms the best.
pub struct Container<S: Storage> {
    users: hb::BitSet,
    storage: S,
}

impl<S: Storage> Container<S> {
    /// Create a new Container from an Init struct.
    pub fn new(init: super::space::FeatureSetInit) -> Self {
        Container {
            users: hb::BitSet::with_capacity(init.capacity as u32),
            storage: S::new(init),
        }
    }

    /// Insert a component into the Container.
    ///
    /// This requires a `CreationId`, which you can get from the containing `Space` by creating an object.
    /// See the `Space` documentation for usage examples.
    pub fn insert(&mut self, id: CreationId, comp: S::Item) {
        self.users.add(id.0 as u32);
        self.storage.insert(id.0, comp)
    }

    /// Get an immutable reference to a component of a specific object, if it has it.
    pub fn get(&self, id: impl Into<Id>) -> Option<&S::Item> {
        let id = id.into().0;
        if self.users.contains(id as u32) {
            // TODO: storage should not return Nones
            Some(self.storage.get(id).unwrap())
        } else {
            None
        }
    }

    /// Get a mutable reference to a component of a specific object, if it has it.
    pub fn get_mut(&mut self, id: impl Into<Id>) -> Option<&mut S::Item> {
        let id = id.into().0;
        if self.users.contains(id as u32) {
            // TODO: storage should not return Nones
            Some(self.storage.get_mut(id).unwrap())
        } else {
            None
        }
    }

    /// Returns whether or not the given object has the component stored in this Container.
    pub fn has(&self, id: impl Into<Id>) -> bool {
        self.users.contains(id.into().0 as u32)
    }

    /// Create an IterFragment that can be turned into a concrete iterator
    /// by joining it with an `IterBuilder`.
    pub fn iter<'a>(&'a self) -> IterFragment<&'a S::Item, impl FnMut(Id) -> &'a S::Item> {
        IterFragment {
            bits: &self.users,
            get: move |id| self.storage.get(id.0).expect(ITER_ERR_MSG),
        }
    }

    /// Create an IterFragment with mutable access to the components in this Container.
    pub fn iter_mut<'a>(
        &'a mut self,
    ) -> IterFragment<&'a mut S::Item, impl FnMut(Id) -> &'a mut S::Item> {
        let storage = &mut self.storage;
        IterFragment {
            bits: &self.users,
            get: move |id| {
                // the bitset iterator won't return the same id twice
                // so we can safely alias mutable references here
                let storage_ptr: *mut _ = storage;
                let storage_ref = unsafe { storage_ptr.as_mut().unwrap() };
                storage_ref.get_mut(id.0).expect(ITER_ERR_MSG)
            },
        }
    }
}

/// A builder to generate iterators over the set of objects that contains specific components.
///
/// The only way to get one of these is by starting with a `SpaceAccess` / `SpaceAccessMut`.
/// This ensures that only objects that are actually alive in the Space are included in the resulting iterator.
/// You can then combine it with `IterFragment`s from `Container`s to access the components you want.
///
/// TODOC: example
pub struct IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(Id) -> Item,
{
    pub(crate) bits: Bits,
    pub(crate) get: Get,
}

/// Information required to add a `Container`'s content to an `IterBuilder`.
/// See `IterBuilder` for a usage example.
pub struct IterFragment<'a, Item, Get>
where
    Get: FnMut(Id) -> Item,
{
    bits: &'a hb::BitSet,
    get: Get,
}

impl<Item, Bits, Get> IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(Id) -> Item,
{
    /// Iterate only over objects that have the features of both this `IterBuilder`
    /// and the given `IterFragment`, producing pairs of both.
    ///
    /// ```
    /// for (foo, bar) in foo_iter().and(bar_container.iter()) {
    ///     // ...
    /// }
    /// ```
    pub fn and<'a, OI, OG: FnMut(Id) -> OI>(
        self,
        other: IterFragment<'a, OI, OG>,
    ) -> IterBuilder<(Item, OI), hb::BitSetAnd<Bits, &'a hb::BitSet>, impl FnMut(Id) -> (Item, OI)>
    {
        let mut gets = (self.get, other.get);
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, other.bits),
            get: move |id| ((gets.0)(id), (gets.1)(id)),
        }
    }

    /// Filter objects like in `and`, but discard the item type of the current `IterBuilder`
    /// from the resulting iterator.
    ///
    /// Typically used to discard the unit types produced by the initial `IterBuilder` you get from a `SpaceAccess`.
    ///
    /// ```
    /// for bar in space_access.iter().overlay(bar_container.iter()) {
    ///     // ...
    /// }
    /// ```
    pub fn overlay<'a, OI, OG: FnMut(Id) -> OI>(
        self,
        other: IterFragment<'a, OI, OG>,
    ) -> IterBuilder<OI, hb::BitSetAnd<Bits, &'a hb::BitSet>, impl FnMut(Id) -> OI> {
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, other.bits),
            get: other.get,
        }
    }

    /// Filter out objects which have the component iterated by the given `IterFragment`.
    pub fn not<'a, OI, OG: FnMut(Id) -> OI>(
        self,
        other: IterFragment<'a, OI, OG>,
    ) -> IterBuilder<Item, hb::BitSetAnd<Bits, hb::BitSetNot<&'a hb::BitSet>>, impl FnMut(Id) -> Item>
    {
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, hb::BitSetNot(other.bits)),
            get: self.get,
        }
    }

    /// Also bundle the id of the object in question with the components.
    pub fn with_ids(self) -> IterBuilder<(Item, Id), Bits, impl FnMut(Id) -> (Item, Id)> {
        let mut get = self.get;
        IterBuilder {
            bits: self.bits,
            get: move |id| (get(id), id),
        }
    }

    /// Convenience method to collect the resulting iterator into a Vec.
    /// Useful when you need to iterate multiple times.
    pub fn collect_to_vec(self) -> Vec<Item> {
        self.into_iter().collect()
    }
}
impl<Item, Bits, Get> IntoIterator for IterBuilder<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(Id) -> Item,
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

/// A concrete iterator produced by `IterBuilder::into_iterator`.
pub struct Iter<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(Id) -> Item,
{
    bit_iter: hb::BitIter<Bits>,
    get: Get,
}
impl<Item, Bits, Get> Iterator for Iter<Item, Bits, Get>
where
    Bits: hb::BitSetLike,
    Get: FnMut(Id) -> Item,
{
    type Item = Item;
    fn next(&mut self) -> Option<Self::Item> {
        let id = Id(self.bit_iter.next()? as usize);
        Some((self.get)(id))
    }
}
