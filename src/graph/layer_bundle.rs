use super::{Component, Graph, LayerView, LayerViewMut};

/// Trait allowing tuples of layer views to be accessed from a [`Graph`][super::Graph]
/// in one call using [`Graph::get_layer_bundle`][super::Graph::get_layer_bundle].
pub trait LayerBundle<'a> {
    fn get_from_graph(graph: &'a Graph) -> Self;
}

impl<'a, T: Component> LayerBundle<'a> for LayerView<'a, T> {
    fn get_from_graph(graph: &'a Graph) -> Self {
        graph.get_layer()
    }
}
impl<'a, T: Component> LayerBundle<'a> for LayerViewMut<'a, T> {
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
