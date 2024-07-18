@group(0) @binding(0)
var cascade_tex: texture_storage_2d<f32, read_write>;
// sampler with bilinear interpolation for merging cascade levels
@group(0) @binding(1)
var cascade_samp: sampler;

@group(1) @binding(0)
var depth_tex: texture_depth_multisampled_2d;
@group(1) @binding(1)
var emissive_tex: texture_2d<f32>;

struct GlobalParams {
    level_count: u32,
    probe_count_c0: vec2<u32>,
}
@group(2) @binding(0)
var<uniform> params: GlobalParams;

// spacing between cascade 0 probes in screen pixels
override SPACING_C0: u32 = 4u;
// length of the radiance interval measured by cascade 0 probes
override RANGE_C0: f32 = 0.5 * sqrt(2. * SPACING_C0 * SPACING_C0);
// distance in pixels to take per raymarch step
// (longer is more performant but may cause light leakage)
override RAYMARCH_STEP_SIZE: f32 = 2.;

const TAU: f32 = 6.283185;

struct CascadeInfo {
    level: u32,
    probe_count: vec2<u32>,
    rays_per_probe: u32,
    linear_spacing: f32,
    range_start: f32,
    range_length: f32,
    tex_offset: vec2<u32>,
}

// get the offset of a cascade's texture area.
// cascade 0 is the size of 2x2 subsequent cascades;
// 0 is laid out on the left side of the texture
// and the rest to its right in stacks of two like this:
// ____________________
// |        | 1  | 3  |
// |   0    |____|____|
// |        | 2  | 4  |
// |________|____|____|
//
fn cascade_offset(level: u32) -> vec2<u32> {
    let casc_size = params.probe_count_c0;
    return vec2<u32>(
        (2u + (level - 1u) / 2u) * casc_size.x,
        ((level - 1u) % 2u) * casc_size.y,
    );
}

// this only works if level > 0,
// cascade 0 is treated as a special case with a different layout
fn cascade_info(level: u32) -> CascadeInfo {
    var info: CascadeInfo;

    info.level = level;
    let level_exp2 = 1u << level;
    let level_exp4 = level_exp2 * level_exp2;
    info.probe_count = params.probe_count_c0 / level_exp2;
    // hardcoded scaling factor of 4x ray directions per cascade,
    // this is important for hardware interpolation.
    // we use 4 rays for cascade 0 and pre-averaged rays for the rest,
    // hence cascade 1 also has 4 rays, cascade 2 has 16 etc.
    info.rays_per_probe = level_exp4;
    info.linear_spacing = SPACING_C0 * level_exp2;
    // each range is 4 times larger than the previous,
    // and starts at the previous one's end,
    // hence the start distance is the sum of a geometric sequence
    info.range_start = RANGE_C0 * (1. - level_exp4) / (1. - 4.);
    info.range_length = RANGE_C0 * level_exp4;
    info.tex_offset = cascade_offset(level);

    return info;
}

// information about a direction corresponding to one texel in the cascade texture
struct DirectionInfo {
    // coordinates of the tile in the texture corresponding to this direction
    dir_tile: vec2<u32>,
    // linearized index of the direction for computing the ray angle
    dir_linear_idx: u32,
    // 2d index of the probe in the probe lattice
    probe_idx: vec2<u32>,
    // position of the probe in pixel space
    probe_pos: vec2<f32>,
    // lower bound of the radiance interval encoded by this cascade
    range_start: f32,
    // number of raymarch steps needed to cover the radiance interval
    step_count: u32,
    // angle between rays in this cascade
    angle_interval: f32,
}

fn direction_info(cascade: CascadeInfo, texel_id: vec2<u32>) -> DirectionInfo {
    var info: DirectionInfo;

    info.dir_tile = texel_id / cascade.probe_count;
    info.dir_linear_idx = info.dir_tile.y * (cascade.rays_per_probe / 2u) + dir_idx_2d.x;
    info.probe_idx = texel_id % cascade.probe_count;
    info.probe_pos = (vec2<f32>(info.probe_idx) + vec2<f32>(0.5)) * cascade.linear_spacing;
    info.range_start = cascade.range_start;
    info.step_count = u32(ceil(cascade.range / RAYMARCH_STEP_SIZE));
    info.angle_interval = TAU / f32(cascade.rays_per_probe);

    return info;
}

