use glium::{
    backend::Facade,
    program::{Program, ProgramCreationError},
};

type ResultType = Result<Program, ProgramCreationError>;

const ORTHO_2D_VERT: &'static str = include_str!("ortho_2d.vert");
const ORTHO_2D_FRAG: &'static str = include_str!("ortho_2d.frag");

pub fn compile_ortho_2d<F: Facade + ?Sized>(facade: &F) -> ResultType {
    Program::from_source(facade, ORTHO_2D_VERT, ORTHO_2D_FRAG, None)
}
