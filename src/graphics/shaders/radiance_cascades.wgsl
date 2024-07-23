struct GlobalParams {
    level_count: u32,
    probe_count_c0: vec2<u32>,
    // spacing between cascade 0 probes in screen pixels
    spacing_c0: f32,
    // length of the radiance interval measured by cascade 0 probes
    // this should be proportional to the diagonal
    // of a square with side spacing_c0
    range_c0: f32,
}
@group(0) @binding(0)
var<uniform> params: GlobalParams;
@group(0) @binding(1)
var cascade_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(2)
var light_tex: texture_2d<f32>;

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
// Note that this function only works for levels above 0,
// level 0 has an offset of (0, 0)
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
    info.linear_spacing = params.spacing_c0 * f32(level_exp2);
    // each range is 4 times larger than the previous,
    // and starts at the previous one's end,
    // hence the start distance is the sum of a geometric sequence
    info.range_start = params.range_c0 * (1. - f32(level_exp4)) / (1. - 4.);
    info.range_length = params.range_c0 * f32(level_exp4);
    info.tex_offset = cascade_offset(level);

    return info;
}

// information about a direction corresponding to one texel in the cascade texture
struct DirectionInfo {
    // coordinates of the tile in the texture corresponding to this direction
    dir_tile: vec2<u32>,
    // linearized index of the direction for computing the ray angle
    dir_idx: u32,
    // some pixels on higher cascade levels may not actually correspond to a direction
    // if the probe count doesn't divide evenly,
    // this allows us to skip work on those pixels
    valid: bool,
    // 2d index of the probe in the probe lattice
    probe_idx: vec2<u32>,
    // position of the probe in pixel space
    probe_pos: vec2<f32>,
    // lower bound of the radiance interval encoded by this cascade
    range_start: f32,
    // distance for rays to travel in pixel space
    range_length: f32,
    // angle between rays in this cascade
    angle_interval: f32,
}

fn direction_info(cascade: CascadeInfo, texel_id: vec2<u32>) -> DirectionInfo {
    var info: DirectionInfo;

    info.dir_tile = texel_id / cascade.probe_count;
    // direction tiles are laid out in a square with side length 2^level
    let tiles_per_row = (1u << cascade.level);
    // check if the pixel is out of this cascade's range
    info.valid = info.dir_tile.x < tiles_per_row && info.dir_tile.y < tiles_per_row;
    info.dir_idx = info.dir_tile.y * tiles_per_row + info.dir_tile.x;
    info.probe_idx = texel_id % cascade.probe_count;
    info.probe_pos = (vec2<f32>(info.probe_idx) + vec2<f32>(0.5)) * cascade.linear_spacing;
    info.range_start = cascade.range_start;
    info.range_length = cascade.range_length;
    info.angle_interval = TAU / f32(cascade.rays_per_probe);

    return info;
}

// information about a ray corresponding to a single pre-averaged raycast
struct Ray {
    start: vec2<f32>,
    dir: vec2<f32>,
    range: f32,
}

// generate one of four rays to cast in a range around the direction defined by `dir`
fn get_ray(dir: DirectionInfo, subray_idx: u32) -> Ray {
    var ray: Ray;

    // hardcoded 4 subrays per direction
    let actual_ray_idx = 4u * dir.dir_idx + subray_idx;
    let ray_angle = (f32(actual_ray_idx) + 0.5) * dir.angle_interval / 4.;
    ray.dir = vec2<f32>(cos(ray_angle), sin(ray_angle));
    ray.start = dir.probe_pos + dir.range_start * ray.dir;
    ray.range = dir.range_length;

    return ray;
}

