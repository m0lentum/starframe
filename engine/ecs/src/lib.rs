pub mod componentcontainer;
pub mod event;
pub mod space;
pub mod storage;
pub mod system;

extern crate anymap;
extern crate hibitset;

pub type IdType = usize;
pub use self::space::Space;
