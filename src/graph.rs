//! Starframe's entity system, i.e. the data structure representing game objects.
//!
//! A game object in Starframe is a _directed graph_ of components.
//! Conceptually, one may look something like this:
//! ```text
//!   Pose <----> Body <----> Collider
//!    ^            ^
//!    |            |
//!    v            v
//!  Sprite     EventSink
//!    |
//!    |
//!    v
//! Texture
//! ```
//! Most edges (connections between nodes) should go both ways, but because the graph is directed,
//! one-directional edges such as from Sprite to Texture in the above diagram can also be made.
//! This is important to the way object boundaries are determined by the deletion algorithm (see
//! [`delete`][self::Graph::delete] for details). This comes into play when sharing one component
//! between multiple objects.
//!
//! Similarly to how systems in ECS iterate over specific sets of components,
//! systems using the graph iterate over specific *patterns* of connected nodes.
//!
//! # Usage example
//! ```
//! # use starframe::{graph::{make_graph, LayerViewMut}, math::Pose};
//!
//! struct Player;
//! enum Hat {
//!     Fedora,
//!     PropellerHat,
//! }
//! struct Sword {
//!     coolness_level: usize,
//! }
//!
//! type MyGraph = make_graph! {
//!     Player,
//!     Hat,
//!     Sword,
//! };
//! let graph = MyGraph::new();
//!
//! fn spawn_player(
//!     pose: Pose,
//!     hat: Hat,
//!     sword: Sword,
//!     (mut l_pose, mut l_player, mut l_hat, mut l_sword): (
//!         LayerViewMut<Pose>,
//!         LayerViewMut<Player>,
//!         LayerViewMut<Hat>,
//!         LayerViewMut<Sword>,
//!     )
//! ) {
//!     let mut pose_node = l_pose.insert(pose);
//!     let mut player_node = l_player.insert(Player);
//!     let mut hat_node = l_hat.insert(hat);
//!     let mut sword_node = l_sword.insert(sword);
//!
//!     player_node.connect(&mut pose_node);
//!     player_node.connect(&mut hat_node);
//!     player_node.connect(&mut sword_node);
//! }
//!
//! spawn_player(
//!     Pose::default(),
//!     Hat::PropellerHat,
//!     Sword { coolness_level: 9001 },
//!     graph.get_layer_bundle(),
//! );
//! spawn_player(
//!     Pose::default(),
//!     Hat::Fedora,
//!     Sword { coolness_level: 1 },
//!     graph.get_layer_bundle(),
//! );
//!
//! let l_player = graph.get_layer::<Player>();
//! let l_sword = graph.get_layer::<Sword>();
//! for player_node in l_player.iter() {
//!     if let Some(sword) = player_node.get_neighbor(&l_sword) {
//!         println!("Watch out, this guy's got a lvl {} sword", sword.c.coolness_level);
//!     }
//! }
//! ```
//!
//! See individual types' documentation for details.

use std::{any::Any, collections::VecDeque, marker::PhantomData};

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

//

#[macro_use]
mod component_defs;
pub use crate::make_graph;
#[doc(hidden)]
pub use component_defs::BUILTIN_LAYER_COUNT;

mod layer_bundle;
pub use layer_bundle::LayerBundle;

//
// Index & ref types
//

type ComponentIdx = usize;
type GenerationIdx = usize;
type Refcount = usize;

/// Allows types to be inserted into the graph.
/// Implemented for custom types using the [`make_graph`][self::make_graph] macro;
/// do not implement manually!
pub trait Component: 'static + Send + Sync {
    /// Address of this type's graph layer.
    const LAYER_INDEX: usize;
}

/// Node position without generation info, used internally to traverse the graph
/// without knowing types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BareNodeKey {
    layer: usize,
    idx: usize,
}
impl<T: Component> From<NodeKey<T>> for BareNodeKey {
    fn from(key: NodeKey<T>) -> Self {
        Self {
            layer: T::LAYER_INDEX,
            idx: key.idx,
        }
    }
}

/// An identifier for looking up a specific node.
pub struct NodeKey<T: Component> {
    pub(crate) idx: usize,
    pub(crate) gen: usize,
    pub(crate) _marker: PhantomData<T>,
}
impl<'a, T: Component> From<NodeRef<'a, T>> for NodeKey<T> {
    fn from(node: NodeRef<'a, T>) -> Self {
        Self {
            idx: node.idx,
            gen: node.layer_meta.generations[node.idx],
            _marker: PhantomData,
        }
    }
}
impl<'a, T: Component> From<NodeRefMut<'a, T>> for NodeKey<T> {
    fn from(node: NodeRefMut<'a, T>) -> Self {
        Self {
            idx: node.idx,
            gen: node.layer_meta.generations[node.idx],
            _marker: PhantomData,
        }
    }
}
// blanket impls required because phantomdata makes derive unnecessarily restrict type of T
impl<T: Component> Clone for NodeKey<T> {
    fn clone(&self) -> Self {
        NodeKey {
            idx: self.idx,
            gen: self.idx,
            _marker: PhantomData,
        }
    }
}
impl<T: Component> Copy for NodeKey<T> {}
impl<T: Component> std::fmt::Debug for NodeKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Node {{\n idx: {:?},\n gen: {},\n}}",
            self.idx, self.gen
        ))
    }
}
impl<T: Component> PartialEq for NodeKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.gen == other.gen
    }
}
impl<T: Component> Eq for NodeKey<T> {}
impl<T: Component> std::hash::Hash for NodeKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
    }
}

