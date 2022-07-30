use super::{Graph, LayerView, LayerViewMut};

/// Trait allowing tuples of layers to be accessed from a [`Graph`][super::Graph]
/// in one call using [`Graph::get_layer_bundle`][super::Graph::get_layer_bundle].
pub trait LayerBundle<'a> {
    fn get_from_graph(graph: &'a Graph) -> Self;
}

impl<'a, T: 'static> LayerBundle<'a> for LayerView<'a, T> {
    fn get_from_graph(graph: &'a Graph) -> Self {
        graph.get_layer()
    }
}
impl<'a, T: 'static> LayerBundle<'a> for LayerViewMut<'a, T> {
    fn get_from_graph(graph: &'a Graph) -> Self {
        graph.get_layer_mut()
    }
}
macro_rules! impl_tuple {
    ($($member:ident),+) => {
        impl<'a, $($member),*> LayerBundle<'a> for ($($member),*)
        where
            $(
                $member: LayerBundle<'a>,
            )*
        {
            fn get_from_graph(graph: &'a Graph) -> Self {
                (
                    $(
                        <$member as LayerBundle>::get_from_graph(graph),
                    )*
                )
            }
        }
    }
}
impl_tuple!(T1, T2);
impl_tuple!(T1, T2, T3);
impl_tuple!(T1, T2, T3, T4);
impl_tuple!(T1, T2, T3, T4, T5);
impl_tuple!(T1, T2, T3, T4, T5, T6);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

// named layer bundle macro for more convenient API.
// this really should be a proc macro lol

/// Generate a named struct of layer views that implements
/// [`LayerBundle`][crate::graph::LayerBundle].
/// # Example
/// ```
/// # use starframe::{math::Pose, physics::Body, graph::named_layer_bundle};
/// named_layer_bundle!{
///     pub struct Layers<'a> { // `pub` and lifetime are required
///         pose: r Pose, // `r` for `read`, generates a `LayerView<Pose>` field
///         body: w Body, // `w` for `write`, generates a `LayerViewMut<Body>` field
///     }
/// }
/// ```
#[macro_export]
macro_rules! named_layer_bundle {
    (pub struct $name:ident<$lt:lifetime> { $($body:tt)* }) => {
        named_layer_bundle!{struct_content $name $lt {} $($body)*}
        named_layer_bundle!{bundle_impl $name graph {} $($body)*}
    };
    (
        struct_content $name:ident $lt:lifetime { $($done_body:tt)* }
        $field:ident: r $l_type:ty, $($tail:tt)*
    ) => {
        named_layer_bundle!{
            struct_content
            $name
            $lt
            {
                $($done_body)*
                pub $field: $crate::graph::LayerView<'a, $l_type>,
            }
            $($tail)*
        }
    };
    (
        struct_content $name:ident $lt:lifetime { $($done_body:tt)* }
        $field:ident: w $l_type:ty, $($tail:tt)*
    ) => {
        named_layer_bundle!{
            struct_content
            $name
            $lt
            {
                $($done_body)*
                pub $field: $crate::graph::LayerViewMut<$lt, $l_type>,
            }
            $($tail)*
        }
    };
    (struct_content $name:ident $lt:lifetime { $($body:tt)* }) => {
        pub struct $name<$lt> {
            $($body)*
        }
    };
    (bundle_impl $name:ident $graph_var:tt { $($done_body:tt)* } $field:ident: $rw:tt $l_type:ty, $($tail:tt)*) => {
        named_layer_bundle!{
            bundle_impl
            $name
            $graph_var
            {
                $($done_body)*
                $field: $graph_var.get_layer_bundle(),
            }
            $($tail)*
        }
    };
    (bundle_impl $name:ident $graph_var:tt { $($body:tt)* }) => {
        impl<'a> $crate::graph::LayerBundle<'a> for $name<'a> {
            fn get_from_graph($graph_var: &'a $crate::graph::Graph) -> Self {
                Self {
                    $($body)*
                }
            }
        }
    }
}
