use ecs::storage::ComponentStorage;
use ecs::{ComponentContainer, ReadAccess, WriteAccess};

use hibitset::{BitIter, BitSet, BitSetAnd, BitSetLike};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use tuple_utils::Split;

use std::marker::PhantomData;

pub type IdType = usize;

pub struct Space {
    alive_objects: BitSet,
    next_obj_id: IdType,
    capacity: IdType,
    components: HashMap<TypeId, Box<Any>>,
}

impl Space {
    pub fn new(capacity: IdType) -> Self {
        Space {
            alive_objects: BitSet::with_capacity(capacity as u32),
            next_obj_id: 0,
            capacity: capacity,
            components: HashMap::new(),
        }
    }

    /// Reserves an object id for use and marks it as alive.
    /// If the space is full, returns None.
    pub fn create_object(&mut self) -> Option<IdType> {
        if self.next_obj_id >= self.capacity {
            return None;
            // TODO: Find a dead object and replace it if one exists
        }

        let id = self.next_obj_id;
        self.alive_objects.add(id as u32);

        self.next_obj_id += 1;

        Some(id)
    }

    /// Mark an object as dead. Does not actually destroy it, but
    /// none of its components will receive updates anymore and they
    /// can be replaced by something new.
    pub fn destroy_object(&mut self, id: IdType) {
        self.alive_objects.remove(id as u32);
    }

    /// Create a component for an object. The component can be of any type,
    /// but there has to be a ComponentContainer for it in this Space.
    ///
    /// # Panics
    ///
    /// Panics if there is no ComponentContainer for this type in this Space.
    pub fn create_component<T: 'static>(&mut self, id: IdType, comp: T) {
        let container = Self::get_container_mut::<T>(&mut self.components);
        container.insert(id, comp);
    }

    /// Create a ComponentContainer for a component type (any type)
    /// with a storage type. The storage should be initialized as empty and
    /// given to the Space to finish setting it up.
    ///
    /// See moleengine::ecs::storage for details about available storage types.
    pub fn create_container<T, S>(&mut self, storage: S)
    where
        T: 'static,
        S: 'static + ComponentStorage<T>,
    {
        self.components.insert(
            TypeId::of::<T>(),
            Box::new(ComponentContainer::new(Box::new(storage), self.capacity)),
        );
    }

    /// Get access to a single ComponentContainer.
    pub fn open<T: 'static>(&self) -> &ComponentContainer<T> {
        Self::get_container::<T>(&self.components)
    }

    pub fn iter<'a, R, W>(
        &'a self,
        reads: R,
        writes: W,
    ) -> ComponentIterator<
        'a,
        BitSetAnd<&BitSet, BitSetAnd<<R as BitAnd>::Value, <W as BitAnd>::Value>>,
        R::ReadType,
        W::WriteType,
    >
    where
        R: ContainerTuple<'a>,
        W: ContainerTuple<'a>,
    {
        ComponentIterator {
            iter: BitSetAnd(&self.alive_objects, BitSetAnd(reads.and(), writes.and())).iter(),
            reads: reads.read_access(),
            writes: writes.write_access(),
            _l: PhantomData,
        }
    }

    /// Used internally to get a type-safe reference to a container.
    /// Panics if the container has not been created.
    fn get_container<T: 'static>(components: &HashMap<TypeId, Box<Any>>) -> &ComponentContainer<T> {
        let raw = components.get(&TypeId::of::<T>()).unwrap();
        raw.downcast_ref::<ComponentContainer<T>>().unwrap()
    }

    /// Used internally to get a type-safe mutable reference to a container.
    /// Panics if the container has not been created.
    fn get_container_mut<T: 'static>(
        components: &mut HashMap<TypeId, Box<Any>>,
    ) -> &mut ComponentContainer<T> {
        let raw = components.get_mut(&TypeId::of::<T>()).unwrap();
        raw.downcast_mut::<ComponentContainer<T>>().unwrap()
    }
}

pub struct ComponentIterator<'a, B, R, W>
where
    B: BitSetLike,
    R: ReadTuple<'a>,
    W: WriteTuple<'a>,
{
    // the bitset iterator goes through the intersection
    // of all components' users plus the space's
    iter: BitIter<B>,
    reads: R,
    writes: W,
    _l: PhantomData<&'a ()>, // without this the compiler complains about 'a
}

