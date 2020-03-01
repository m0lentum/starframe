use hibitset as hb;

pub type IdType = usize;

pub trait FeatureSet: 'static {
    fn init(capacity: IdType) -> Self;
    fn tick(dt: f32);
}

// TODO: decide which file this should live in
use super::container::Container;
use crate::util::Transform;
pub struct TransformFragment {
    pub tr: Transform,
}
pub struct TransformFeature {
    fragments: Container<TransformFragment>,
}

impl TransformFeature {
    pub fn with_capacity(capacity: IdType) -> Self {
        TransformFeature {
            fragments: Container::with_capacity(capacity),
        }
    }
}

pub struct Space<F: FeatureSet> {
    alive_objects: hb::BitSet,
    enabled_objects: hb::BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    pub features: F,
    // TODO: pools
}

impl<F: FeatureSet> Space<F> {
    /// Create a Space with a given maximum capacity.
    pub fn new(capacity: IdType, features: F) -> Self {
        Space {
            alive_objects: hb::BitSet::with_capacity(capacity as u32),
            enabled_objects: hb::BitSet::with_capacity(capacity as u32),
            generations: vec![0; capacity],
            next_obj_id: 0,
            capacity,
            features,
        }
    }

    /// Create a new object in this Space. An object does not do anything on its own;
    /// use SpaceFeatures to add functionality to it.
    /// # Panics
    /// Panics if the Space is full.
    pub fn create_object(&mut self) -> MasterObjectHandle<'_, F> {
        self.try_create_object()
            .expect("Tried to add an object to a full space")
    }

    /// Like create_object, but returns None instead of panicking if the Space is full.
    pub fn try_create_object(&mut self) -> Option<MasterObjectHandle<'_, F>> {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id);
            Some(MasterObjectHandle { id, space: self })
        } else {
            // find a dead object
            use hb::BitSetLike;
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    self.create_object_at(id as IdType);
                    Some(MasterObjectHandle {
                        id: id as IdType,
                        space: self,
                    })
                }
                _ => None,
            }
        }
    }

    fn create_object_at(&mut self, id: IdType) {
        self.alive_objects.add(id as u32);
        self.enabled_objects.add(id as u32);
        self.generations[id] += 1;
    }
}

pub struct MasterObjectHandle<'a, F: FeatureSet> {
    id: IdType,
    space: &'a mut Space<F>,
}

pub struct ObjectHandle {
    id: IdType,
}