/// An immutable reference to a node in the graph.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a, T: Component> {
    /// The component that this node points to.
    pub c: &'a T,
    idx: usize,
    layer_meta: &'a LayerMetadata,
}

impl<'a, T: Component> NodeRef<'a, T> {
    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr LayerView<'l, Target>,
    ) -> Option<NodeRef<'lr, Target>> {
        get_neighbor(self.layer_meta, self.idx, layer)
    }

    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a mutable reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor_mut<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr mut LayerViewMut<'l, Target>,
    ) -> Option<NodeRefMut<'lr, Target>> {
        get_neighbor_mut(self.layer_meta, self.idx, layer)
    }

    /// Get a key that can be used to access this node later.
    #[inline]
    pub fn key(&self) -> NodeKey<T> {
        NodeKey {
            idx: self.idx,
            gen: self.layer_meta.generations[self.idx],
            _marker: PhantomData,
        }
    }
}

/// A mutable reference to a node in the graph.
pub struct NodeRefMut<'a, T: Component> {
    /// The component that this node points to.
    pub c: &'a mut T,
    idx: usize,
    layer_meta: &'a mut LayerMetadata,
}

impl<'a, T: Component> NodeRefMut<'a, T> {
    /// Add an edge going both ways between this and another node.
    ///
    /// This makes both nodes hierarchically equal parts of the same object.
    /// Both nodes can find each other with [`get_neighbor`][Self::get_neighbor],
    /// and both will (in most cases) be deleted if `Graph::delete` is called on either one.
    ///
    /// Internally this calls [`connect_oneway`][Self::connect_oneway] twice, so all the same caveats apply.
    ///
    /// # Panics
    /// Panics if either edge this creates would make `connect_oneway` panic.
    #[inline]
    pub fn connect<Other: Component>(&mut self, other: &mut NodeRefMut<'_, Other>) {
        self.connect_oneway(other);
        other.connect_oneway(self);
    }

    /// Create an edge from this node to another, only going in that direction.
    ///
    /// In a sense, this makes the other node hierarchically lower than this one.
    /// If the lower node is deleted, this one won't be, but if this one is deleted,
    /// the lower one will be too, unless there are other nodes also pointing to it with
    /// a one-directional edge.
    ///
    /// # Panics
    /// The current graph implementation is limited to one edge per node per layer.
    /// If an edge from this node to somewhere in the other's layer already exists,
    /// this function will panic, because this signals
    /// that you're creating a malformed object that won't work the way you expect.
    pub fn connect_oneway<Other: Component>(&mut self, other: &mut NodeRefMut<'_, Other>) {
        if self.layer_meta.edges.len() <= Other::LAYER_INDEX {
            self.layer_meta
                .edges
                .resize_with(Other::LAYER_INDEX + 1, Vec::new);
        }
        let edges = &mut self.layer_meta.edges[Other::LAYER_INDEX];
        if edges.len() <= self.idx {
            edges.resize(self.idx + 1, None);
        }
        let prev_val = edges[self.idx].replace(other.idx);
        assert!(
            prev_val.is_none(),
            "Multiple edges to the same layer from the same component are not supported"
        );
        other.layer_meta.refcounts[other.idx] += 1;
    }

    /// Like [`connect_oneway`][Self::connect_oneway], but for the special case
    /// of connecting to a node in the same layer.
    ///
    /// This isn't possible otherwise because `NodeRefMut` mutably borrows the layer
    /// it points to. It's a little finicky, but fortunately this is a relatively rare case.
    /// # Example
    /// ```
    /// # use starframe::{graph::make_graph, physics::Body};
    /// # type Graph = make_graph!{};
    /// # let graph = Graph::new();
    /// let mut l_body = graph.get_layer_mut::<Body>();
    /// let b2 = l_body.insert(Body::new_particle(1.0));
    /// let b2 = b2.key();
    /// let mut b1 = l_body.insert(Body::new_particle(1.0));
    /// b1.connect_oneway_same_layer(b2);
    /// ```
    pub fn connect_oneway_same_layer(&mut self, other: NodeKey<T>) {
        if other.gen != self.layer_meta.generations[other.idx] {
            return;
        }
        let edges = &mut self.layer_meta.edges[T::LAYER_INDEX];
        if edges.len() <= self.idx {
            edges.resize(self.idx + 1, None);
        }
        let prev_val = edges[self.idx].replace(other.idx);
        assert!(
            prev_val.is_none(),
            "Multiple edges to the same layer from the same component are not supported"
        );
        self.layer_meta.refcounts[other.idx] += 1;
    }

    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr LayerView<'l, Target>,
    ) -> Option<NodeRef<'lr, Target>> {
        get_neighbor(self.layer_meta, self.idx, layer)
    }

    /// If there's an edge starting from this node and ending at a node of the given type,
    /// get a mutable reference to that node, otherwise return None.
    #[inline]
    pub fn get_neighbor_mut<'lr, 'l, Target: Component>(
        &self,
        layer: &'lr mut LayerViewMut<'l, Target>,
    ) -> Option<NodeRefMut<'lr, Target>> {
        get_neighbor_mut(self.layer_meta, self.idx, layer)
    }

    /// Get a key that can be used to access this node later.
    #[inline]
    pub fn key(&self) -> NodeKey<T> {
        NodeKey {
            idx: self.idx,
            gen: self.layer_meta.generations[self.idx],
            _marker: PhantomData,
        }
    }
}

