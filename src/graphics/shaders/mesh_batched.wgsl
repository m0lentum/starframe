struct Uniforms {
    view: mat3x3<f32>,
};

@group(0)
@binding(0)
var<uniform> unif: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    out.color = color;

    let viewed = unif.view * vec3<f32>(position, 1.0);
    out.position = vec4<f32>(viewed.xy, 0.0, 1.0);

    return out;
}

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return in.color;
}
