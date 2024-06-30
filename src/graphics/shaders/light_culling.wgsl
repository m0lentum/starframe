// compute shader implementing the "gather" style of light culling for forward+.
// credit to https://github.com/bcrusco/Forward-Plus-Renderer/ for the reference implementation

struct CameraUniforms {
    view_proj: mat4x4<f32>,
    // most shaders only use the view_proj matrix,
    // but here we need to work in view space and need some extra information.
    // importantly, view matrix does _not_ include any scaling effects,
    // any zooming is factored into the projection matrix instead
    view: mat4x4<f32>,
    viewport_size_world: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

const MAX_LIGHTS: u32 = 1024u;

struct PointLight {
    position: vec3<f32>,
    color: vec3<f32>,
    radius: f32,
    attn_linear: f32,
    attn_quadratic: f32,
}

struct PointLights {
    count: u32,
    tiles_x: u32,
    tiles_y: u32,
    lights: array<PointLight, MAX_LIGHTS>,
}

@group(1) @binding(0)
var<storage> point_lights: PointLights;
@group(1) @binding(1)
var<storage, read_write> light_bins: array<i32>;

// no sampler needed for depth because every thread
// corresponds exactly to a pixel
@group(2) @binding(0)
var depth_tex: texture_depth_multisampled_2d;

// workgroup variables

// limits of depth within a tile,
// used to cull lights that touch the tile but not the depth range.
// stored as integers because they allow atomic operations
var<workgroup> min_depth_cmp: atomic<u32>;
var<workgroup> max_depth_cmp: atomic<u32>;
var<workgroup> gathered_indices: array<i32, MAX_LIGHTS>;
var<workgroup> gathered_count: atomic<u32>;

const TILE_SIZE: u32 = 16u;
const THREAD_COUNT: u32 = TILE_SIZE * TILE_SIZE;

@compute
@workgroup_size(TILE_SIZE, TILE_SIZE)
fn main(
    @builtin(global_invocation_id) pixel_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(local_invocation_index) local_idx: u32,
    @builtin(workgroup_id) tile_id: vec3<u32>,
) {
    let bin_idx = tile_id.y * point_lights.tiles_x + tile_id.x;

    if local_idx == 0u {
        atomicStore(&max_depth_cmp, 0u);
        atomicStore(&min_depth_cmp, 0xffffffffu);
        atomicStore(&gathered_count, 0u);
    }

    workgroupBarrier();

    // compute limits of depth within the tile

    let depth = textureLoad(depth_tex, pixel_id.xy, 0);
    // we just need the order at this point,
    // we'll convert depth to worldspace z coordinate later
    let depth_int = bitcast<u32>(depth);
    atomicMin(&min_depth_cmp, depth_int);
    atomicMax(&max_depth_cmp, depth_int);

    workgroupBarrier();

    // compute the cuboid region touched by this tile
    // (not a frustum since we're using an orthographic projection)

    let viewport_pixels = textureDimensions(depth_tex);
    let pixel_size = camera.viewport_size_world.x / f32(viewport_pixels.x);
    let tile_dim = f32(TILE_SIZE) * pixel_size;
    let tile_incr = vec2<f32>(tile_dim, -tile_dim);
    // operating in view space
    let view_top_left = vec2<f32>(-0.5, 0.5) * camera.viewport_size_world;

    let tile_top_left = view_top_left + tile_incr * vec2<f32>(tile_id.xy);
    let tile_bottom_right = tile_top_left + tile_incr;
    let min_depth_clip = bitcast<f32>(atomicLoad(&min_depth_cmp));
    let max_depth_clip = bitcast<f32>(atomicLoad(&max_depth_cmp));
    // convert clipspace depth back to view space
    // (this assumes a projection matrix
    // with two nonzero elements on the third row
    // and a view matrix with no scaling)
    let min_depth_view = (min_depth_clip - camera.view_proj[3][2]) / camera.view_proj[2][2];
    let max_depth_view = (max_depth_clip - camera.view_proj[3][2]) / camera.view_proj[2][2];

    // up to four lights handled per thread because we have 256 threads
    // and a maximum of 1024 lights
    let last_loop_idx = (point_lights.count - 1u) / THREAD_COUNT;
    for (var loop_idx = 0u; loop_idx <= last_loop_idx; loop_idx++) {
        let light_idx = loop_idx * THREAD_COUNT + local_idx;
        if light_idx >= point_lights.count {
            break;
        }

        let light = point_lights.lights[light_idx];
        let pos = (camera.view * vec4<f32>(light.position, 1.)).xy;
        let closest_point_in_tile = vec2<f32>(
            clamp(pos.x, tile_top_left.x, tile_bottom_right.x),
            clamp(pos.y, tile_bottom_right.y, tile_top_left.y),
        );
        let dist_from_closest = closest_point_in_tile - pos;
        if dot(dist_from_closest, dist_from_closest) < light.radius * light.radius {
            let next_idx = atomicAdd(&gathered_count, 1u);
            gathered_indices[next_idx] = i32(light_idx);
        }
    }

    workgroupBarrier();

    // one thread writes to the global output buffer

    if local_idx == 0u {
        let start = bin_idx * MAX_LIGHTS;
        for (var i = 0u; i < gathered_count; i++) {
            light_bins[start + i] = gathered_indices[i];
        }
        // mark the final light unless every light in the world touched this tile
        if gathered_count < MAX_LIGHTS {
            light_bins[start + gathered_count] = -1;
        }
    }
}
