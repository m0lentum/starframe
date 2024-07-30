@group(0) @binding(0)
var light_tex: texture_2d<f32>;

struct CascadeParams {
    level: u32,
    level_count: u32,
    probe_count: vec2<u32>,
    rays_per_probe: u32,
    linear_spacing: f32,
    range_start: f32,
    range_length: f32,
}
@group(1) @binding(0)
var<uniform> cascade: CascadeParams;
@group(1) @binding(1)
var cascade_src: texture_2d<f32>;
@group(1) @binding(2)
var cascade_dst: texture_storage_2d<rgba8unorm, write>;

const TAU: f32 = 6.283185;

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

fn direction_info(texel_id: vec2<u32>) -> DirectionInfo {
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

struct RayResult {
    value: vec3<f32>,
    // if the ray didn't reach a light,
    // the radiance value is interpreted
    // as occlusion by a translucent material instead
    is_radiance: bool,
}

// raymarch on the depth and emissive textures in uv space
// to find occluders and lights
fn raymarch(ray: Ray) -> RayResult {
    var out: RayResult;

    // each cascade begins raymarching on its corresponding mip level
    // and only accesses lower mips if it finds a light or fully opaque shadow
    let initial_mip_level = i32(cascade.level);
    var mip_level = initial_mip_level;
    var pixel_size = i32(1u << u32(mip_level));
    var screen_size = vec2<i32>(textureDimensions(light_tex)) / pixel_size;

    // multiplicative factor accumulated from translucent materials
    // starts at (1, 1, 1) letting all light through,
    // and gets lower over time
    var occlusion = vec3<f32>(1.);
    var t = 0.;
    var ray_pos = ray.start;
    var pixel_pos = vec2<i32>(ray_pos) / pixel_size;
    let pixel_dir = vec2<i32>(sign(ray.dir));
    // pixel that we were in on the previous level if we've gone down the mip tree
    var upper_pixel = vec2<i32>(-1);
    // bounded loop as a failsafe to avoid hanging
    // in case there's a bug that causes the raymarch to stop in place
    for (var loop_idx = 0u; loop_idx < 10000u; loop_idx++) {
        if pixel_pos.x < 0 || pixel_pos.x >= screen_size.x || pixel_pos.y < 0 || pixel_pos.y >= screen_size.y {
            // left the screen
            // just treat the edge of the screen as a shadow for now,
            // TODO: return radiance from an environment map
            out.value = vec3<f32>(0.);
            out.is_radiance = true;
            return out;
        }

        if t > ray.range {
            // out of range, return the amount of light that got occluded
            // to merge with the above level
            out.value = occlusion;
            out.is_radiance = false;
            return out;
        }

        let rad = textureLoad(light_tex, pixel_pos, mip_level);
        if rad.a == 1. {
            // pixel contains an emitter or occluder,
            // traverse down the tree to the final mip level 
            // to alleviate flickering when small lights are moving
            if mip_level == 0 {
                // remove absorbed wavelengths
                out.value = saturate(occlusion) * rad.rgb;
                out.is_radiance = true;
                return out;
            } else {
                // we're in a pixel where one the pixels on a lower mip level
                // is an occluder or emitter; traverse to the next mip level to find it
                mip_level -= 1;
                upper_pixel = pixel_pos;
                // find which quadrant of the pixel we're in to get the right lower-mip pixel
                let ray_in_pixel = (ray_pos - vec2<f32>(pixel_pos * pixel_size)) / f32(pixel_size);
                pixel_pos *= 2;
                if ray_in_pixel.x > 0.5 {
                    pixel_pos.x += 1;
                }
                if ray_in_pixel.y > 0.5 {
                    pixel_pos.y += 1;
                }
                pixel_size /= 2;
                screen_size *= 2;
                continue;
            }
        }

        // traverse back up the tree if we've gone down to look for a light pixel and missed
        let curr_upper = pixel_pos / 2;
        if mip_level < initial_mip_level && (curr_upper.x != upper_pixel.x || curr_upper.y != upper_pixel.y) {
            mip_level += 1;
            upper_pixel /= 2;
            pixel_pos /= 2;
            pixel_size *= 2;
            screen_size /= 2;
            continue;
        }

        if rad.a > 0. {
            // volumetric material, accumulate occlusion
            // TODO: make the amount depend on the worldspace size of a pixel
            // and the exact ray increment taken instead of just a flat value per pixel
            occlusion -= (vec3<f32>(1.) - rad.rgb) * rad.a * f32(pixel_size);
            if occlusion.r <= 0. && occlusion.g <= 0. && occlusion.b <= 0. {
                out.value = vec3<f32>(0.);
                out.is_radiance = true;
                return out;
            }
        }

        // move to the next pixel intersected by the ray
        let x_threshold = f32(select(pixel_pos.x, pixel_pos.x + 1, pixel_dir.x == 1) * pixel_size);
        let y_threshold = f32(select(pixel_pos.y, pixel_pos.y + 1, pixel_dir.y == 1) * pixel_size);
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

    return out;
}

// find the rays pointing in the `dir_idx` direction
// on the four N+1-probes nearest to the N-probe at `dir.probe_idx`
// and get the interpolated radiance
fn sample_next_cascade(dir: DirectionInfo, subdir_idx: u32) -> vec4<f32> {
    let next_probe_count = cascade.probe_count / 2u;
    // there's an extra row/column around the edges on the previous level,
    // account for that in selecting the probe at the next level
    let next_probe_idx = vec2<u32>(
        select(0u, (dir.probe_idx.x - 1u) / 2u, dir.probe_idx.x != 0u),
        select(0u, (dir.probe_idx.y - 1u) / 2u, dir.probe_idx.y != 0u),
    );
    let next_tiles_per_row = 1u << (cascade.level + 1u);
    let next_dir_idx = dir.dir_idx * 4u + subdir_idx;
    let next_dir_tile = vec2<u32>(
        next_dir_idx % next_tiles_per_row,
        next_dir_idx / next_tiles_per_row,
    );
    let dir_tile_offset = next_dir_tile * next_probe_count;

    // storage textures can't be sampled,
    // so we have to do the interpolation manually.
    // an alternative would be to convert this whole shader to a fragment shader
    // but set up a profiler first if you try that
    // because there are tradeoffs that may well overshadow the gains from hardware interpolation

    let tl_pos = dir_tile_offset + next_probe_idx;
    let tl = textureLoad(cascade_src, tl_pos, 0);
    let tr = textureLoad(cascade_src, tl_pos + vec2<u32>(1u, 0u), 0);
    let bl = textureLoad(cascade_src, tl_pos + vec2<u32>(0u, 1u), 0);
    let br = textureLoad(cascade_src, tl_pos + vec2<u32>(1u, 1u), 0);

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
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let texel_id = global_id.xy;
    // pre-averaged cascades (all but the 0th one) all have the same size:
    // cascade 1 stores the same number of directions as cascade 0 (i.e. 4)
    // and a quarter the probe count, hence its size is exactly the probe count.
    // and subsequent cascades quarter the probe count and quadruple the direction count
    let cascade_tex_size = textureDimensions(cascade_src);
    // in case the cascade size isn't a multiple of workgroup size,
    // don't do work out of bounds
    if texel_id.x >= cascade_tex_size.x || texel_id.y >= cascade_tex_size.y {
        return;
    }

    let dir = direction_info(texel_id);
    // pixel doesn't actually correspond to any direction
    // due to probe counts rounding down when they don't divide evenly
    if !dir.valid {
        return;
    }

    // cascades above 0 are pre-averaged,
    // 0 isn't because we need directional information for final lighting
    if cascade.level > 0u {
        var ray_avg = vec3<f32>(0.);
        for (var subray_idx = 0u; subray_idx < 4u; subray_idx++) {
            var rad = raymarch(get_ray(dir, subray_idx));
            if !rad.is_radiance && cascade.level < cascade.level_count - 1u {
                // ray didn't hit anything or only hit volumetric occlusion,
                // merge with level above (but only if there is a level above)
                let next = sample_next_cascade(dir, subray_idx);
                rad.value = rad.value * next.rgb;
            }
            ray_avg += rad.value;
        }
        ray_avg *= 0.25;

        textureStore(cascade_dst, texel_id, vec4<f32>(ray_avg, 1.));
    } else {
        // cascade 0 has different requirements,
        // construct a DirectionInfo manually so that we can use the same abstractions
        var dir: DirectionInfo;
        dir.dir_idx = 0u;
        // for this one we store the values in position-major order instead of direction-major
        // so that all four directions can be accessed with a single textureSample call.
        // instead of texel_id directly corresponding to a pixel in the texture
        // it corresponds to a 2x2 block of pixels that are part of one probe
        dir.probe_idx = texel_id;
        dir.probe_pos = (vec2<f32>(dir.probe_idx) + vec2<f32>(0.5)) * cascade.linear_spacing;
        dir.range_start = 0.;
        dir.range_length = cascade.range_length;
        // set the angle interval to the full circle
        // so we can reuse get_ray's subray logic to generate the rays
        dir.angle_interval = TAU;

        for (var ray_idx = 0u; ray_idx < 4u; ray_idx++) {
            var rad = raymarch(get_ray(dir, ray_idx));
            if !rad.is_radiance {
                let next = sample_next_cascade(dir, ray_idx);
                rad.value = rad.value * next.rgb;
            }

            // store each ray result in a different texel in position-major order
            let probe_offset = 2u * texel_id;
            let target_texel = vec2<u32>(probe_offset.x + ray_idx % 2u, probe_offset.y + ray_idx / 2u);
            textureStore(cascade_dst, target_texel, vec4<f32>(rad.value, 1.));
        }
    }
}
