//! A Space is an environment that manages game objects,
//! and the only way you can create objects at all in MoleEngine.
//! It implements a variant of the Entity-Component-System pattern.
//!
//! Component storage and Systems are bundled into Features,
//! which are implemented as a user-defined freeform struct
//! that implements `FeatureSet` and parameterizes the Space.
//! This way behaviours a Space supports and dependencies between them
//! are defined at compile time and can be borrow-checked.
//!
//! TODOC: pools, containers, code example

use anymap::AnyMap;
use hibitset::{self as hb, BitSetLike};

use super::{container as cont, Recipe};

/// Trait describing Features of a Space.
/// These determine which component types and behaviors are available in the Space.
/// See the module-level documentation for a full usage example.
///
/// TODOC: containers, init, tick & render
pub trait FeatureSet: 'static + Sized {
    fn init(container_init: cont::Init) -> Self;
    fn containers(&mut self) -> cont::DynRefs;
    fn create_pools(mut _pool_access: PoolCreateAccess<Self>) {}
    fn tick(&mut self, dt: f32);
    fn render(&self);
}

//

/// A handle to an object that can be used to add new components to it.
/// Only given out during object creation.
#[derive(Clone, Copy)]
pub struct MasterKey {
    pub(crate) id: usize,
}

/// An environment where game objects live.
/// The Space handles reserving and giving out IDs for objects,
/// while all Components are stored and handled inside of Features.
/// See the module-level documentation for a full usage example.
pub struct Space<F: FeatureSet> {
    reserved_ids: hb::BitSet,
    enabled_ids: hb::BitSet,
    next_obj_id: usize,
    capacity: usize,
    pub features: F,
    pools: AnyMap,
}

impl<F: FeatureSet> Space<F> {
    /// Create a Space with a a given maximum capacity.
    /// Currently this capacity is a hard limit; Spaces do not grow.
    /// The FeatureSet's `init` and `create_pools` functions are called here.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut space = Space {
            reserved_ids: hb::BitSet::with_capacity(capacity as u32),
            enabled_ids: hb::BitSet::with_capacity(capacity as u32),
            next_obj_id: 0,
            capacity,
            features: F::init(cont::Init { capacity }),
            pools: AnyMap::new(),
        };
        F::create_pools(PoolCreateAccess {
            pools: &mut space.pools,
            reserved_ids: &mut space.reserved_ids,
            features: &mut space.features,
        });
        // find first index after what pools reserved and start accepting new objects from there
        //
        // negation of the bitset can't not have a first item so we unwrap here.
        // creating a pool will cause a panic if there's not enough room
        // this is a little ugly implementation-wise but a panic is probably always the desirable behavior here
        space.next_obj_id = (!&space.reserved_ids).iter().nth(0).unwrap() as usize;
        space
    }

    /// Create an 'ad-hoc' object in this Space, that is, one that isn't based on a Recipe.
    /// Returns `Some(())` if successful, `None` if there's no room left in the Space.
    pub fn create_object(&mut self, f: impl FnOnce(MasterKey, &mut F)) -> Option<()> {
        let key = self.do_create_object()?;
        f(key, &mut self.features);
        Some(())
    }

    fn do_create_object(&mut self) -> Option<MasterKey> {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id);
            Some(MasterKey { id })
        } else {
            // find a dead object
            match (!&self.reserved_ids).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    self.create_object_at(id as usize);
                    Some(MasterKey { id: id as usize })
                }
                _ => None,
            }
        }
    }

    fn create_object_at(&mut self, id: usize) {
        self.reserved_ids.add(id as u32);
        self.enabled_ids.add(id as u32);
    }

    /// Instantiate a Recipe in this Space.
    /// If a Pool exists for that Recipe, uses the Pool, otherwise reserves a new object.
    /// Returns `Some(())` if successful, `None` if there's no room in the Pool or Space.
    pub fn spawn<R: super::Recipe<F>>(&mut self, recipe: R) -> Option<()> {
        if let Some(pool) = self.pools.get_mut::<Pool<F, R>>() {
            pool.spawn(recipe, &mut self.enabled_ids, &mut self.features)
        } else {
            self.create_object(|a, feat| {
                R::spawn_consts(a, feat);
                recipe.spawn_vars(a, feat);
            })
        }
    }

    /// Spawn objects described in a RON file into this Space.
    /// Can fail if either parsing the data fails or all objecs don't fit in the Space.
    pub fn read_ron_file<R>(&mut self, file: std::fs::File) -> Result<(), ron::de::Error>
    where
        R: super::recipe::DeserializeRecipes<F>,
    {
        let mut reader = std::io::BufReader::new(file);
        let mut bytes = Vec::new();
        use std::io::Read;
        reader.read_to_end(&mut bytes)?;

        let mut deser = ron::de::Deserializer::from_bytes(bytes.as_slice())?;
        R::deserialize_into_space(&mut deser, self)
    }

    pub fn tick(&mut self, dt: f32) {
        self.features.tick(dt);
    }

    pub fn render(&self) {
        self.features.render();
    }
}

// Pools

/// Information required to create Pools in a Space.
pub struct PoolCreateAccess<'a, F: FeatureSet> {
    pools: &'a mut AnyMap,
    reserved_ids: &'a mut hb::BitSet,
    features: &'a mut F,
}

impl<'a, F: FeatureSet> PoolCreateAccess<'a, F> {
    pub fn create<R: Recipe<F> + 'static>(&mut self, size: usize) {
        let pool: Pool<F, R> = Pool::init(size, self.reserved_ids, self.features);
        self.pools.insert(pool);
    }
}

struct Pool<F: FeatureSet, R: Recipe<F>> {
    reserved_slots: hb::BitSet,
    _marker: std::marker::PhantomData<(F, R)>,
}

impl<F: FeatureSet, R: Recipe<F>> Pool<F, R> {
    pub(self) fn init(size: usize, reserved_ids: &mut hb::BitSet, features: &mut F) -> Self {
        let mut slots = hb::BitSet::new();
        for vacant_id in (!&*reserved_ids).iter().take(size) {
            slots.add(vacant_id);
        }
        for taken_id in &slots {
            reserved_ids.add(taken_id);
        }

        for slot in &slots {
            R::spawn_consts(MasterKey { id: slot as usize }, features);
        }
        Pool {
            reserved_slots: slots,
            _marker: std::marker::PhantomData,
        }
    }

    pub(self) fn spawn(
        &mut self,
        recipe: R,
        enabled_ids: &mut hb::BitSet,
        features: &mut F,
    ) -> Option<()> {
        let available_ids = hb::BitSetAnd(&self.reserved_slots, !&*enabled_ids);
        let my_id = available_ids.iter().nth(0)?;
        enabled_ids.add(my_id);
        recipe.spawn_vars(MasterKey { id: my_id as usize }, features);
        Some(())
    }
}
