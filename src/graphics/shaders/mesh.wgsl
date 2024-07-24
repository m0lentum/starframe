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
    probe_count: vec2<u32>,
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

const SQRT_2: f32 = 1.41421562;
const PI: f32 = 3.14159265;
const HALF_PI: f32 = 1.5707963;
const PI_3_2: f32 = 4.71238898;

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

    // remove the half-pixel in clipspace coordinates
    let pixel_pos = in.clip_position.xy - vec2<f32>(0.5);
    // +0.5 because probe positioning is offset from the corner by half a space
    let nearest_probe = min(
        // clamp to probe count to avoid overshoot on the bottom and right edges
        light_params.probe_count - vec2<u32>(1u),
        vec2<u32>(round((pixel_pos / light_params.probe_spacing) + vec2<f32>(0.5))),
    );
    let probe_pixel = 2u * nearest_probe;
    // read the light values in each of the probe's four quadrants
    let br = textureLoad(cascade_tex, probe_pixel, 0);
    let bl = textureLoad(cascade_tex, probe_pixel + vec2<u32>(1u, 0u), 0);
    let tl = textureLoad(cascade_tex, probe_pixel + vec2<u32>(0u, 1u), 0);
    let tr = textureLoad(cascade_tex, probe_pixel + vec2<u32>(1u, 1u), 0);
    var radiances = array(tr, tl, bl, br);
    // each direction on the radiance probe covers a quarter segment of a 2-sphere,
    // and diffuse lighting is an integral over a hemisphere
    // centered on the surface normal.
    // approximate the integral by computing where the hemisphere's bottom plane
    // intersects with the vertical center planes of each probe quadrant
    // (this is hard to explain without being able to draw a picture..)
    var directions = array(
        vec3<f32>(SQRT_2, SQRT_2, 0.),
        vec3<f32>(-SQRT_2, SQRT_2, 0.),
        vec3<f32>(-SQRT_2, -SQRT_2, 0.),
    );
    var radiance = vec3<f32>(0.);
    for (var dir_idx = 0u; dir_idx < 2u; dir_idx++) {
        let dir = directions[dir_idx];
        let dir_normal = directions[dir_idx + 1u];
        let rad = radiances[dir_idx];
        let rad_opposite = radiances[dir_idx + 2u];

        let plane_isect = normalize(cross(dir_normal, normal));
        let angle = acos(plane_isect.z);
        let dir_coverage = select(angle, PI - angle, normal.z > 0.) / PI;
        radiance += dir_coverage * rad.rgb + (1. - dir_coverage) * rad_opposite.rgb;
    }

    return vec4<f32>(radiance, 1.) * diffuse_color;
}

