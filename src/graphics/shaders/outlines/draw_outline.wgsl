struct Uniforms {
    thickness: u32,
    l1_weight: f32,
    l2_weight: f32,
    inf_weight: f32,
    color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> unif: Uniforms;

@group(1) @binding(0)
var result_tex: texture_2d<f32>;

@group(2) @binding(0)
var last_frame_tex: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

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
) -> @location(0) vec4<f32> {
    let uv_f: vec2<f32> = in.uv * vec2<f32>(textureDimensions(result_tex));
    let uv_i: vec2<i32> = vec2<i32>(uv_f);
    let closest = textureLoad(result_tex, uv_i, 0).xy;
    // out of computed range, so definitely out of the outline as well
    if (closest.x < 0.0) {
        discard;
    }

    let dist = closest - uv_f;
    let dist_norm = unif.l1_weight * (abs(dist.x) + abs(dist.y))
        + unif.l2_weight * length(dist)
        + unif.inf_weight * max(abs(dist.x), abs(dist.y));

    // antialias by changing alpha when within a pixel of the line's "true" edge
    let to_edge = f32(unif.thickness) - dist_norm;
    var alpha = clamp(to_edge, 0.0, 1.0);

    // temporal smoothing by comparing to last frame and averaging alpha.
    // this helps a bit with high-frequency jitter
    // resulting from thin moving triangles.

    let closest_prev = textureLoad(last_frame_tex, uv_i, 0).xy;
    // only do it if the movement was at most one diagonal pixel
    // so it blends with the antialiasing of the edge
    // instead of causing noticeably wide fades
    if (distance(closest, closest_prev) < 1.5) {
	let dist_prev = closest_prev - uv_f;
	let dist_norm_prev = unif.l1_weight * (abs(dist_prev.x) + abs(dist_prev.y))
	    + unif.l2_weight * length(dist_prev)
	    + unif.inf_weight * max(abs(dist_prev.x), abs(dist_prev.y));
	let to_edge_prev = f32(unif.thickness) - dist_norm_prev;
	let alpha_prev = clamp(to_edge_prev, 0.0, 1.0);
	alpha = (alpha + alpha_prev) / 2.0;
    }

    return vec4<f32>(unif.color.xyz, unif.color.a * alpha);
}
