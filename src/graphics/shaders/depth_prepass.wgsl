// uniforms are the same as mesh.wgsl, minus lights
// (materials are also needed here because of transparent textures).
// would be nice to share this code, consider using `naga_oil` for that

struct CameraUniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

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

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let pos_world = instance.model * vec4<f32>(position, 1.);
    out.clip_position = camera.view_proj * pos_world;
    out.tex_coords = tex_coords;

    return out;
}

// fragment shader doesn't return a color, only writes depth
@fragment
fn fs_main(
    in: VertexOutput
) {
    let alpha = textureSample(t_diffuse, s_diffuse, in.tex_coords).a;
    // only write depth for full-opacity pixels
    if alpha < 0.98 {
        discard;
    }
}
