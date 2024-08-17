struct FrameParams {
    // size of a light texture pixel
    // relative to a 10x10cm square in world space,
    // used to keep absorption by translucent materials
    // consistent with different screen sizes and zoom levels.
    // 10x10cm chosen because it's a scale where a low alpha
    // makes typically sized objects almost fully transparent
    // and high alpha makes them almost fully opaque
    // (TODO: allow user scaling of the size of this reference square)
    pixel_size_10cm: f32,
}

@group(0) @binding(0)
var light_tex: texture_2d<f32>;
@group(0) @binding(1)
var env_map: texture_1d<f32>;
@group(0) @binding(2)
var<uniform> frame: FrameParams;

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
var bilinear_samp: sampler;

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
    // angle from the x axis normalized to the range [0, 1],
    // for sampling the environment map
    angle_normalized: f32,
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
    ray.angle_normalized = ray_angle / TAU;

    return ray;
}

struct RayResult {
    value: vec3<f32>,
    // if the ray didn't reach a light,
    // the radiance value is interpreted
    // as occlusion by a translucent material instead
    is_radiance: bool,
}

// raymarch on the light texture to gather radiance
fn raymarch(ray: Ray) -> RayResult {
    var out: RayResult;

    // each cascade begins raymarching on its corresponding mip level
    // and only accesses lower mips if it finds a light or fully opaque shadow
    let initial_mip_level = i32(cascade.level);
    // how far down the mip chain we need to traverse for sufficient precision
    // depends on the cascade level.
    // best quality is achieved by going all the way to 0,
    // but this is a tradeoff in performance.
    // going halfway down gives good quality,
    // only causing flickering with very small lights
    // (which would flicker anyway, to a lesser extent, even if going to 0)
    let target_mip_level = initial_mip_level / 2;
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
            // left the screen, get light value from the environment map
            let env_light = textureSample(env_map, bilinear_samp, ray.angle_normalized).rgb;
            out.value = saturate(occlusion) * env_light;
            out.is_radiance = true;
            return out;
        }

        if t > ray.range {
            // out of range, return the amount of light that got occluded
            // to merge with the above level
            out.value = saturate(occlusion);
            out.is_radiance = false;
            return out;
        }

        let rad = textureLoad(light_tex, pixel_pos, mip_level);
        if rad.a == 1. {
            // pixel contains an emitter or occluder,
            // traverse down the tree to the final mip level 
            // to alleviate flickering when small lights are moving
            if mip_level == target_mip_level {
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

        // move to the next pixel intersected by the ray
        var t_step: f32;
        let x_threshold = f32(select(pixel_pos.x, pixel_pos.x + 1, pixel_dir.x == 1) * pixel_size);
        let y_threshold = f32(select(pixel_pos.y, pixel_pos.y + 1, pixel_dir.y == 1) * pixel_size);
        let to_next_x = abs((x_threshold - ray_pos.x) / ray.dir.x);
        let to_next_y = abs((y_threshold - ray_pos.y) / ray.dir.y);
        if to_next_x < to_next_y {
            t_step = to_next_x;
            pixel_pos.x += pixel_dir.x;
        } else {
            t_step = to_next_y;
            pixel_pos.y += pixel_dir.y;
        }
        t += t_step;
        ray_pos += t_step * ray.dir;

        if rad.a > 0. {
            // volumetric material, accumulate occlusion
            occlusion -= (vec3<f32>(1.) - rad.rgb) * rad.a * f32(pixel_size) * frame.pixel_size_10cm;
            if occlusion.r <= 0. && occlusion.g <= 0. && occlusion.b <= 0. {
                out.value = vec3<f32>(0.);
                out.is_radiance = true;
                return out;
            }
        }
    }

    return out;
}

// find the rays pointing in the `dir_idx` direction
// on the four N+1-probes nearest to the N-probe at `dir.probe_idx`
// and get the interpolated radiance
fn sample_next_cascade(dir: DirectionInfo, subdir_idx: u32) -> vec4<f32> {
    let next_probe_count = cascade.probe_count / 2u;
    let next_tiles_per_row = 1u << (cascade.level + 1u);
    let next_dir_idx = dir.dir_idx * 4u + subdir_idx;
    let next_dir_tile = vec2<u32>(
        next_dir_idx % next_tiles_per_row,
        next_dir_idx / next_tiles_per_row,
    );
    let dir_tile_offset = vec2<f32>(next_dir_tile * next_probe_count);

    // +0.5 because this position is relative to the top left corner of the screen
    // and probe positioning is offset from the corner by half a space
    var pos_probespace = vec2<f32>(dir.probe_idx) + vec2<f32>(0.5);
    // pixel position within the tile on the next cascade
    // is just this position halved
    pos_probespace *= 0.5;
    // clamp to avoid interpolation getting values from adjacent tiles
    let br_edge = next_probe_count - vec2<u32>(1u);
    pos_probespace = clamp(
        pos_probespace,
        vec2<f32>(0.5),
        vec2<f32>(br_edge) - vec2<f32>(0.5),
    );

    let probe_uv = (dir_tile_offset + pos_probespace) / vec2<f32>(textureDimensions(cascade_src));
    // sampling with hardware interpolation gets all four probes' contributions in one call
    return textureSample(cascade_src, bilinear_samp, probe_uv);
}


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
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let texel_id = vec2<u32>(in.position.xy);

    let dir = direction_info(texel_id);
    // pixel doesn't actually correspond to any direction
    // due to probe counts rounding down when they don't divide evenly
    if !dir.valid {
        return vec4<f32>(0.);
    }

    // this wouldn't work as-is for cascade 0,
    // but that part is done by the mesh shader
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

    return vec4<f32>(ray_avg, 1.);
}
