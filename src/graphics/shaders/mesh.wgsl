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

struct DirectionalLight {
    direct_color: vec3<f32>,
    ambient_color: vec3<f32>,
    // the w component determines whether or not the direction
    // is actually used in shading;
    // if 1 we do normal shading and if 0 we only do a flat ambient light
    direction: vec4<f32>,
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
    lights: array<PointLight, 1024>,
}

@group(1) @binding(0)
var<uniform> dir_light: DirectionalLight;
@group(1) @binding(1)
var<storage> point_lights: PointLights;

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

    let pos_model = model * vec4<f32>(position, 1.);
    let model_3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let inv_scaling = mat3_inv_scale_sq(model_3);
    let norm_transformed = inv_scaling * (model_3 * normal);
    let tan_transformed = inv_scaling * (model_3 * tangent);

    out.clip_position = camera.view_proj * pos_model;
    out.world_position = pos_model.xyz;
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

    // directional light

    var diffuse_light: vec3<f32>;
    var ambient_light: vec3<f32>;
    if dir_light.direction.w == 0. {
        // no direct light, flat ambient lighting only
        diffuse_light = vec3<f32>(0., 0., 0.);
        ambient_light = dir_light.ambient_color;
    } else {
        // dot with the negative light direction
        // indicates how opposite to the light the normal is,
        // and hence the strength of the diffuse light
        let normal_dot_light = -dot(normal, dir_light.direction.xyz);

        let diffuse_strength = max(normal_dot_light, 0.);
        diffuse_light = diffuse_strength * dir_light.direct_color;

        // stylized approximation: ambient light comes from the direction opposite to the main light
        // TODO: instead of hardcoding intensity 0.1 here,
        // give it as part of the ambient color
        let ambient_strength = 0.1 + 0.1 * max(-normal_dot_light, 0.);
        ambient_light = dir_light.ambient_color * ambient_strength;
    }

    // point lights

    var point_light_total = vec3<f32>(0., 0., 0.);
    for (var li: u32 = 0u; li < point_lights.count; li++) {
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

    let full_color = vec4<f32>(ambient_light + diffuse_light + point_light_total, 1.) * diffuse_color;
    return full_color;
}

