mod spatialindex;
pub(crate) use spatialindex::SpatialIndex;

mod collider;
pub use collider::{Collider, ColliderShape, ColliderType, Material};

pub mod shape_shape;
pub use shape_shape::{Contact, ContactIterator, ContactResult};

pub mod query;
