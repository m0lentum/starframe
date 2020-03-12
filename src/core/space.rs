use anymap::AnyMap;
use hibitset::{self as hb, BitSetLike};

use super::{container as cont, Recipe};

pub trait FeatureSet: 'static + Sized {
    fn init(container_init: cont::Init) -> Self;
    fn containers(&mut self) -> cont::DynRefs;
    fn create_pools(mut _pool_access: PoolCreateAccess<Self>) {}
    fn tick(&mut self, dt: f32);
    fn render(&self);
}

//

/// A handle to an object that can be used to add new components to it.
#[derive(Clone, Copy)]
pub struct MasterKey {
    pub(crate) id: usize,
}

pub struct Space<F: FeatureSet> {
    reserved_ids: hb::BitSet,
    enabled_ids: hb::BitSet,
    next_obj_id: usize,
    capacity: usize,
    pub features: F,
    pools: AnyMap,
}

impl<F: FeatureSet> Space<F> {
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

    /// Create an object in this Space and add some Features to it.
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
