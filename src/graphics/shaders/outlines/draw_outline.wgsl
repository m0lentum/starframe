struct Uniforms {
    thickness: u32;
    l1_weight: f32;
    l2_weight: f32;
    inf_weight: f32;
};

[[group(0), binding(0)]]
var<uniform> unif: Uniforms;

[[group(1), binding(0)]]
var gbuf_tex: texture_2d<f32>;

struct VertexOutput {
    [[builtin(position)]] position: vec4<f32>;
    [[location(0)]] uv: vec2<f32>;
};

[[stage(vertex)]]
fn vs_main(
    [[builtin(vertex_index)]] vert_idx: u32,
) -> VertexOutput {
    var out: VertexOutput;

    out.uv = vec2<f32>(f32((vert_idx << 1u) & 2u), f32(vert_idx & 2u));
    out.position = vec4<f32>(out.uv.x * 2.0 - 1.0, out.uv.y * -2.0 + 1.0, 0.0, 1.0);

    return out;
}

[[stage(fragment)]]
fn fs_main(
    in: VertexOutput,
) -> [[location(0)]] vec4<f32> {
    let uv_f: vec2<f32> = in.uv * vec2<f32>(textureDimensions(gbuf_tex));
    let uv_i: vec2<i32> = vec2<i32>(uv_f);
    let closest = textureLoad(gbuf_tex, uv_i, 0);

    let dist = closest.xy - uv_f;
    let dist_norm = unif.l1_weight * (abs(dist.x) + abs(dist.y))
        + unif.l2_weight * length(dist)
        + unif.inf_weight * max(abs(dist.x), abs(dist.y));

    // bump threshold a bit to avoid rounding artifacts on straight edges
    let threshold = f32(unif.thickness) + 0.25;
    if (closest.x < 0.0 || dist_norm > threshold) {
        discard;
    }

    // TODO customizable color (possibly distance curve or texture)
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}