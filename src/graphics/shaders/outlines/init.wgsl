@group(0) @binding(0)
var stencil_tex: texture_multisampled_2d<f32>;

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

// value representing no known closest point
let EMPTY = vec2<f32>(-1.0, -1.0);

// sample the stencil texture to figure out where within the pixel we are for antialiasing
// source: https://bgolus.medium.com/the-quest-for-very-wide-outlines-ba82ed442cd9
@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec2<f32> {
    let uv_f: vec2<f32> = in.uv * vec2<f32>(textureDimensions(stencil_tex));
    let uv_i: vec2<i32> = vec2<i32>(uv_f);

    let sample_count: i32 = textureNumSamples(stencil_tex);
    let sample_count_inv = 1.0 / f32(sample_count);
    
    // gather all samples of the stencil buffer at this pixel
    var stencil_sum = 0i;
    for (var sample_idx: i32 = 0; sample_idx < sample_count; sample_idx = sample_idx + 1) {
	let sample_val = bitcast<i32>(textureLoad(stencil_tex, uv_i, sample_idx).r);
	stencil_sum = stencil_sum + sample_val;
    }
    // nothing on this pixel
    if stencil_sum == 0i {
	return EMPTY;
    }
    // entire pixel covered
    if stencil_sum >= sample_count {
	return in.position.xy;
    }
    let stencil_avg = f32(stencil_sum) / f32(sample_count);

    // pixel only partially covered, check neighboring pixels and offset position
    // in the estimated normal direction of the edge (see the blog post cited above).
    // only taking one sample in each direction; this is very slightly less accurate
    // than averaging all samples but visually indistinguishable

    let left = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x - 1, uv_i.y), 0).r);
    let top_left = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x - 1, uv_i.y - 1), 0).r);
    let top = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x, uv_i.y - 1), 0).r);
    let top_right = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x + 1, uv_i.y - 1), 0).r);
    let right = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x + 1, uv_i.y), 0).r);
    let btm_right = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x + 1, uv_i.y + 1), 0).r);
    let btm = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x, uv_i.y + 1), 0).r);
    let btm_left = bitcast<i32>(textureLoad(stencil_tex, vec2<i32>(uv_i.x - 1, uv_i.y + 1), 0).r);

    // stencil is 1 if solid and 0 if not, and we want to move towards the edge,
    // therefore we want +x when right is 1 and left is 0 and so on.
    // signs in the order that achieves this.
    let offset_dir = vec2<f32>(
	f32(top_right + 2i * right + btm_right - top_left - 2i * left - btm_left),
	f32(btm_right + 2i * btm + btm_left - top_left - 2i * top - top_right),
    );

    // the lower the stencil average on this pixel, the farther from the edge we are
    let offset_amount = 1.0 - stencil_avg;
    return in.position.xy + offset_amount * normalize(offset_dir);
}
