use super::gameloop::LoopState;

pub trait GameState<D> {
    fn update(&mut self, data: &mut D, dt: f32) -> StateOp<D>;
    fn render(&mut self, data: &mut D);
}

pub enum StateOp<D> {
    Stay,
    Push(Box<dyn GameState<D>>),
    Swap(Box<dyn GameState<D>>),
    Pop,
    Destroy,
}

pub struct StateMachine<D> {
    shared_data: D,
    state_stack: Vec<Box<dyn GameState<D>>>,
}

impl<D> StateMachine<D> {
    pub fn new(shared_data: D, initial_state: Box<dyn GameState<D>>) -> Self {
        StateMachine {
            shared_data,
            state_stack: vec![initial_state],
        }
    }

    pub fn update(&mut self, dt: f32) -> LoopState {
        let state = match self.state_stack.last_mut() {
            Some(s) => s,
            None => return LoopState::End,
        };
        match state.update(&mut self.shared_data, dt) {
            StateOp::Stay => (),
            StateOp::Push(next_state) => self.state_stack.push(next_state),
            StateOp::Swap(next_state) => {
                self.state_stack.pop();
                self.state_stack.push(next_state);
            }
            StateOp::Pop => {
                self.state_stack.pop();
            }
            StateOp::Destroy => return LoopState::End,
        }
        LoopState::Continue
    }

    pub fn render(&mut self) {
        let state = match self.state_stack.last_mut() {
            Some(s) => s,
            None => return,
        };
        state.render(&mut self.shared_data);
    }
}
