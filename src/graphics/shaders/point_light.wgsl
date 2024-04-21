@group(0) @binding(0)
var position_gbuf: texture_2d<f32>;
@group(0) @binding(1)
var normal_gbuf: texture_2d<f32>;
@group(0) @binding(2)
var albedo_gbuf: texture_2d<f32>;
@group(0) @binding(3)
var samp: sampler;

struct CameraUniforms {
    view_proj: mat4x4<f32>,
}

@group(1) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) light_position: vec3<f32>,
    @location(1) @interpolate(flat) light_color: vec3<f32>,
    @location(2) @interpolate(flat) attn_linear: f32,
    @location(3) @interpolate(flat) attn_quadratic: f32,
}

// point lights drawn as an instanced set of light volumes
// shaped as 2D circles
@vertex
fn vs_main(
    @location(0) vert_position: vec2<f32>,
    // all the rest are instance variables
    @location(1) light_position: vec3<f32>,
    @location(2) light_color: vec3<f32>,
    @location(3) radius: f32,
    @location(4) attn_linear: f32,
    @location(5) attn_quadratic: f32,
) -> VertexOutput {
    var out: VertexOutput;

    // since our light volumes are flat circles,
    // in order to have them affect things in front of them
    // and not get obscured by the depth buffer,
    // we need to offset the volume in the z direction as well
    let vert_pos_world = light_position + vec3<f32>(radius * vert_position, -radius);
    out.clip_position = camera.view_proj * vec4<f32>(vert_pos_world, 1.);

    out.light_position = light_position;
    out.light_color = light_color;
    out.attn_linear = attn_linear;
    out.attn_quadratic = attn_quadratic;

    return out;
}

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    // position is not actually in clip space in the fragment shader!
    // in.clip_position.xy is the fragment position in the framebuffer in units of pixels
    let uv = in.clip_position.xy / vec2<f32>(textureDimensions(albedo_gbuf));
    let albedo = textureSample(albedo_gbuf, samp, uv);
    if albedo.x == 0. && albedo.y == 0. && albedo.z == 0. {
        discard;
    }
    let position = textureSample(position_gbuf, samp, uv).xyz;
    let normal = textureSample(normal_gbuf, samp, uv).xyz;

    let from_light = position - in.light_position;
    let dist = length(from_light);
    let attenuation = 1. / (1. + dist * in.attn_linear + dist * dist * in.attn_quadratic);

    let light_dir = from_light / dist;
    let normal_dot_light = -dot(normal, light_dir);

    let diffuse_strength = attenuation * max(normal_dot_light, 0.);
    let diffuse_light = diffuse_strength * in.light_color;
    
    // no ambient light for point lights

    let full_color = vec4<f32>(diffuse_light, 1.) * albedo;
    return full_color;
}