impl<'a, B, R, W> ComponentIterator<'a, B, R, W>
where
    B: BitSetLike,
    R: ReadTuple<'a>,
    W: WriteTuple<'a>,
{
    pub fn next(&'a mut self) -> Option<(R::ItemType, W::ItemType)> {
        if let Some(id) = self.iter.next() {
            Some((
                self.reads.get(id as IdType),
                self.writes.get_mut(id as IdType),
            ))
        } else {
            None
        }
    }
}

pub trait ContainerTuple<'a>: Sized + BitAnd {
    type ReadType: ReadTuple<'a>;
    type WriteType: WriteTuple<'a>;
    type MaskType: BitSetLike;

    fn read_access(&self) -> Self::ReadType;
    fn write_access(&self) -> Self::WriteType;
}

/// This is what the following macro translates to for a pair.
/// Leaving this here for easier reading in the future.
impl<'a, A, B> ContainerTuple<'a> for (&'a ComponentContainer<A>, &'a ComponentContainer<B>) {
    type ReadType = (ReadAccess<'a, A>, ReadAccess<'a, B>);
    type WriteType = (WriteAccess<'a, A>, WriteAccess<'a, B>);
    type MaskType = <(&'a ComponentContainer<A>, &'a ComponentContainer<B>) as BitAnd>::Value;

    fn read_access(&self) -> Self::ReadType {
        let (a, b) = *self;
        (a.read(), b.read())
    }

    fn write_access(&self) -> Self::WriteType {
        let (a, b) = *self;
        (a.write(), b.write())
    }
}