// information about a ray corresponding to a single pre-averaged raycast
struct Ray {
    // coordinates are in uv space ([0, 1] x [0, 1])
    // so that we can use a sampler during raycasting instead of doing texture loads manually
    start: vec2<f32>,
    incr: vec2<f32>,
}

// generate one of four rays to cast in a range around the direction defined by `dir`
fn get_ray(dir: DirectionInfo, subray_idx: u32) -> Ray {
    var ray: Ray;

    let screen_size = vec2<f32>(textureDimensions(depth_tex));
    let actual_ray_idx = subray_count * dir.dir_idx + subray_idx;

    // hardcoded 4 subrays per direction
    let ray_angle = (f32(actual_ray_idx) + 0.5) * dir.angle_interval / 4.;
    let ray_dir = vec2<f32>(cos(ray_angle), sin(ray_angle));

    let ray_incr_pixelspace = RAYMARCH_STEP_SIZE * ray_dir;
    ray.incr = ray_incr_pixelspace / screen_size;

    let ray_start_pixelspace = dir.probe_pos + dir.range_start * ray_dir;
    ray.start = ray_start_pixelspace / screen_size;

    return info;
}

// raymarch on the depth and emissive textures in uv space
// to find occluders and lights
fn raymarch(ray: Ray, step_count: u32) -> vec4<f32> {
    var ray_pos = ray.start;
    var prev_depth = textureSample(depth_tex, screen_samp, ray_pos);
    for (var si = 0u; si < step_count; si++) {
        ray_pos += ray.incr;
        if ray_pos.x < 0. || ray_pos.x > 1. || ray_pos.y < 0. || ray_pos.y > 1. {
            // left the screen
            // TODO: return radiance from an environment map
            return vec4<f32>(1.);
        }
        let emissive = textureSample(emissive_tex, screen_samp, ray_pos);
        if emissive.a > 0. {
            // hit a light
            return vec4<f32>(emissive.rgb, 0.);
        }
        let curr_depth = textureSample(depth_tex, screen_samp, ray_pos);
        if curr_depth < prev_depth {
            // hit an occluder
            // TODO: only have this happen in a specific range of depths
            return vec4<f32>(0.);
        }
    }

    // w element of 1 means "nothing hit, merge this with the next cascade"
    return vec4<f32>(0., 0., 0., 1.);
}

// find the rays pointing in the `dir_idx` direction
// on the four N+1-probes nearest to the N-probe at `probe_pos`
// and get the interpolated radiance using hardware filtering
fn sample_next_cascade(cascade: CascadeInfo, dir: DirectionInfo) -> vec4<f32> {
    let next_probe_count = cascade.probe_count / 2u;
    let next_casc_offset = cascade_offset(cascade.level + 1u);
    let dir_area_offset = dir.dir_tile * next_probe_count;
    let total_offset = vec2<f32>(next_casc_offset + dir_area_offset);

    // sampling coordinates for the next cascade level
    // weighted by the relative position of the current probe
    // (remember that the sampling coordinates for a pixel are at (0.5, 0.5)
    // offset from the pixel's integer coordinates,
    // hence the square we're in for e.g. probe (0, 0) is [0.5, 1.5] x [0.5, 1.5].
    // this is why the (0.25, 0.25) vector being added has positive sign)
    var interp_coords = vec2<f32>(dir.probe_idx) * 0.5 + vec2<f32>(0.25);
    // clamp away from this direction tile's texture area edges
    // to prevent spurious contribution from adjacent probes
    interp_coords = clamp(vec2<f32>(0.5), vec2<f32>(next_probe_count) - vec2<f32>(0.5), interp_coords);
    let interp_coords_uv = (next_casc_offset + interp_coords) / vec2<f32>(textureDimensions(cascade_tex));
    return textureSample(cascade_tex, cascade_samp, interp_coords_uv);
}

