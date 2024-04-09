struct CameraUniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct MaterialUniforms {
    base_color: vec4<f32>,
}
@group(1) @binding(0)
var<uniform> material: MaterialUniforms;
@group(1) @binding(1)
var t_diffuse: texture_2d<f32>;
@group(1) @binding(2)
var s_diffuse: sampler;
@group(1) @binding(3)
var t_normal: texture_2d<f32>;
@group(1) @binding(4)
var s_normal: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    // position of the individual vertex in the cap geometry
    @location(0) pos_local: vec3<f32>,
    // start and end points of the segment from the instance buffer
    @location(1) start_point: vec4<f32>,
    @location(2) end_point: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    // most of this is copied from line_segment.wgsl,
    // a little bit more context in its comments
    let x_basis = end_point.xy - start_point.xy;
    let y_basis = normalize(vec2<f32>(-x_basis.y, x_basis.x));

    let width = mix(start_point.w, end_point.w, pos_local.x);
    let z_coord = mix(start_point.z, end_point.z, pos_local.x);

    let basis_mat = mat2x2<f32>(x_basis, width * y_basis);
    let pos_world = start_point.xy + basis_mat * pos_local.xy;

    out.clip_position = camera.view_proj * vec4<f32>(pos_world, z_coord, 1.);

    out.uv = vec2<f32>(2. * pos_local.x, pos_local.y + 0.5);

    return out;
}

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.uv);
    return material.base_color * tex_color;
}
