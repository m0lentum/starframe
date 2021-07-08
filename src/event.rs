//! Events and utilities for delivering them to game objects and responding to them.
//!
//! TODOC: complete usage example and rationale

use super::graph as g;

/// Events that occur within systems and are gathered in [`EventSink`][self::EventSink]s.
/// Connections between the `EventSink` and other components in the [`Graph`][crate::graph::Graph]
/// determine which events are gathered. See each event type for details.
///
/// This is heavily WIP, as not many things in Starframe exist that can produce events yet.
/// Expect major changes here.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A contact happened in the physics system. Received if connected to a
    /// [`Collider`][crate::physics::Collider].
    Contact(crate::physics::ContactEvent),
}

/// A function that consumes events.
pub type Consumer<Params> = fn(&mut Params, g::Node<EventSink>, Event);
// consumer that gets called if the graph is broken somehow and
// an actual consumer doesn't exist
fn noop_consumer<Params>(_: &mut Params, _: g::Node<EventSink>, _: Event) {}

/// Parts necessary for delivering [`Event`][self::Event]s to objects in the
/// [`Graph`][crate::graph::Graph].
pub struct EventGraph<Params> {
    /// The graph layer for event sinks, which collect events.
    /// Pass this to functions that produce events.
    pub sinks: g::Layer<EventSink>,
    // Consumers stored in a separate Vec so that sinks can be
    // moved around without a type parameter.
    // Every sink has exactly one associated consumer,
    // so the association is made using the sink's index in the Layer.
    consumers: Vec<Consumer<Params>>,
}

impl<Params> EventGraph<Params> {
    /// Create the necessary graph layers for events.
    pub fn new(graph: &mut g::Graph) -> Self {
        EventGraph {
            sinks: graph.create_layer(),
            consumers: Vec::new(),
        }
    }

    /// Add an event sink and a consumer to the graph, returning the node created for the sink.
    ///
    /// Connect the sink node to other nodes in the graph to determine which events it receives.
    pub fn add_sink(
        &mut self,
        consumer: Consumer<Params>,
        graph: &mut g::Graph,
    ) -> g::NodeRef<EventSink> {
        let sink = self.sinks.insert(EventSink::new(), graph);

        use g::UnsafeNode;
        let idx = sink.pos().item_idx;
        if idx >= self.consumers.len() {
            self.consumers.resize(idx + 1, noop_consumer);
        }
        self.consumers[idx] = consumer;

        sink
    }

    /// Gather all events from all sinks and return a closure that runs their consumers.
    ///
    /// This kind of indirection is necessary because we want to be able to manipulate sinks themselves
    /// from within their responders.
    /// TODOC: explain with an example
    pub fn flush(&mut self, graph: &g::Graph) -> impl FnOnce(&mut Params) {
        let sinks = &mut self.sinks;
        let consumers = &self.consumers;
        let mut evts_with_responders: Vec<(Event, g::Node<EventSink>, Consumer<Params>)> = sinks
            .iter_mut(graph)
            .flat_map(|sink_ref| {
                let node = g::NodeRefMut::as_node(&sink_ref, graph);
                use g::UnsafeNode;
                let cmer = consumers[sink_ref.pos().item_idx];
                (sink_ref.item.events.drain(..)).map(move |evt| (evt, node, cmer))
            })
            .collect();

        move |params: &mut Params| {
            for (evt, node, resp) in evts_with_responders.drain(..) {
                resp(params, node, evt);
            }
        }
    }
}

/// A component that gathers events that occur to the components
/// it's connected to in the graph.
pub struct EventSink {
    events: Vec<Event>,
}

impl EventSink {
    pub(self) fn new() -> EventSink {
        EventSink { events: Vec::new() }
    }

    /// Push an event into the sink for later consumption.
    pub fn push(&mut self, evt: Event) {
        self.events.push(evt);
    }
}

impl Default for EventSink {
    fn default() -> Self {
        Self::new()
    }
}
