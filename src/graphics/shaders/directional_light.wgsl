@group(0) @binding(0)
var position_gbuf: texture_2d<f32>;
@group(0) @binding(1)
var normal_gbuf: texture_2d<f32>;
@group(0) @binding(2)
var albedo_gbuf: texture_2d<f32>;
@group(0) @binding(3)
var samp: sampler;

struct LightUniforms {
    direct_color: vec3<f32>,
    ambient_color: vec3<f32>,
    direction: vec3<f32>,
}
@group(1) @binding(0)
var<uniform> light: LightUniforms;

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

// fragment shader draws the entire screen with a single directional light
@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let albedo = textureSample(albedo_gbuf, samp, in.uv);
    if albedo.x == 0. && albedo.y == 0. && albedo.z == 0. {
        discard;
    }
    let normal = textureSample(normal_gbuf, samp, in.uv).xyz;

    // dot with the negative light direction
    // indicates how opposite to the light the normal is,
    // and hence the strength of the diffuse light
    let normal_dot_light = -dot(normal, light.direction);

    let diffuse_strength = max(normal_dot_light, 0.);
    let diffuse_light = diffuse_strength * light.direct_color;

    // stylized approximation: ambient light comes from the direction opposite to the main light
    // TODO: instead of hardcoding intensity 0.1 here,
    // give it as part of the ambient color
    let ambient_strength = 0.1 + 0.1 * max(-normal_dot_light, 0.);
    let ambient_light = light.ambient_color * ambient_strength;

    let full_color = vec4<f32>(ambient_light + diffuse_light, 1.) * albedo;

    return full_color;
}

// alternative fragment shader that draws everything black with no light at all
@fragment
fn fs_dark(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let albedo = textureSample(albedo_gbuf, samp, in.uv);
    if albedo.x == 0. && albedo.y == 0. && albedo.z == 0. {
        discard;
    }
    return vec4<f32>(0., 0., 0., 1.);
}

// another alternative fragment shader that copies the albedo directly
@fragment
fn fs_fullbright(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let albedo = textureSample(albedo_gbuf, samp, in.uv);
    if albedo.x == 0. && albedo.y == 0. && albedo.z == 0. {
        discard;
    }
    return vec4<f32>(albedo.xyz, 1.);
}