@compute
@workgroup_size(16, 16)
fn main(
    @builtin(global_invocation_id) local_id: vec3<u32>,
) {
    let texel_id = local_id.xy;
    // pre-averaged cascades (all but the 0th one) all have the same size:
    // cascade 1 stores the same number of directions as cascade 0 (i.e. 4)
    // and a quarter the probe count, hence its size is exactly the probe count.
    // and subsequent cascades quarter the probe count and quadruple the direction count
    let preavg_cascade_tex_size = params.probe_count_c0;
    // in case the cascade size isn't a multiple of workgroup size,
    // don't do work out of bounds
    if texel_id.x >= preavg_cascade_tex_size.x || texel_id.y >= preavg_cascade_tex_size.y {
        return;
    }

    // run through each cascade in order, starting with the top

    let top_cascade_idx = params.level_count - 1;
    let top_cascade = cascade_info(top_cascade_idx);
    let top_dir = direction_info(top_cascade, texel_id);

    // pre-averaging of 4 rays for the top cascade 
    // (this one doesn't merge with rays of higher cascades)
    var ray_avg = vec4<f32>(0.);
    for (var subray_idx = 0u; subray_idx < 4u; subray_idx++) {
        let ray = get_ray(top_dir, subray_idx);
        let ray_radiance = raymarch(ray, dir.step_count);
        ray_avg += vec4<f32>(ray_radiance.rgb, 1. - ray_radiance.a);
    }
    ray_avg *= 0.25;

    textureStore(cascade_tex, top_cascade.tex_offset + texel_id, ray_avg);

    // make sure all threads have computed this cascade before moving to the next
    storageBarrier();

    // same process plus merging with the level above for all cascades besides 0

    for (var cascade_idx = top_cascade_idx - 1u; level > 0u; level--) {
        let cascade = cascade_info(cascade_idx);
        let dir = direction_info(cascade, texel_id);

        var ray_avg = vec4<f32>(0.);
        for (var subray_idx = 0u; subray_idx < 4u; subray_idx++) {
            let ray = get_ray(dir, subray_idx);
            let ray_radiance = raymarch(ray, dir.step_count);
            if ray_radiance.a == 0. {
                // ray hit something opaque, do not merge
                ray_avg += vec4<f32>(ray_radiance.rgb, 1. - ray_radiance.a);
            } else {
                let next_casc = sample_next_cascade(cascade, dir);
                ray_avg += ray_radiance + next_casc;
            }
        }
        ray_avg *= 0.25;

        textureStore(cascade_tex, cascade.tex_offset + texel_id, ray_avg);

        storageBarrier();
    }

    // and finally, the 0th cascade which does not do pre-averaging
    // and does merging with the above level.
    // we also don't call cascade_info or dir_info because this cascade has a different layout

    var dir_0: DirectionInfo;
    dir_0.dir_idx = 0u;
    // for this one we store the values in position-major order instead of direction-major
    // so that all four directions can be accessed with a single textureSample call.
    // instead of texel_id directly corresponding to a pixel in the texture
    // it corresponds to a 2x2 block of pixels that are part of one probe
    dir_0.probe_idx = texel_id;
    dir_0.probe_pos = (vec2<f32>(dir_0.probe_idx) + vec2<f32>(0.5)) * SPACING_C0;
    dir_0.range_start = 0.;
    dir_0.step_count = u32(ceil(RANGE_C0 / RAYMARCH_STEP_SIZE));
    // set the angle interval to the full circle
    // so we can reuse get_ray's subray logic to generate the rays
    dir_0.angle_interval = TAU;

    for (var ray_idx = 0u; ray_idx < 4u; ray_idx++) {
        let ray = get_ray(dir, ray_idx);
        var ray_radiance = raymarch(ray, dir.step_count);
        if ray_radiance.a == 0. {
            ray_radiance = vec4<f32>(ray_radiance.rgb, 1. - ray_radiance.a);
        } else {
            let next_casc = sample_next_cascade(cascade, dir.probe_idx, dir.dir_idx);
            ray_radiance += next_casc;
        }

        // store each ray result in a different texel in position-major order
        let probe_offset = 2u * texel_id;
        let target_texel = vec2<u32>(probe_offset.x + ray_idx % 2u, probe_offset.y + ray_idx / 2u);
        textureStore(cascade_tex, target_texel, ray_radiance);
    }
}
