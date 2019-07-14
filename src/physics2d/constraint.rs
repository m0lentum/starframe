
pub enum ConstraintType {
    Equal,
    LessThan,
    GreaterThan,
}
// possible degrees of freedom: x, y, rotation

// can be attached to a point or another object

// one object can have many

// potentially many implementations for this: Gauss-Seidel, Jacobi etc.
// will need to study more to make a call on this one
// pub trait ConstraintSolver {
//     fn solve<'a>(items: &'a mut [RigidBodyFilter<'a>], contacts: &'a Collision);
// }