// raymarch on the depth and emissive textures in uv space
// to find occluders and lights
fn raymarch(ray: Ray) -> vec4<f32> {
    let screen_size = vec2<i32>(textureDimensions(light_tex));

    var t = 0.;
    var ray_pos = ray.start;
    var pixel_pos = vec2<i32>(ray_pos);
    let pixel_dir = vec2<i32>(sign(ray.dir));
    // bounded loop as a failsafe to avoid hanging
    // in case there's a bug that causes the raymarch to stop in place
    for (var loop_idx = 0u; loop_idx < 10000u; loop_idx++) {
        if t > ray.range {
            // out of range, return an alpha value of 0 
            // to indicate that this ray hit nothing and needs to merge with the above level
            return vec4<f32>(0.);
        }

        if pixel_pos.x < 0 || pixel_pos.x >= screen_size.x || pixel_pos.y < 0 || pixel_pos.y >= screen_size.y {
            // left the screen
            // TODO: return radiance from an environment map
            return vec4<f32>(0.);
        }

        // TODO: handle case where we started inside a shadow caster
        // (these should get light and only occlude things behind them)
        let radiance = textureLoad(light_tex, pixel_pos, 0);
        if radiance.a > 0. {
            // hit an occluder or emitter
            return radiance;
        }

        // move to the next pixel intersected by the ray
        let x_threshold = f32(select(pixel_pos.x, pixel_pos.x + 1, pixel_dir.x == 1));
        let y_threshold = f32(select(pixel_pos.y, pixel_pos.y + 1, pixel_dir.y == 1));
        let to_next_x = abs((x_threshold - ray_pos.x) / ray.dir.x);
        let to_next_y = abs((y_threshold - ray_pos.y) / ray.dir.y);
        if to_next_x < to_next_y {
            t += to_next_x;
            ray_pos += to_next_x * ray.dir;
            pixel_pos.x += pixel_dir.x;
        } else {
            t += to_next_y;
            ray_pos += to_next_y * ray.dir;
            pixel_pos.y += pixel_dir.y;
        }
    }

    return vec4<f32>(0.);
}

