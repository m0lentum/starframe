mod spatialindex;
pub(crate) use spatialindex::SpatialIndex;

mod collider;
pub use collider::{Collider, ColliderShape};

pub mod shape_shape;
pub use shape_shape::{Contact, ContactIterator, ContactResult};

pub mod query;