macro_rules! impl_container {
    ( $( $type: ident ),* ) => {
        impl<'a, $($type),*> ContainerTuple<'a> for ($(&'a ComponentContainer<$type>),*,) {
            type ReadType = ($(ReadAccess<'a, $type>),*,);
            type WriteType = ($(WriteAccess<'a, $type>),*,);
            type MaskType = <($(&'a ComponentContainer<$type>),*,) as BitAnd>::Value;

            #[allow(non_snake_case)]
            fn read_access(&self) -> Self::ReadType {
                let ($($type,)*) = *self;
                ($($type.read(),)*)
            }

            #[allow(non_snake_case)]
            fn write_access(&self) -> Self::WriteType {
                let ($($type,)*) = *self;
                ($($type.write(),)*)
            }
        }
    }
}

impl_container!{A}
impl_container!{A, B, C}
impl_container!{A, B, C, D}
impl_container!{A, B, C, D, E}
impl_container!{A, B, C, D, E, F}
impl_container!{A, B, C, D, E, F, G}
impl_container!{A, B, C, D, E, F, G, H}
impl_container!{A, B, C, D, E, F, G, H, I}
impl_container!{A, B, C, D, E, F, G, H, I, J}
impl_container!{A, B, C, D, E, F, G, H, I, J, K}
impl_container!{A, B, C, D, E, F, G, H, I, J, K, L}
impl_container!{A, B, C, D, E, F, G, H, I, J, K, L, M}
impl_container!{A, B, C, D, E, F, G, H, I, J, K, L, M, N}
impl_container!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O}
impl_container!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P}

pub trait ReadTuple<'a> {
    type ItemType;

    fn get(&'a self, id: IdType) -> Self::ItemType;
}

// pair case in plain rust for readability
impl<'a, A, B> ReadTuple<'a> for (ReadAccess<'a, A>, ReadAccess<'a, B>) {
    type ItemType = (&'a A, &'a B);

    fn get(&'a self, id: IdType) -> Self::ItemType {
        let (a, b) = self;
        unsafe { (a.get(id), b.get(id)) }
    }
}

macro_rules! impl_read {
    ( $( $type: ident ),* ) => {
        impl<'a, $($type),*> ReadTuple<'a> for ($(ReadAccess<'a, $type>),*,) {
            type ItemType = ($(&'a $type),*,);

            #[allow(non_snake_case)]
            fn get(&'a self, id: IdType) -> Self::ItemType {
                let ($($type,)*) = self;
                unsafe {
                    ($($type.get(id),)*)
                }
            }
        }
    }
}

impl_read!{A}
impl_read!{A, B, C}
impl_read!{A, B, C, D}
impl_read!{A, B, C, D, E}
impl_read!{A, B, C, D, E, F}
impl_read!{A, B, C, D, E, F, G}
impl_read!{A, B, C, D, E, F, G, H}
impl_read!{A, B, C, D, E, F, G, H, I}
impl_read!{A, B, C, D, E, F, G, H, I, J}
impl_read!{A, B, C, D, E, F, G, H, I, J, K}
impl_read!{A, B, C, D, E, F, G, H, I, J, K, L}
impl_read!{A, B, C, D, E, F, G, H, I, J, K, L, M}
impl_read!{A, B, C, D, E, F, G, H, I, J, K, L, M, N}
impl_read!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O}
impl_read!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P}

pub trait WriteTuple<'a> {
    type ItemType;

    fn get_mut(&'a mut self, id: IdType) -> Self::ItemType;
}

// pair case in plain rust for readability
impl<'a, A, B> WriteTuple<'a> for (WriteAccess<'a, A>, WriteAccess<'a, B>) {
    type ItemType = (&'a mut A, &'a mut B);

    fn get_mut(&'a mut self, id: IdType) -> Self::ItemType {
        let (a, b) = self;
        unsafe { (a.get_mut(id), b.get_mut(id)) }
    }
}

macro_rules! impl_write {
    ( $( $type: ident ),* ) => {
        impl<'a, $($type),*> WriteTuple<'a> for ($(WriteAccess<'a, $type>),*,) {
            type ItemType = ($(&'a mut $type),*,);

            #[allow(non_snake_case)]
            fn get_mut(&'a mut self, id: IdType) -> Self::ItemType {
                let ($($type,)*) = self;
                unsafe {
                    ($($type.get_mut(id),)*)
                }
            }
        }
    }
}

impl_write!{A}
impl_write!{A, B, C}
impl_write!{A, B, C, D}
impl_write!{A, B, C, D, E}
impl_write!{A, B, C, D, E, F}
impl_write!{A, B, C, D, E, F, G}
impl_write!{A, B, C, D, E, F, G, H}
impl_write!{A, B, C, D, E, F, G, H, I}
impl_write!{A, B, C, D, E, F, G, H, I, J}
impl_write!{A, B, C, D, E, F, G, H, I, J, K}
impl_write!{A, B, C, D, E, F, G, H, I, J, K, L}
impl_write!{A, B, C, D, E, F, G, H, I, J, K, L, M}
impl_write!{A, B, C, D, E, F, G, H, I, J, K, L, M, N}
impl_write!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O}
impl_write!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P}

// BitAnd adapted from Specs: https://github.com/slide-rs/specs/blob/master/src/join.rs
//
// ------------------------------------------------------------------------------------------------------

/// `BitAnd` is a helper method to & bitsets together resulting in a tree.
pub trait BitAnd {
    /// The combined bitsets.
    type Value: BitSetLike;
    /// Combines `Self` into a single `BitSetLike` through `BitSetAnd`.
    fn and(&self) -> Self::Value;
}

/// This needs to be special cased
impl<'a, A> BitAnd for (&'a ComponentContainer<A>,) {
    type Value = &'a BitSet;
    fn and(&self) -> Self::Value {
        self.0.get_users()
    }
}

macro_rules! bitset_and {
    // use variables to indicate the arity of the tuple
    ($($from:ident),*) => {
        impl<'a, $($from),*> BitAnd for ($(&'a ComponentContainer<$from>),*)
        {
            type Value = BitSetAnd<
                <<Self as Split>::Left as BitAnd>::Value,
                <<Self as Split>::Right as BitAnd>::Value
            >;

            fn and(&self) -> Self::Value {
                let (l, r) = self.split();
                BitSetAnd(l.and(), r.and())
            }
        }
    }
}

bitset_and!{A, B}
bitset_and!{A, B, C}
bitset_and!{A, B, C, D}
bitset_and!{A, B, C, D, E}
bitset_and!{A, B, C, D, E, F}
bitset_and!{A, B, C, D, E, F, G}
bitset_and!{A, B, C, D, E, F, G, H}
bitset_and!{A, B, C, D, E, F, G, H, I}
bitset_and!{A, B, C, D, E, F, G, H, I, J}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K, L}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K, L, M}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K, L, M, N}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O}
bitset_and!{A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P}

// ------------------------------------------------------------------------------------------------------