fn get_neighbor_idx<Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
) -> Option<usize> {
    if node_layer_meta.edges.len() <= Target::LAYER_INDEX
        || node_layer_meta.edges[Target::LAYER_INDEX].len() <= node_idx
    {
        None
    } else {
        node_layer_meta.edges[Target::LAYER_INDEX][node_idx]
    }
}

fn get_neighbor<'lr, 'l, Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
    target_layer: &'lr LayerView<'l, Target>,
) -> Option<NodeRef<'lr, Target>> {
    get_neighbor_idx::<Target>(node_layer_meta, node_idx).map(|target| NodeRef {
        c: &target_layer.components[target],
        idx: target,
        layer_meta: target_layer.meta,
    })
}

fn get_neighbor_mut<'lr, 'l, Target: Component>(
    node_layer_meta: &LayerMetadata,
    node_idx: usize,
    target_layer: &'lr mut LayerViewMut<'l, Target>,
) -> Option<NodeRefMut<'lr, Target>> {
    get_neighbor_idx::<Target>(node_layer_meta, node_idx).map(move |target| NodeRefMut {
        c: &mut target_layer.components[target],
        idx: target,
        layer_meta: target_layer.meta,
    })
}

//
// Layers
//

/// Tracking edges, refcounts, generations and vacant slots for a single layer.
#[derive(Debug)]
struct LayerMetadata {
    edges: Vec<Vec<Option<ComponentIdx>>>,
    refcounts: Vec<Refcount>,
    generations: Vec<GenerationIdx>,
    vacant_slots: VecDeque<ComponentIdx>,
}
impl LayerMetadata {
    fn new(layer_count: usize) -> Self {
        Self {
            edges: vec![Vec::new(); layer_count],
            refcounts: Vec::new(),
            generations: Vec::new(),
            vacant_slots: VecDeque::new(),
        }
    }
}

/// Storage type allowing us to store all layers in a single Vec
/// and access their metadata without having to know their type.
#[derive(Debug)]
struct TypeErasedLayer {
    meta: LayerMetadata,
    components: list_any::VecAny,
}
impl TypeErasedLayer {
    fn new(layer_count: usize) -> Self {
        Self {
            meta: LayerMetadata::new(layer_count),
            components: list_any::VecAny::deferred(),
        }
    }
}

/// An immutable view into a single layer of the graph.
///
/// Acquired with [`Graph::get_layer`][self::Graph::get_layer_mut] or as a part of
/// [`Graph::get_layer_bundle`][self::Graph::get_layer_bundle].
pub struct LayerView<'a, T: Component> {
    meta: &'a LayerMetadata,
    pub(crate) components: &'a [T],
    // Using the same unsafe pattern as with `LayerViewMut`,
    // even though it's not _strictly_ necessary here.
    // The reason why (and why it's in an Option) is so that we can implement
    // borrowing a LayerView from a LayerViewMut so we're not restricted to one or the other
    // in function parameters.
    _guard: Option<RwLockReadGuard<'a, TypeErasedLayer>>,
}

impl<'a, T: Component> LayerView<'a, T> {
    /// Get an immutable reference to a node if it still exists, otherwise return None.
    pub fn get(&self, key: NodeKey<T>) -> Option<NodeRef<'_, T>> {
        if self.meta.generations.len() <= key.idx || self.meta.generations[key.idx] != key.gen {
            None
        } else {
            Some(self.get_unchecked(key))
        }
    }

    /// Get an immutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_unchecked(&self, key: NodeKey<T>) -> NodeRef<'_, T> {
        self.get_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_unchecked_by_item_idx(&self, idx: usize) -> NodeRef<'_, T> {
        NodeRef {
            c: &self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            layer_meta: &*self.meta,
            comp_iter: self.components.iter().enumerate(),
        }
    }
}

/// A mutable view into a single layer of the graph.
///
/// Acquired with [`Graph::get_layer_mut`][self::Graph::get_layer_mut] or as a part of
/// [`Graph::get_layer_bundle`][self::Graph::get_layer_bundle].
pub struct LayerViewMut<'a, T: Component> {
    meta: &'a mut LayerMetadata,
    pub(crate) components: list_any::VecAnyGuard<'a, T, dyn Any + Send + Sync + 'static>,
    // Storing the lock guard inside this
    // because I can't figure out a way to map it cleanly to a view like this.
    // This requires unsafe and is ugly :(
    // SAFETY: never access the guard, only use above fields.
    // Because it's in the same struct as the references to its inside, all drop at the same time
    _guard: RwLockWriteGuard<'a, TypeErasedLayer>,
}

impl<'a, T: Component> LayerViewMut<'a, T> {
    /// Insert a component into the layer.
    ///
    /// This returns a reference to the node that was created,
    /// which you can use to connect it to other nodes.
    /// # Example
    /// ```
    /// # use starframe::{graph::make_graph, math::Pose, physics::Collider};
    /// # type MyGraph = make_graph!{};
    /// # let graph = MyGraph::new();
    /// let mut l_pose = graph.get_layer_mut::<Pose>();
    /// let mut l_collider = graph.get_layer_mut::<Collider>();
    /// let mut pose_node = l_pose.insert(Pose::default());
    /// let mut collider_node = l_collider.insert(Collider::new_circle(1.0));
    /// pose_node.connect(&mut collider_node);
    /// ```
    pub fn insert(&mut self, component: T) -> NodeRefMut<'_, T> {
        let item_idx = if let Some(vacant_slot) = self.meta.vacant_slots.pop_front() {
            // no generation increment here, that happens on delete
            self.components[vacant_slot] = component;
            vacant_slot
        } else {
            self.components.push(component);
            self.meta.refcounts.push(0);
            self.meta.generations.push(0);
            self.components.len() - 1
        };

