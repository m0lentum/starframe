//! Storage and iterators for handling components of game objects.
use hibitset as hb;

use super::space::MasterKey;
use super::storage::Storage;

const ITER_ERR_MSG: &'static str =
    "A component didn't exist where it should have. This is almost certainly an error in moleengine.";

/// Opaque information needed to create a Container.
/// These are handed out when initializing a `space::FeatureSet`.
#[derive(Clone, Copy)]
pub struct Init {
    pub(crate) capacity: usize,
}

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
    pub fn new(init: Init) -> Self {
        Container {
            users: hb::BitSet::with_capacity(init.capacity as u32),
            storage: S::with_capacity(init.capacity),
        }
    }

    /// Insert a component into the Container.
    ///
    /// This requires a MasterKey, which you can get from the containing `Space` by creating an object.
    /// See the `Space` documentation for usage examples.
    pub fn insert(&mut self, key: MasterKey, comp: S::Item) {
        self.users.add(key.id as u32);
        self.storage.insert(key.id, comp)
    }

    /// Create an IterFragment that can be turned into a concrete iterator
    /// by joining it with an `IterBuilder`.
    pub fn iter<'a>(
        &'a self,
    ) -> IterFragment<&'a S::Item, &'a hb::BitSet, impl FnMut(usize) -> &'a S::Item> {
        IterFragment {
            bits: &self.users,
            get: move |id| self.storage.get(id).expect(ITER_ERR_MSG),
        }
    }

    /// Create an IterFragment with mutable access to the components in this Container.
    pub fn iter_mut<'a>(
        &'a mut self,
    ) -> IterFragment<&'a mut S::Item, &'a hb::BitSet, impl FnMut(usize) -> &'a mut S::Item> {
        let storage = &mut self.storage;
        IterFragment {
            bits: &self.users,
            get: move |id| {
                // the bitset iterator won't return the same id twice
                // so we can safely alias mutable references here
                let storage_ptr: *mut _ = storage;
                let storage_ref = unsafe { storage_ptr.as_mut().unwrap() };
                storage_ref.get_mut(id).expect(ITER_ERR_MSG)
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
    Get: FnMut(usize) -> Item,
{
    pub(crate) bits: Bits,
    pub(crate) get: Get,
}

/// Information required to add a `Container`'s content to an `IterBuilder`.
/// See `IterBuilder` for a usage example.
pub struct IterFragment<Item, Bits, Get>
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
    /// Iterate only over objects that have the features of both this `IterBuilder`
    /// and the given `IterFragment`, producing pairs of both.
    ///
    /// ```
    /// for (foo, bar) in foo_iter().and(bar_container.iter()) {
    ///     // ...
    /// }
    /// ```
    pub fn and<OI, OB: hb::BitSetLike, OG: FnMut(usize) -> OI>(
        self,
        other: IterFragment<OI, OB, OG>,
    ) -> IterBuilder<(Item, OI), hb::BitSetAnd<Bits, OB>, impl FnMut(usize) -> (Item, OI)> {
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
    pub fn overlay<OI, OB: hb::BitSetLike, OG: FnMut(usize) -> OI>(
        self,
        other: IterFragment<OI, OB, OG>,
    ) -> IterBuilder<OI, hb::BitSetAnd<Bits, OB>, impl FnMut(usize) -> OI> {
        IterBuilder {
            bits: hb::BitSetAnd(self.bits, other.bits),
            get: other.get,
        }
    }

    /// Filter out objects which have the component iterated by the given `IterFragment`.
    pub fn not<OI, OB: hb::BitSetLike, OG: FnMut(usize) -> OI>(
        self,
        other: IterFragment<OI, OB, OG>,
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

/// A concrete iterator produced by `IterBuilder::into_iterator`.
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
