//! A limitation of the trait-based approach for layer addressing
//! is that we need to implement the trait ourselves on starframe types.
//! This is that.

use super::Component;

macro_rules! impl_components {
    ($head:ty, $($tail:ty,)*) => {
        impl_components!{0, $head, $($tail,)*}
    };
    ($count:expr, $head:ty, $($tail:ty,)+) => {
        impl Component for $head {
            const LAYER_INDEX: usize = $count;
        }
        impl_components!{$count + 1, $($tail,)*}
    };
    ($count:expr, $last:ty,) => {
        impl Component for $last {
            const LAYER_INDEX: usize = $count;
        }
        pub const BUILTIN_LAYER_COUNT: usize = $count + 1;
    };
}

impl_components! {
    crate::math::Pose,
    // physics
    crate::physics::Collider,
    crate::physics::Body,
    crate::physics::Rope,
    // graphics
    crate::graphics::Shape,
}

// user-facing version of the above macro for users to add their own types to the graph

#[macro_export]
macro_rules! make_graph {
    ($head:ty, $($tail:ty,)*) => {
        {
            make_graph!{{$crate::graph::BUILTIN_LAYER_COUNT}, $head, $($tail,)*}
        }
    };
    ($count:expr, $head:ty, $($tail:ty,)+) => {
        impl $crate::graph::Component for $head {
            const LAYER_INDEX: usize = $count;
        }
        make_graph!{$count + 1, $($tail,)*}
    };
    ($count:expr, $last:ty,) => {
        impl $crate::graph::Component for $last {
            const LAYER_INDEX: usize = $count;
        }
        $crate::graph::Graph::new($count + 1)
    };
    // no custom types to add
    () => {
        $crate::graph::Graph::new($crate::graph::BUILTIN_LAYER_COUNT)
    }
}
