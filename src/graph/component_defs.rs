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

/// Create a [`Graph`][super::Graph] with layers for the given custom types.
/// All Starframe types that are meant to be used as components will also receive layers.
///
/// DO NOT invoke this more than once. You will get either confusing compile errors
/// or broken graphs at runtime depending on arguments.
///
/// # Example
/// ```
/// # use starframe::graph::make_graph;
/// struct PlayerController;
/// struct Enemy;
/// struct ReallyCoolGameMechanic;
///
/// let graph = make_graph! {
///     PlayerController,
///     Enemy,
///     ReallyCoolGameMechanic,
/// };
/// ```
/// # Limitations
///
/// This macro implements the [`Component`][super::Component] trait on
/// your types to give them addresses in the graph at compile time.
/// Therefore, the orphan rule prevents you from adding types from other crates
/// to the graph directly. Instead, you need to define your own types containing
/// those foreign types.
///
/// This is also the reason why you can't invoke this macro more than once
/// — the graph structure is defined at compile time and multiple invocations would break it.
/// It is, however, possible (though not advised) to have multiple graphs with the same structure
/// like this:
/// ```
/// # use starframe::graph::{make_graph, Graph};
/// fn graph() -> Graph {
///     make_graph! {
///         // ...
///     }
/// }
/// let graph = graph();
/// ```
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
        $crate::graph::Graph::with_layer_count($count + 1)
    };
    // no custom types to add
    () => {
        $crate::graph::Graph::with_layer_count($crate::graph::BUILTIN_LAYER_COUNT)
    }
}
