use super::graph as g;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Contact(crate::physics::ContactEvent),
}

type Response<Params> = fn(&mut Params, g::TypedNode<EventSink<Params>>, Event);

/// A component that gathers events that occur to the components it's connected to.
/// See `Event` for which connections produce which types of event.
pub struct EventSink<Params> {
    pub(self) events: Vec<Event>,
    pub(self) response: Response<Params>,
}
impl<Params> EventSink<Params> {
    pub fn new(response: Response<Params>) -> Self {
        EventSink {
            events: Vec::new(),
            response,
        }
    }

    pub fn push(&mut self, evt: Event) {
        self.events.push(evt);
    }
}

pub type EventSinkLayer<Params> = g::Layer<EventSink<Params>>;

impl<Params> EventSinkLayer<Params> {
    /// Gathers all events from all sinks and returns a closure that runs them all.
    ///
    /// This kind of indirection is necessary because we want to be able to manipulate sinks themselves
    /// from within their responders.
    /// TODOC: explain with an example
    pub fn flush(&mut self, graph: &g::Graph) -> impl FnOnce(&mut Params) {
        let mut evts_with_responders: Vec<(
            Event,
            g::TypedNode<EventSink<Params>>,
            Response<Params>,
        )> = self
            .iter_mut(graph)
            .flat_map(|(sink, sink_node)| {
                let resp = sink.response;
                sink.events.drain(..).map(move |evt| (evt, sink_node, resp))
            })
            .collect();

        move |params: &mut Params| {
            for (evt, node, resp) in evts_with_responders.drain(..) {
                resp(params, node, evt);
            }
        }
    }
}