        NodeRefMut {
            c: &mut self.components[item_idx],
            idx: item_idx,
            layer_meta: &mut self.meta,
        }
    }

    /// Get an immutable reference to a node if it still exists, otherwise return None.
    pub fn get(&self, key: NodeKey<T>) -> Option<NodeRef<'_, T>> {
        if self.meta.generations[key.idx] != key.gen {
            None
        } else {
            Some(self.get_unchecked(key))
        }
    }

    /// Get an immutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_unchecked(&self, key: NodeKey<T>) -> NodeRef<'_, T> {
        self.get_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_unchecked_by_item_idx(&self, idx: usize) -> NodeRef<'_, T> {
        NodeRef {
            c: &self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    /// Get a mutable reference to a node if it still exists, otherwise return None.
    pub fn get_mut(&mut self, key: NodeKey<T>) -> Option<NodeRefMut<'_, T>> {
        if self.meta.generations.len() <= key.idx || self.meta.generations[key.idx] != key.gen {
            None
        } else {
            Some(self.get_mut_unchecked(key))
        }
    }

    /// Get a mutable reference to a node without checking if it still exists.
    /// Use with caution.
    #[inline]
    pub fn get_mut_unchecked(&mut self, key: NodeKey<T>) -> NodeRefMut<'_, T> {
        self.get_mut_unchecked_by_item_idx(key.idx)
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_mut_unchecked_by_item_idx(&mut self, idx: usize) -> NodeRefMut<'_, T> {
        NodeRefMut {
            c: &mut self.components[idx],
            idx,
            layer_meta: self.meta,
        }
    }

    pub fn iter(&self) -> LayerIter<'_, T> {
        LayerIter {
            layer_meta: self.meta,
            comp_iter: self.components.iter().enumerate(),
        }
    }

    pub fn iter_mut(&mut self) -> LayerIterMut<'_, T> {
        LayerIterMut {
            layer_meta: self.meta,
            comp_iter: self.components.iter_mut().enumerate(),
        }
    }

    /// Take an immutable view into this mutable view.
    pub fn as_view(&self) -> LayerView<'_, T> {
        LayerView {
            meta: self.meta,
            components: &self.components,
            _guard: None,
        }
    }
}

//
// Iterators
//

/// An immutable iterator over components in a single layer.
///
/// Create with the `iter` method on [`LayerView`][self::LayerView]
/// or [`LayerViewMut`][self::LayerViewMut].
pub struct LayerIter<'a, T: Component> {
    layer_meta: &'a LayerMetadata,
    comp_iter: std::iter::Enumerate<std::slice::Iter<'a, T>>,
}

impl<'a, T: Component> Iterator for LayerIter<'a, T> {
    type Item = NodeRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let (next_idx, next) = loop {
            let (next_idx, next) = self.comp_iter.next()?;
            if self.layer_meta.refcounts[next_idx] > 0 {
                break (next_idx, next);
            }
        };
        Some(NodeRef {
            c: next,
            idx: next_idx,
            layer_meta: self.layer_meta,
        })
    }
}

/// A mutable iterator over components in a single layer.
///
/// Create with the `iter_mut` method on [`LayerViewMut`][self::LayerViewMut].
pub struct LayerIterMut<'a, T: Component> {
    layer_meta: &'a mut LayerMetadata,
    comp_iter: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
}

impl<'a, T: Component> Iterator for LayerIterMut<'a, T> {
    type Item = NodeRefMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let (next_idx, next) = loop {
            let (next_idx, next) = self.comp_iter.next()?;
            if self.layer_meta.refcounts[next_idx] > 0 {
                break (next_idx, next);
            }
        };
        let layer_meta: *mut LayerMetadata = self.layer_meta;
        Some(NodeRefMut {
            c: next,
            idx: next_idx,
            // SAFETY: aliasing layer_meta is ok here because we only ever use it
            // to access node-specific metadata.
            // If we could only store references to node-specific metadata in the NodeRef
            // and use slice iterators to avoid the unsafe here that would be neat,
            // but due to how edges are indexed this isn't feasible.
            layer_meta: unsafe { &mut *layer_meta },
        })
    }
}

//
// Graph
//

/// The component graph itself.
///
/// Use the [`make_graph`][self::make_graph] macro to automatically create one
/// with the correct layer count.
/// See that macro's documentation for an example and some talk of limitations.
///
/// A graph is built out of _layers_, one per type of component stored.
/// Layers contain _nodes_ representing individual components,
/// and these are connected to other components with _edges_.
#[derive(Debug)]
pub struct Graph<const LAYER_COUNT: usize> {
    layers: Vec<RwLock<TypeErasedLayer>>,
}

