use piston::event_loop::*;
use piston::input::*;
use piston::window::Window;

pub trait Game {
    fn update(&mut self, args: &UpdateArgs);
    fn render(&mut self, args: &RenderArgs);

    fn run<W: Window>(&mut self, window: &mut W, events: &mut Events) {
        while let Some(evt) = events.next(window) {
            if let Some(r) = evt.render_args() {
                self.render(&r);
            }

            if let Some(u) = evt.update_args() {
                self.update(&u);
            }
        }
    }
}
