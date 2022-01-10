struct Uniforms {
    view: mat3x3<f32>;
};

[[group(0), binding(0)]]
var<uniform> unif: Uniforms;

struct VertexOutput {
    [[builtin(position)]] position: vec4<f32>;
};

[[stage(vertex)]]
fn vs_main(
    [[location(0)]] position: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let viewed = unif.view * vec3<f32>(position, 1.0);
    out.position = vec4<f32>(viewed.xy, 0.0, 1.0);

    return out;
}

[[stage(fragment)]]
fn fs_main(
    in: VertexOutput
) -> [[location(0)]] vec2<f32> {
    return in.position.xy;
}
