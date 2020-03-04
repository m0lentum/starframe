use hibitset as hb;

pub type IdType = usize;

pub trait FeatureSet: 'static {
    fn init(capacity: IdType) -> Self;
    fn tick(&mut self, dt: f32);
}

// TODO: decide which file these should live in
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

    pub fn add_transform(&mut self, obj: &MasterObjectHandle, tr: Transform) {
        self.fragments.insert(obj, TransformFragment { tr });
    }
}

//

use crate::visuals_glium::Shape;
pub struct ShapeFragment {
    pub shape: Shape,
    tr: Transform,
}
impl ShapeFragment {
    pub fn new(shape: Shape) -> Self {
        ShapeFragment {
            shape,
            tr: Transform::default(),
        }
    }
}
pub struct ShapeFeature {
    fragments: Container<ShapeFragment>,
}
impl ShapeFeature {
    pub fn with_capacity(capacity: IdType) -> Self {
        ShapeFeature {
            fragments: Container::with_capacity(capacity),
        }
    }

    pub fn add_shape(&mut self, obj: &MasterObjectHandle, shape: Shape) {
        self.fragments.insert(obj, ShapeFragment::new(shape));
    }

    pub fn sync_transforms(&mut self, transforms: &TransformFeature) {
        let simple_iter = self
            .fragments
            .build_iter_simple()
            .and(&transforms.fragments)
            .build(|id| (self.fragments.get(id), transforms.fragments.get(id)));
        for (i, (shape_frag, tr_frag)) in simple_iter.enumerate() {
            println!("We got a shape and tr boyz {}", i);
        }
    }

    pub fn draw<S: glium::Surface, C: crate::visuals_glium::camera::CameraController>(
        &self,
        target: &mut S,
        camera: &crate::visuals_glium::camera::Camera2D<C>,
        shaders: &crate::visuals_glium::Shaders,
    ) {
        let view = camera.view_matrix();

        for frag in self.fragments.iter() {
            let model = frag.tr.0.into_homogeneous_matrix();
            let mv = view * model;
            let mv_uniform = [
                [mv.cols[0].x, mv.cols[0].y, mv.cols[0].z],
                [mv.cols[1].x, mv.cols[1].y, mv.cols[1].z],
                [mv.cols[2].x, mv.cols[2].y, mv.cols[2].z],
            ];

            use glium::uniform;
            let uniforms = glium::uniform! {
                model_view: mv_uniform,
                color: frag.shape.color,
            };
            target
                .draw(
                    &*frag.shape.verts,
                    glium::index::NoIndices(frag.shape.primitive_type),
                    &shaders.ortho_2d,
                    &uniforms,
                    &Default::default(),
                )
                .expect("Drawing failed");
        }
    }
}

//

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
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: hb::BitSet::with_capacity(capacity as u32),
            enabled_objects: hb::BitSet::with_capacity(capacity as u32),
            generations: vec![0; capacity],
            next_obj_id: 0,
            capacity,
            features: F::init(capacity),
        }
    }

    /// Create a new object in this Space. An object does not do anything on its own;
    /// use SpaceFeatures to add functionality to it.
    /// # Panics
    /// Panics if the Space is full.
    pub fn create_object(&mut self) -> MasterObjectHandle {
        self.try_create_object()
            .expect("Tried to add an object to a full space")
    }

    /// Like create_object, but returns None instead of panicking if the Space is full.
    pub fn try_create_object(&mut self) -> Option<MasterObjectHandle> {
        if self.next_obj_id < self.capacity {
            let id = self.next_obj_id;
            self.next_obj_id += 1;
            self.create_object_at(id);
            Some(MasterObjectHandle {
                id,
                gen: self.generations[id],
            })
        } else {
            // find a dead object
            use hb::BitSetLike;
            match (!&self.alive_objects).iter().nth(0) {
                Some(id) if id < self.capacity as u32 => {
                    self.create_object_at(id as IdType);
                    Some(MasterObjectHandle {
                        id: id as IdType,
                        gen: self.generations[id as usize],
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

    pub fn clear(&mut self) {
        self.alive_objects.clear();
        // self.pools.clear();
        for gen in &mut self.generations {
            *gen = 0;
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.features.tick(dt);
    }
}

pub struct MasterObjectHandle {
    pub(crate) id: IdType,
    pub(crate) gen: u8,
}

pub struct ObjectHandle {
    pub(crate) id: IdType,
    pub(crate) gen: u8,
}
