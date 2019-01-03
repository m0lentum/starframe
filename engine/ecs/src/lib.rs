pub mod componentcontainer;
pub mod space;
pub mod storage;
pub mod system;

pub type IdType = usize;
pub use self::componentcontainer::{ComponentContainer, ReadAccess, WriteAccess};
pub use self::space::Space;
