struct Uniforms {
    step_size: u32,
    l1_weight: f32,
    l2_weight: f32,
    inf_weight: f32,
};

@group(0)
@binding(0)
var<uniform> unif: Uniforms;

@group(1)
@binding(0)
var gbuf_tex: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// vertex shader draws a single full-screen triangle using just vertex indices
// source: https://www.saschawillems.de/blog/2016/08/13/vulkan-tutorial-on-rendering-a-fullscreen-quad-without-buffers/
// (y flipped for wgpu)
@vertex
fn vs_main(
    @builtin(vertex_index) vert_idx: u32,
) -> VertexOutput {
    var out: VertexOutput;

    out.uv = vec2<f32>(f32((vert_idx << 1u) & 2u), f32(vert_idx & 2u));
    out.position = vec4<f32>(out.uv.x * 2.0 - 1.0, out.uv.y * -2.0 + 1.0, 0.0, 1.0);

    return out;
}

@fragment
fn fs_main(
    in: VertexOutput,
) -> @location(0) vec2<f32> {
    // working with integer UVs without a sampler
    let uv_f: vec2<f32> = in.uv * vec2<f32>(textureDimensions(gbuf_tex));
    let uv_i: vec2<i32> = vec2<i32>(uv_f);

    var min_dist: f32 = 1.0 / 0.0; // infinity
    var min_coord = vec2<f32>(-1.0, -1.0);

    for (var u_offset: i32 = -1; u_offset <= 1; u_offset = u_offset + 1) {
        for (var v_offset: i32 = -1; v_offset <= 1; v_offset = v_offset + 1) {
            let offset = vec2<i32>(u_offset, v_offset) * i32(unif.step_size);
            let val_at_offset = textureLoad(gbuf_tex, uv_i + offset, 0);
            
            let dist: vec2<f32> = val_at_offset.xy - uv_f;
            let dist_norm = unif.l1_weight * (abs(dist.x) + abs(dist.y))
                + unif.l2_weight * length(dist)
                + unif.inf_weight * max(abs(dist.x), abs(dist.y));

            if (val_at_offset.x > 0.0 && dist_norm < min_dist) {
                min_dist = dist_norm;
                min_coord = val_at_offset.xy;
            }
        }
    }

    return min_coord;
}
