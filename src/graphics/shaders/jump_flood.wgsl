// generate a distance field texture
// encoding the integer Manhattan distance
// to the closest nonzero pixel in the light emitter/occluder map

struct IterParams {
    idx: u32,
}
@group(0) @binding(0)
var<uniform> iter_params: IterParams;
@group(0) @binding(1)
var light_tex: texture_2d<f32>;
@group(0) @binding(2)
var jfa_src: texture_2d<u32>;
@group(0) @binding(3)
var jfa_dst: texture_storage_2d<r32uint, write>;

const EMPTY_VAL: u32 = 0xffffffffu;

// make sure this matches the JFA_PASS_COUNT in gi.rs
const PASS_COUNT: u32 = 8u;
const TILE_SIZE: u32 = 16u;

// we don't need the full 32 bit precision
// so pack the 2d pixel positions into single u32s
fn pack(pixel: vec2<u32>) -> u32 {
    return (pixel.x << 16u) + (pixel.y & 0xffffu);
}

fn unpack(val: u32) -> vec2<u32> {
    return vec2<u32>(val >> 16u, val & 0xffffu);
}

// generate initial closest point values 
// for pixels with distance 0 and leave everything else empty
@compute
@workgroup_size(TILE_SIZE, TILE_SIZE)
fn init(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let texel_id = global_id.xy;
    let tex_size = textureDimensions(jfa_src);
    if texel_id.x >= tex_size.x || texel_id.y >= tex_size.y {
        return;
    }

    let light_val = textureLoad(light_tex, texel_id, 0);
    let jfa_val = select(pack(texel_id), EMPTY_VAL, light_val.a == 0.);
    textureStore(jfa_dst, texel_id, vec4<u32>(jfa_val));
}

// run one iteration of the jump flood algorithm
// storing closest of 9 points spaced 2^iter_idx pixels apart
@compute
@workgroup_size(TILE_SIZE, TILE_SIZE)
fn iter(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let texel_id = vec2<i32>(global_id.xy);
    let tex_size = vec2<i32>(textureDimensions(jfa_src));
    if texel_id.x >= tex_size.x || texel_id.y >= tex_size.y {
        return;
    }

    let jump = i32(1u << iter_params.idx);
    var sample_points = array(
        vec2<i32>(-jump, jump), vec2<i32>(0, jump), vec2<i32>(jump, jump),
        vec2<i32>(-jump, 0), vec2<i32>(0, 0), vec2<i32>(jump, 0),
        vec2<i32>(-jump, -jump), vec2<i32>(0, -jump), vec2<i32>(jump, -jump),
    );
    var closest_point = vec2<u32>(0u);
    var closest_dist = 0xffffffffu;
    for (var sample_idx = 0u; sample_idx < 9u; sample_idx++) {
        let sample_pt = texel_id + sample_points[sample_idx];
        if sample_pt.x < 0 || sample_pt.x >= tex_size.x || sample_pt.y < 0 || sample_pt.y >= tex_size.y {
            continue;
        }
        let sample = textureLoad(jfa_src, sample_pt, 0).r;
        if sample == EMPTY_VAL {
            continue;
        }
        let sample_p = unpack(sample);
        let dist = vec2<i32>(sample_p) - texel_id;
        let dist_manhattan = u32(abs(dist.x) + abs(dist.y));
        if dist_manhattan < closest_dist {
            closest_dist = dist_manhattan;
            closest_point = sample_p;
        }
    }

    textureStore(jfa_dst, texel_id, vec4<u32>(pack(closest_point)));
}

// write distance values to the final SDF texture
@compute
@workgroup_size(TILE_SIZE, TILE_SIZE)
fn finish(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let texel_id = vec2<i32>(global_id.xy);
    let tex_size = vec2<i32>(textureDimensions(jfa_src));
    if texel_id.x >= tex_size.x || texel_id.y >= tex_size.y {
        return;
    }

    let closest = textureLoad(jfa_src, texel_id, 0).r;
    var dist_val = 0u;
    if closest == EMPTY_VAL {
        // empty value still there,
        // write the maximum safe distance 2^(PASS_COUNT - 1)
        dist_val = 1u << PASS_COUNT;
    } else {
        let dist = texel_id - vec2<i32>(unpack(closest));
        let dist_manhattan = abs(dist.x) + abs(dist.y);
        dist_val = u32(dist_manhattan);
    }

    // no need to pack this one because we're only storing one value
    textureStore(jfa_dst, texel_id, vec4<u32>(dist_val));
}
