//
// uniforms
//

// camera

struct CameraUniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

// lights

const MAX_LIGHTS: u32 = 1024u;
const TILE_SIZE: u32 = 16u;

struct DirectionalLight {
    color: vec3<f32>,
    direction: vec3<f32>,
}

struct MainLights {
    ambient_color: vec3<f32>,
    dir_light_count: u32,
    dir_lights: array<DirectionalLight>,
}

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
@group(1) @binding(2)
var<storage> main_lights: MainLights;

// material

struct MaterialUniforms {
    base_color: vec4<f32>,
}

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;
@group(2) @binding(1)
var t_diffuse: texture_2d<f32>;
@group(2) @binding(2)
var s_diffuse: sampler;
@group(2) @binding(3)
var t_normal: texture_2d<f32>;
@group(2) @binding(4)
var s_normal: sampler;

// instance

struct InstanceUniforms {
    model: mat4x4<f32>,
}

@group(3) @binding(0)
var<uniform> instance: InstanceUniforms;

//
// vertex shader
//

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec3<f32>,
};

// counteract the scaling effect of a transformation
// in order to transform normals correctly
fn mat3_inv_scale_sq(m: mat3x3<f32>) -> vec3<f32> {
    return vec3<f32>(
        1.0 / dot(m[0].xyz, m[0].xyz),
        1.0 / dot(m[1].xyz, m[1].xyz),
        1.0 / dot(m[2].xyz, m[2].xyz)
    );
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec3<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let model = instance.model;

    let pos_world = model * vec4<f32>(position, 1.);
    let model_3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let inv_scaling = mat3_inv_scale_sq(model_3);
    let norm_transformed = inv_scaling * (model_3 * normal);
    let tan_transformed = inv_scaling * (model_3 * tangent);

    out.clip_position = camera.view_proj * pos_world;
    out.world_position = pos_world.xyz;
    out.tex_coords = tex_coords;
    out.normal = normalize(norm_transformed);
    out.tangent = normalize(tan_transformed);

    return out;
}

//
// fragment shader
//

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    // color texture and normal map

    let diffuse_color = material.base_color * textureSample(t_diffuse, s_diffuse, in.tex_coords);

    let bitangent = cross(in.tangent, in.normal);
    let tbn = mat3x3(in.tangent, bitangent, in.normal);

    let tex_normal = textureSample(t_normal, s_normal, in.tex_coords).xyz;
    let normal = tbn * normalize(tex_normal * 2. - 1.);

    // directional lights

    var dir_light_total = vec3<f32>(0.);
    for (var li = 0u; li < main_lights.dir_light_count; li++) {
        let dir_light = main_lights.dir_lights[li];
        // dot with the negative light direction
        // indicates how opposite to the light the normal is,
        // and hence the strength of the diffuse light
        let normal_dot_light = -dot(normal, dir_light.direction.xyz);
        let diffuse_strength = max(normal_dot_light, 0.);
        dir_light_total += diffuse_strength * dir_light.color;
    }

    // point lights

    // get the pixel we're in from the screenspace coordinates
    // and select the right tile based on it
    let pixel = vec2<u32>(in.clip_position.xy);
    let tile_id = pixel / TILE_SIZE;
    let bin_idx = tile_id.y * point_lights.tiles_x + tile_id.x;
    let bin_start = bin_idx * MAX_LIGHTS;

    var point_light_total = vec3<f32>(0., 0., 0.);
    for (var bi = 0u; bi < point_lights.count; bi++) {
        let li = light_bins[bin_start + bi];
        if li == -1 {
            break;
        }
        let light = point_lights.lights[li];

        let from_light = in.world_position - light.position;
        let dist = length(from_light);
        let attenuation = 1. / (1. + dist * light.attn_linear + dist * dist * light.attn_quadratic);

        let light_dir = from_light / dist;
        let normal_dot_light = -dot(normal, light_dir);

        let light_strength = attenuation * max(normal_dot_light, 0.);
        let light_contrib = light_strength * light.color;
        point_light_total += light_contrib;
    }

    let light_total = main_lights.ambient_color + dir_light_total + point_light_total;
    let final_color = vec4<f32>(light_total, 1.) * diffuse_color;
    return final_color;
}