// find the rays pointing in the `dir_idx` direction
// on the four N+1-probes nearest to the N-probe at `dir.probe_idx`
// and get the interpolated radiance
fn sample_next_cascade(cascade: CascadeInfo, dir: DirectionInfo, subdir_idx: u32) -> vec4<f32> {
    let next_probe_count = cascade.probe_count / 2u;
    // there's an extra row/column around the edges on the previous level,
    // account for that in selecting the probe at the next level
    let next_probe_idx = vec2<u32>(
        select(0u, (dir.probe_idx.x - 1u) / 2u, dir.probe_idx.x != 0u),
        select(0u, (dir.probe_idx.y - 1u) / 2u, dir.probe_idx.y != 0u),
    );
    let next_casc_offset = cascade_offset(cascade.level + 1u);
    let next_tiles_per_row = 1u << (cascade.level + 1u);
    let next_dir_idx = dir.dir_idx * 4u + subdir_idx;
    let next_dir_tile = vec2<u32>(
        next_dir_idx % next_tiles_per_row,
        next_dir_idx / next_tiles_per_row,
    );
    let dir_tile_offset = next_dir_tile * next_probe_count;
    let total_offset = next_casc_offset + dir_tile_offset;

    // storage textures can't be sampled,
    // so we have to do the interpolation manually.
    // an alternative would be to convert this whole shader to a fragment shader
    // but set up a profiler first if you try that
    // because there are tradeoffs that may well overshadow the gains from hardware interpolation

    let tl_pos = total_offset + next_probe_idx;
    let tl = textureLoad(cascade_tex, tl_pos);
    let tr = textureLoad(cascade_tex, tl_pos + vec2<u32>(1u, 0u));
    let bl = textureLoad(cascade_tex, tl_pos + vec2<u32>(0u, 1u));
    let br = textureLoad(cascade_tex, tl_pos + vec2<u32>(1u, 1u));

    var br_weight: vec2<f32>;
    // clamp the weights in case we're on the border
    // to avoid spurious contribution from adjacent direction tiles
    for (var axis = 0u; axis < 2u; axis++) {
        if next_probe_idx[axis] == 0u {
            br_weight[axis] = 1.;
        } else if next_probe_idx[axis] >= next_probe_count[axis] - 1u {
            br_weight[axis] = 0.;
        } else {
            // regular case where every other N-probe 
            // is closer to the N+1-probe to its right
            // and every other to its left
            br_weight[axis] = select(0.75, 0.25, dir.probe_idx[axis] % 2u == 1u);
        }
    }

    return mix(
        mix(tl, tr, br_weight.x),
        mix(bl, br, br_weight.x),
        br_weight.y,
    );
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

    let top_cascade_idx = params.level_count - 1u;
    let top_cascade = cascade_info(top_cascade_idx);
    let top_dir = direction_info(top_cascade, texel_id);

    // some pixels on higher cascade levels may not actually correspond to a probe
    // if the probe count doesn't divide evenly, skip work if we're one of those
    if top_dir.valid {
        // pre-averaging of 4 rays for the top cascade 
        // (this one doesn't merge with rays of higher cascades)
        var ray_avg = vec4<f32>(0.);
        for (var subray_idx = 0u; subray_idx < 4u; subray_idx++) {
            ray_avg += raymarch(get_ray(top_dir, subray_idx));
        }
        ray_avg *= 0.25;

        textureStore(cascade_tex, top_cascade.tex_offset + texel_id, ray_avg);
    }

    // make sure all threads have computed this cascade before moving to the next
    storageBarrier();

    // same process plus merging with the level above for all cascades besides 0

    for (var cascade_idx = top_cascade_idx - 1u; cascade_idx > 0u; cascade_idx--) {
        let cascade = cascade_info(cascade_idx);
        let dir = direction_info(cascade, texel_id);

        if dir.valid {
            var ray_avg = vec4<f32>(0.);
            for (var subray_idx = 0u; subray_idx < 4u; subray_idx++) {
                let ray_radiance = raymarch(get_ray(dir, subray_idx));
                ray_avg += ray_radiance;
                if ray_radiance.a == 0. {
                    // ray didn't hit anything, merge with level above
                    ray_avg += sample_next_cascade(cascade, dir, subray_idx);
                }
            }
            ray_avg *= 0.25;

            textureStore(cascade_tex, cascade.tex_offset + texel_id, ray_avg);
        }

        storageBarrier();
    }

    // and finally, the 0th cascade which does not do pre-averaging
    // and does merging with the above level.
    // we also don't call cascade_info or dir_info because this cascade has a different layout

    var casc_0: CascadeInfo;
    casc_0.level = 0u;
    casc_0.probe_count = params.probe_count_c0;
    casc_0.rays_per_probe = 4u;
    casc_0.linear_spacing = params.spacing_c0;
    casc_0.range_start = 0.;
    casc_0.range_length = params.range_c0;
    casc_0.tex_offset = vec2<u32>(0u);

    var dir_0: DirectionInfo;
    dir_0.dir_idx = 0u;
    // for this one we store the values in position-major order instead of direction-major
    // so that all four directions can be accessed with a single textureSample call.
    // instead of texel_id directly corresponding to a pixel in the texture
    // it corresponds to a 2x2 block of pixels that are part of one probe
    dir_0.probe_idx = texel_id;
    dir_0.probe_pos = (vec2<f32>(dir_0.probe_idx) + vec2<f32>(0.5)) * casc_0.linear_spacing;
    dir_0.range_start = 0.;
    dir_0.range_length = casc_0.range_length;
    // set the angle interval to the full circle
    // so we can reuse get_ray's subray logic to generate the rays
    dir_0.angle_interval = TAU;

    for (var ray_idx = 0u; ray_idx < 4u; ray_idx++) {
        var ray_radiance = raymarch(get_ray(dir_0, ray_idx));
        if ray_radiance.a == 0. {
            ray_radiance += sample_next_cascade(casc_0, dir_0, ray_idx);
        }

        // store each ray result in a different texel in position-major order
        let probe_offset = 2u * texel_id;
        let target_texel = vec2<u32>(probe_offset.x + ray_idx % 2u, probe_offset.y + ray_idx / 2u);
        textureStore(cascade_tex, target_texel, ray_radiance);
    }
}
