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

struct CascadeRenderParams {
    probe_spacing: f32,
}
@group(1) @binding(0)
var<uniform> light_params: CascadeRenderParams;
@group(1) @binding(1)
var cascade_tex: texture_2d<f32>;
@group(1) @binding(2)
var cascade_samp: sampler;

// material

struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive_color: vec4<f32>,
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
    // get the necessary parameters

    let diffuse_color = material.base_color * textureSample(t_diffuse, s_diffuse, in.tex_coords);

    let bitangent = cross(in.tangent, in.normal);
    let tbn = mat3x3(in.tangent, bitangent, in.normal);

    let tex_normal = textureSample(t_normal, s_normal, in.tex_coords).xyz;
    let normal = tbn * normalize(tex_normal * 2. - 1.);

    // look up the nearest radiance probe and compute lighting based on it

    let nearest_probe = round(in.clip_position.xy / light_params.probe_spacing);
    let probe_center = 2. * nearest_probe + vec2<f32>(1.);
    // map the (x,y) part of the normal to a square,
    // then use its position in the square for bilinear interpolation
    // to get the value corresponding to that direction in the probe
    let normal_abs = abs(normal.xy);
    let normal_sign = vec2<f32>(sign(normal.xy));
    let normal_on_square = select(
        vec2<f32>(1., normal_abs.y / normal_abs.x) * normal_sign,
        vec2<f32>(normal_abs.x / normal_abs.y, 1.) * normal_sign,
        normal_abs.x > normal_abs.y,
    );
    let normal_in_square = normal_on_square * length(normal.xy);

    let sample_pixel = probe_center + 0.5 * normal_in_square;
    let sample_uv = sample_pixel / vec2<f32>(textureDimensions(cascade_tex));
    let light_val = textureSample(cascade_tex, cascade_samp, sample_uv);

    let final_color = vec4<f32>(light_val.rgb, 1.) * diffuse_color;
    return final_color;
}