impl<const LAYER_COUNT: usize> Default for Graph<LAYER_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const LAYER_COUNT: usize> Graph<LAYER_COUNT> {
    pub fn new() -> Self {
        let mut layers = Vec::new();
        layers.resize_with(LAYER_COUNT, || {
            RwLock::new(TypeErasedLayer::new(LAYER_COUNT))
        });
        Self { layers }
    }

    /// Lock a layer for reading.
    /// # Panics
    /// Panics if the layer is currently locked for writing.
    pub fn get_layer<T: Component>(&self) -> LayerView<'_, T> {
        let err = || {
            // not sure if panic here is the right call,
            // but it's surely better than having it hang forever in case of a conflict
            panic!(
                "Could not lock layer for reading: {}",
                std::any::type_name::<T>()
            )
        };
        let guard = self.layers[T::LAYER_INDEX].try_read().unwrap_or_else(err);
        // taking references to things inside the lock for the sake of API.
        // SAFETY: the guard will drop at the same time as the references
        // and we never access the guard itself.
        unsafe {
            let meta: *const LayerMetadata = &guard.meta;
            let components: *const list_any::VecAny = &guard.components;
            LayerView {
                meta: &*meta,
                components: (&*components).downcast_slice().unwrap(),
                _guard: Some(guard),
            }
        }
    }

    /// Lock a layer for writing.
    /// # Panics
    /// Panics if the layer is currently locked for reading or writing.
    pub fn get_layer_mut<T: Component>(&self) -> LayerViewMut<'_, T> {
        let err = || {
            panic!(
                "Could not lock layer for writing: {}",
                std::any::type_name::<T>()
            )
        };
        let mut guard = self.layers[T::LAYER_INDEX].try_write().unwrap_or_else(err);
        // taking references to things inside the lock for the sake of API.
        // SAFETY: the guard will drop at the same time as the references
        // and we never access the guard itself.
        unsafe {
            let meta: *mut LayerMetadata = &mut guard.meta;
            let components: *mut list_any::VecAny = &mut guard.components;
            LayerViewMut {
                meta: &mut *meta,
                components: (&mut *components).downcast_mut().unwrap(),
                _guard: guard,
            }
        }
    }

    /// Get a tuple of layer views in one call.
    /// This is a common pattern in arguments for functions that manipulate the graph.
    /// # Example
    /// ```
    /// # use starframe::{
    /// #    math::Pose,
    /// #    physics::{Body, Collider},
    /// #    graph::{LayerView, LayerViewMut, make_graph}
    /// # };
    /// # type MyGraph = make_graph!{};
    /// # let graph = MyGraph::new();
    ///
    /// fn do_things_with_bodies(
    ///     how_many_things: usize,
    ///     for_how_long: usize,
    ///     (l_pose, l_body, l_collider): (
    ///         LayerViewMut<Pose>,
    ///         LayerViewMut<Body>,
    ///         LayerView<Collider>,
    ///     ),
    /// ) {
    ///     // do stuff...
    /// }
    ///
    /// do_things_with_bodies(42, 69, graph.get_layer_bundle());
    /// ```
    pub fn get_layer_bundle<'a, B: LayerBundle<'a>>(&'a self) -> B {
        B::get_from_graph(self)
    }

    fn write_all_layers(&mut self) -> Vec<RwLockWriteGuard<'_, TypeErasedLayer>> {
        self.layers
            .iter()
            .map(|lock| {
                lock.try_write()
                    .expect("One or more layers were in use when trying to delete")
            })
            .collect()
    }

    /// Drop all content and recreate the graph from scratch.
    pub fn reset(&mut self) {
        let layer_count = self.layers.len();
        for mut layer in self.write_all_layers() {
            *layer = TypeErasedLayer::new(layer_count);
        }
    }

    /// Delete a whole _object_ from the graph, beginning from the given node.
    /// This needs write access to all layers, so every layer must be unlocked.
    ///
    /// What constitutes a single object in the graph isn't quite straightforward due to the
    /// number of ways in which nodes can be connected.
    /// The deletion algorithm performs a depth-first search to find and delete every component
    /// reachable from the starting node, stopping at shared components.
    /// An individual node is considered deleted once it no longer has any edges pointing to it.
    ///
    /// Illustrated example:
    /// ```text
    /// O<->(O)<->O-->O<--O<->O
    ///               ^
    ///               L->O
    /// Delete edges, starting from (O) (note how it stops on the "shared" node)
    /// O   (O)   O   O<--O<->O
    ///               ^
    ///               L->O
    /// Remove nodes without any more edges
    /// O<--O<->O
    /// ^
    /// L->O
    /// Done!
    /// ```
    /// There are a lot of nuances to this depending on object structure, but the vast majority of the time
    /// objects will just be their own islands in the graph where everything is connected with bidirectional edges.
    /// In these cases, the whole island will be deleted regardless of which node you start on.
    ///
    /// # Panics
    /// Panics if any layer is locked for reading or writing.
    pub fn delete<T: Component>(&mut self, root: NodeKey<T>) {
        #[derive(Clone, Copy, Debug)]
        struct VisitedNode {
            node: BareNodeKey,
            visit_count: Refcount,
            all_refs_visited: bool,
        }

        #[derive(Clone, Copy, Debug)]
        struct VisitedEdge {
            start_idx: usize,
            end_idx: usize,
            is_shared: bool,
        }

        let mut locked_layers = self.write_all_layers();

        // check if the node is already considered deleted before doing anything
        let start_layer = &locked_layers[T::LAYER_INDEX].meta;
        if start_layer.generations.len() <= root.idx
            || start_layer.generations[root.idx] != root.gen
            || start_layer.refcounts.len() <= root.idx
            || start_layer.refcounts[root.idx] == 0
        {
            return;
        }

        let root: BareNodeKey = root.into();
        let mut visited_nodes = vec![VisitedNode {
            node: root,
            visit_count: 0,
            all_refs_visited: false,
        }];
        let mut visited_edges: Vec<VisitedEdge> = Vec::new();

        // recursive depth first search to find all nodes and edges
        fn search_all(
            curr_node: BareNodeKey,
            curr_node_idx: usize,
            visited_nodes: &mut Vec<VisitedNode>,
            visited_edges: &mut Vec<VisitedEdge>,
            locked_layers: &[RwLockWriteGuard<TypeErasedLayer>],
        ) {
            let curr_layer = &locked_layers[curr_node.layer];
            for (target_layer_idx, edges_to_target) in curr_layer.meta.edges.iter().enumerate() {
                if curr_node.idx < edges_to_target.len() {
                    if let Some(other_item_idx) = edges_to_target[curr_node.idx] {
                        let next_node = BareNodeKey {
                            layer: target_layer_idx,
                            idx: other_item_idx,
                        };

                        if let Some(already_seen) =
                            visited_nodes.iter().position(|n| n.node == next_node)
                        {
                            visited_edges.push(VisitedEdge {
                                start_idx: curr_node_idx,
                                end_idx: already_seen,
                                is_shared: false,
                            });
                            visited_nodes[already_seen].visit_count += 1;
                        } else {
                            let next_node_idx = visited_nodes.len();
                            visited_edges.push(VisitedEdge {
                                start_idx: curr_node_idx,
                                end_idx: next_node_idx,
                                is_shared: false,
                            });
                            visited_nodes.push(VisitedNode {
                                node: next_node,
                                visit_count: 1,
                                all_refs_visited: false,
                            });
                            search_all(
                                next_node,
                                next_node_idx,
                                visited_nodes,
                                visited_edges,
                                locked_layers,
                            );
                        }
                    }
                }
            }
        }
        search_all(
            root,
            0,
            &mut visited_nodes,
            &mut visited_edges,
            &locked_layers,
        );

        for vis in visited_nodes.iter_mut() {
            if locked_layers[vis.node.layer].meta.refcounts[vis.node.idx] == vis.visit_count {
                vis.all_refs_visited = true;
            }
        }

        // identify shared nodes and remove edges found after them
        for (node_idx, node) in visited_nodes.iter().enumerate() {
            if !node.all_refs_visited {
                fn remove_edges_past_node(curr_node_idx: usize, visited_edges: &mut [VisitedEdge]) {
                    let edge_idxs_from_node: Vec<usize> = visited_edges
                        .iter()
                        .enumerate()
                        .filter(|(_, edge)| edge.start_idx == curr_node_idx)
                        .map(|(idx, _)| idx)
                        .collect();
                    for edge_idx in edge_idxs_from_node {
                        // check that we didn't already go through here to avoid infinite loop
                        if !visited_edges[edge_idx].is_shared {
                            visited_edges[edge_idx].is_shared = true;
                            remove_edges_past_node(visited_edges[edge_idx].end_idx, visited_edges);
                        }
                    }
                }
                remove_edges_past_node(node_idx, &mut visited_edges);
            }
        }
        // remove edges not marked as shared
        for owned_edge in visited_edges.iter().filter(|e| !e.is_shared) {
            let start_node = visited_nodes[owned_edge.start_idx].node;
            let end_node = visited_nodes[owned_edge.end_idx].node;
            locked_layers[start_node.layer].meta.edges[end_node.layer][start_node.idx] = None;
            locked_layers[end_node.layer].meta.refcounts[end_node.idx] -= 1;
        }

        for vis_node in visited_nodes {
            let node = vis_node.node;
            let layer = &mut locked_layers[node.layer];
            if layer.meta.refcounts[node.idx] == 0 {
                debug_assert!(
                    !layer.meta.vacant_slots.iter().any(|&idx| idx == node.idx),
                    "Same slot marked vacant twice ({:?})",
                    node,
                );
                layer.meta.vacant_slots.push_back(node.idx);
                layer.meta.generations[node.idx] += 1;
            }
        }
    }
}

