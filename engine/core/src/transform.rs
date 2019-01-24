use graphics::math::Matrix2d;

/// Alias for the Similarity2 type from nalgebra, which is what
/// MoleEngine uses for most transformations.
pub type Transform = nalgebra::Similarity2<f32>;

/// Maps a nalgebra::Similarity2 into the less sophisticated graphics::Matrix2d
/// for rendering purposes.
pub fn transform_for_gfx(tr: &Transform) -> Matrix2d {
    // graphics::Matrix2d == [[f32;3];2]
    let h = tr.to_homogeneous().map(|f| f as f64);
    [[h[0], h[3], h[6]], [h[1], h[4], h[7]]]
}
