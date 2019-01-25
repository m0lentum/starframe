use graphics::Graphics;
use piston::input::Event;

pub trait GameState<D, G: Graphics> {
    fn on_event(&mut self, data: &mut D, evt: &Event) -> Option<Box<dyn GameState<D, G>>>;
}

pub struct Game<D, G: Graphics> {
    data: D,
    active_state: Box<dyn GameState<D, G>>,
}

impl<D, G: Graphics> Game<D, G> {
    pub fn new(data: D, initial_state: Box<dyn GameState<D, G>>) -> Self {
        Game {
            data,
            active_state: initial_state,
        }
    }

    pub fn on_event(&mut self, evt: &Event) {
        if let Some(new_state) = self.active_state.on_event(&mut self.data, evt) {
            self.active_state = new_state;
        }
    }
}