//
// Tests
//

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Pose(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Velocity(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Body(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Shape(usize);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Subshape(usize);

    type Graph = make_graph! {
        Pose,
        Velocity,
        Body,
        Shape,
        Subshape,
    };

    // shorthands for layer views because we have to repeat this stuff a lot here
    type L<'a, T> = LayerView<'a, T>;
    type LM<'a, T> = LayerViewMut<'a, T>;
    type AllLayers<'a> = (
        L<'a, Pose>,
        L<'a, Velocity>,
        L<'a, Body>,
        L<'a, Shape>,
        L<'a, Subshape>,
    );
    type AllLayersMut<'a> = (
        LM<'a, Pose>,
        LM<'a, Velocity>,
        LM<'a, Body>,
        LM<'a, Shape>,
        LM<'a, Subshape>,
    );

    /// Nodes can be connected and then queried for their neighbors.
    /// Multiple ownership works.
    #[test]
    fn connect_nodes() {
        let graph = Graph::new();

        let everyones_shape = graph.get_layer_mut().insert(Shape(69)).key();

        // do this a few times to make sure we connect correctly even with multiple objects there
        for i in 0..3 {
            let pose_key;
            let rb_key;
            {
                let (mut poses, mut vels, mut bodies, mut shapes, _) =
                    graph.get_layer_bundle::<AllLayersMut>();

                let mut everyones_shape = &mut shapes.get_mut(everyones_shape).unwrap();

                let mut pose_node = poses.insert(Pose(i));
                let mut vel_node = vels.insert(Velocity(i));
                let mut body_node = bodies.insert(Body(i));
                vel_node.connect(&mut pose_node);
                body_node.connect(&mut pose_node);
                body_node.connect(&mut vel_node);
                body_node.connect_oneway(&mut everyones_shape);
                // refcounts
                assert_eq!(pose_node.layer_meta.refcounts[pose_node.idx], 2);
                assert_eq!(body_node.layer_meta.refcounts[body_node.idx], 2);
                assert_eq!(
                    everyones_shape.layer_meta.refcounts[everyones_shape.idx],
                    i + 1
                );
                // neighbors are found
                // (getting them is cumbersome here because we have to juggle layer references
                // back and forth to drop mutable refs, but this won't be done in real code)
                pose_key = pose_node.key();
                rb_key = body_node.key();
            }
            {
                let (poses, _, bodies, shapes, _) = graph.get_layer_bundle::<AllLayers>();
                assert_eq!(
                    *bodies.get(rb_key).unwrap().get_neighbor(&shapes).unwrap().c,
                    Shape(69)
                );
                assert_eq!(
                    *poses
                        .get(pose_key)
                        .unwrap()
                        .get_neighbor(&bodies)
                        .unwrap()
                        .c,
                    Body(i)
                );
                assert!(poses.get(pose_key).unwrap().get_neighbor(&shapes).is_none());
            }

            // spawn something with different connections in between
            let mut poses = graph.get_layer_mut::<Pose>();
            let mut shapes = graph.get_layer_mut::<Shape>();
            let mut pose_node_ = poses.insert(Pose(42 + i));
            let mut shape_node_ = shapes.insert(Shape(i));
            pose_node_.connect(&mut shape_node_);
            let pose_key_ = pose_node_.key();
            drop(poses);
            drop(shapes);
            assert_eq!(
                *graph
                    .get_layer::<Pose>()
                    .get(pose_key_)
                    .unwrap()
                    .get_neighbor(&graph.get_layer::<Shape>())
                    .unwrap()
                    .c,
                Shape(i)
            );
        }
    }

    #[test]
    fn iterate() {
        let graph = Graph::new();
        let (mut poses, mut vels, mut bodies, mut shapes, _) =
            graph.get_layer_bundle::<AllLayersMut>();

        let everyones_shape = shapes.insert(Shape(69)).key();

        for i in 0..10 {
            let mut pose_node = poses.insert(Pose(i));
            let mut vel_node = vels.insert(Velocity(i));
            let mut body_node = bodies.insert(Body(0));
            body_node.connect(&mut pose_node);
            if i % 2 == 0 {
                pose_node.connect(&mut vel_node);
            }
            if i % 4 == 0 {
                body_node.connect_oneway(&mut shapes.get_mut(everyones_shape).unwrap());
            }
        }

        drop(poses);
        drop(vels);

        let poses = graph.get_layer::<Pose>();
        let vels = graph.get_layer::<Velocity>();

        println!("Patterns of `iterate`:");

        let mut match_count = 0; // not including shape
        let mut full_match_count = 0; // including shape
        for mut body in bodies.iter_mut() {
            let pose = match body.get_neighbor(&poses) {
                Some(pose) => pose,
                None => continue,
            };
            let vel = match pose.get_neighbor(&vels) {
                Some(vel) => vel,
                None => continue,
            };
            match_count += 1;
            body.c.0 = 42;

            let mut shape = body.get_neighbor_mut(&mut shapes);
            if let Some(shape) = &mut shape {
                full_match_count += 1;
                shape.c.0 += 1;
            }

            // test that only real connections were followed
            assert_eq!(vel.c.0 % 2, 0);

            println!(
                "{:?}, {:?}, {:?}, {:?}",
                body.c,
                pose.c,
                vel.c,
                shape.map(|s| s.c)
            );
        }
        assert_eq!(match_count, 5);
        assert_eq!(full_match_count, 3);
        assert_eq!(*shapes.get(everyones_shape).unwrap().c, Shape(72));

        println!("All rbs: {:?}", bodies.components);

        for body in bodies.iter() {
            if body
                .get_neighbor(&poses)
                .and_then(|pose| pose.get_neighbor(&vels))
                .is_none()
            {
                assert_eq!(body.c.0, 0);
            } else {
                assert_eq!(body.c.0, 42);
            }
        }
    }

    #[test]
    fn delete() {
        let mut graph = Graph::new();

        let vels_to_del: Vec<NodeKey<Velocity>> = {
            let (mut poses, mut vels, mut bodies, mut shapes, mut sub_shapes) =
                graph.get_layer_bundle::<AllLayersMut>();

            let mut everyones_shape = shapes.insert(Shape(69));
            let mut shape_owns_thing = sub_shapes.insert(Subshape(42));
            everyones_shape.connect(&mut shape_owns_thing);

            for i in 0..10 {
                let mut pose_node = poses.insert(Pose(i));
                let mut vel_node = vels.insert(Velocity(i));
                let mut body_node = bodies.insert(Body(0));
                body_node.connect(&mut pose_node);
                if i % 2 == 0 {
                    pose_node.connect(&mut vel_node);
                } else {
                    let mut subshape = sub_shapes.insert(Subshape(i));
                    vel_node.connect(&mut subshape);
                }
                if i % 3 == 0 {
                    body_node.connect_oneway(&mut everyones_shape);
                }
            }

            assert_eq!(vels.iter().count(), 10);

            vels.iter().map(|v| v.key()).collect()
        };
        for vel_to_del in vels_to_del {
            graph.delete(vel_to_del);
        }
        // all vels deleted (== have 0 referrers)
        assert_eq!(graph.get_layer::<Velocity>().iter().count(), 0);
        // half of poses had vels attached and have also been deleted
        assert_eq!(graph.get_layer::<Pose>().iter().count(), 5);

        let bodies_to_del: Vec<NodeKey<Body>> = graph
            .get_layer::<Body>()
            .iter()
            .map(|rb| rb.key())
            .collect();
        // rbs are connected to everything so deleting all of them should delete everything
        for rb_to_del in bodies_to_del {
            // everyones_shape and its subcomponent should live until the last rb is deleted
            // BECAUSE the last rb is connected to it
            // (remember this if changing the iteration counts!)
            assert_eq!(graph.get_layer::<Shape>().iter().count(), 1);
            assert_eq!(graph.get_layer::<Subshape>().iter().count(), 1);

            graph.delete(rb_to_del);
        }
        assert_eq!(graph.get_layer::<Shape>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Pose>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Velocity>().iter().count(), 0);
        assert_eq!(graph.get_layer::<Body>().iter().count(), 0);

        // check that edges were all cleared too
        for (layer_idx, layer) in graph.layers.iter().map(|l| l.read()).enumerate() {
            for edge in layer.meta.edges.iter().flat_map(|e| e.iter()) {
                assert!(edge.is_none(), "Layer {} had a non-empty edge", layer_idx);
            }
        }
    }

    #[test]
    fn reuse_deleted_slots() {
        let mut graph = Graph::new();

        let mut shapes = graph.get_layer_mut::<Shape>();
        let mut subshapes = graph.get_layer_mut::<Subshape>();

        let mut everyones_shape = shapes.insert(Shape(69));
        // connect everyones_shape to something to make sure we don't accidentally delete it
        let mut shape_guardian = subshapes.insert(Subshape(69));
        shape_guardian.connect_oneway(&mut everyones_shape);
        let everyones_shape = everyones_shape.key();
        drop(subshapes);
        drop(shapes);

        // delete and respawn stuff a few times to hopefully see if things get connected wrong at some point
        // (this doesn't prove things won't go wrong over a long enough time but fingers crossed)
        for i in 0..5 {
            let pose_nodes: Vec<NodeKey<Pose>> = graph
                .get_layer_mut::<Pose>()
                .iter()
                // delete half of everything that was placed,
                // so every new iteration we should be reusing slots for half and pushing new for half
                .take(5)
                .map(|pose| pose.key())
                .collect();
            for pose_node in pose_nodes {
                graph.delete(pose_node);
            }
            for j in 0..10 {
                let pose_key;
                {
                    let (mut poses, mut vels, mut bodies, mut shapes, _) =
                        graph.get_layer_bundle::<AllLayersMut>();

                    let id = i * 20 + j;
                    let mut pose_node = poses.insert(Pose(id));
                    let mut vel_node = vels.insert(Velocity(id));
                    let mut rb_node = bodies.insert(Body(id));
                    pose_node.connect(&mut vel_node);
                    vel_node.connect(&mut rb_node);
                    pose_node.connect_oneway(
                        &mut shapes
                            .get_mut(everyones_shape)
                            .expect("everyones_shape was deleted"),
                    );

                    pose_key = pose_node.key();
                }
                // delete and replace on every other loop
                if i % 2 == 0 {
                    let pose_len_before = graph.get_layer::<Pose>().components.len();

                    graph.delete(pose_key);
                    let mut poses = graph.get_layer_mut::<Pose>();
                    assert!(
                        poses.get(pose_key).is_none(),
                        "pose_node was deleted so checking it should now give None"
                    );
                    let mut vels = graph.get_layer_mut::<Velocity>();
                    let mut bodies = graph.get_layer_mut::<Body>();

                    let mut pose_node = poses.insert(Pose(100));
                    let mut vel_node = vels.insert(Velocity(100));
                    pose_node.connect(&mut vel_node);
                    if i % 4 == 0 {
                        let mut body_node = bodies.insert(Body(100));
                        body_node.connect(&mut pose_node);
                    }

                    let pose_len_after = poses.components.len();
                    // reused the slot that was left behind by delete
                    assert_eq!(pose_len_before, pose_len_after);
                }
            }
        }

        let (poses, vels, bodies, shapes, _) = graph.get_layer_bundle::<AllLayers>();
        println!("poses.content: {:?}", &poses.components);
        println!("vels.content: {:?}", &vels.components);
        println!("bodies.content: {:?}", &bodies.components);

        assert_eq!(poses.components.len(), 30);
        assert_eq!(vels.components.len(), 30);
        // everyones_shape was never deleted
        assert_eq!(shapes.components.len(), 1);
    }
}
