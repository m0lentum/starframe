use glium::{
    backend::Facade,
    program::{Program, ProgramCreationError},
};

pub struct Shaders {
    pub ortho_2d: Program,
}

impl Shaders {
    pub fn compile<F: Facade + ?Sized>(facade: &F) -> Result<Self, ProgramCreationError> {
        let ortho_2d = Program::from_source(
            facade,
            include_str!("ortho_2d.vert"),
            include_str!("ortho_2d.frag"),
            None,
        )?;

        Ok(Shaders { ortho_2d })
    }
}
