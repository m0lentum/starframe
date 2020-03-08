use hibitset as hb;

pub type IdType = usize;

use super::container::{Container, ContainerAccess};
pub trait FeatureSet: 'static {
    fn init(capacity: IdType) -> Self;
    fn containers(&mut self) -> Vec<&mut dyn ContainerAccess>;
    fn tick(&mut self, dt: f32);
}

// TODO: decide which file these should live in
use crate::util::Transform;
pub type TransformFeature = Container<Transform>;
use crate::visuals_glium::Shape;
pub type ShapeFeature = Container<Shape>;
impl ShapeFeature {
    pub fn draw<S: glium::Surface, C: crate::visuals_glium::camera::CameraController>(
        &self,
        trs: &TransformFeature,
        target: &mut S,
        camera: &crate::visuals_glium::camera::Camera2D<C>,
        shaders: &crate::visuals_glium::Shaders,
    ) {
        let view = camera.view_matrix();

        for (shape, tr) in self.iter().and(trs.iter()) {
            let model = tr.0.into_homogeneous_matrix();
            let mv = view * model;
            let mv_uniform = [
                [mv.cols[0].x, mv.cols[0].y, mv.cols[0].z],
                [mv.cols[1].x, mv.cols[1].y, mv.cols[1].z],
                [mv.cols[2].x, mv.cols[2].y, mv.cols[2].z],
            ];

            use glium::uniform;
            let uniforms = glium::uniform! {
                model_view: mv_uniform,
                color: shape.color,
            };
            target
                .draw(
                    &*shape.verts,
                    glium::index::NoIndices(shape.primitive_type),
                    &shaders.ortho_2d,
                    &uniforms,
                    &Default::default(),
                )
                .expect("Drawing failed");
        }

        for (i, _shape) in self.iter().not(trs.iter()).into_iter().enumerate() {
            println!("There was {} thing with shape but no tr", i);
        }
    }
}

//

pub struct Space<F: FeatureSet> {
    alive_objects: hb::BitSet,
    enabled_objects: hb::BitSet,
    next_obj_id: IdType,
    capacity: IdType,
    pub features: F,
}

pub struct SpaceInternals {
    // TODO: pools
}

impl<F: FeatureSet> Space<F> {
    pub fn with_capacity(capacity: IdType) -> Self {
        Space {
            alive_objects: hb::BitSet::with_capacity(capacity as u32),
            enabled_objects: hb::BitSet::with_capacity(capacity as u32),
            next_obj_id: 0,
            capacity,
            features: F::init(capacity),
        }
    }

    /// Create a new object in this Space. An object does not do anything on its own;
    /// use SpaceFeatures to add functionality to it.
    /// Returns None if the Space is full.
    pub fn create_object(&mut self) -> Option<MasterObjectHandle<F>> {
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

    /// Create an object and immediately add some Features to it.
    pub fn create_object_with(
        &mut self,
        add_fn: impl FnOnce(IdType, &mut F),
    ) -> Option<MasterObjectHandle<F>> {
        let mut handle = self.create_object()?;
        handle.add_features(add_fn);
        Some(handle)
    }

    fn create_object_at(&mut self, id: IdType) {
        self.alive_objects.add(id as u32);
        self.enabled_objects.add(id as u32);
    }

    fn kill_object(&mut self, id: IdType) {
        let id = id as u32;
        self.alive_objects.remove(id);
        for container in self.features.containers() {
            container.users().remove(id);
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.features.tick(dt);
    }
}

pub struct MasterObjectHandle<'a, F: FeatureSet> {
    id: IdType,
    space: &'a mut Space<F>,
}

impl<'a, F: FeatureSet> MasterObjectHandle<'a, F> {
    pub fn add_features(&mut self, add_fn: impl FnOnce(IdType, &mut F)) {
        add_fn(self.id, &mut self.space.features);
    }

    pub fn kill(&mut self) {
        self.space.kill_object(self.id);
    }
}
