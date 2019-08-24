pub mod space;
pub use space::{ObjectHandle, ObjectRecipe, Space};

pub mod componentcontainer;

pub mod storage;

pub mod event;

pub mod system;

//

pub use hibitset;

pub type IdType = usize;
