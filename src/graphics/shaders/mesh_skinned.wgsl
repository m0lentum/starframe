@group(0)
@binding(0)
var<storage> joint_mats: array<mat4x4<f32>>;

struct Uniforms {
    model_view: mat3x3<f32>,
    joint_offset: u32,
};

@group(1)
@binding(0)
var<uniform> unif: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    // u16 not supported in wgsl, so bit-twiddle from two u32s
    @location(2) joints: vec2<u32>,
    @location(3) weights: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    out.color = color;

    // skin

    let j0 = unif.joint_offset + (joints[0] & 0xFFFFu);
    let j1 = unif.joint_offset + (joints[0] >> 16u);
    let j2 = unif.joint_offset + (joints[1] & 0xFFFFu);
    let j3 = unif.joint_offset + (joints[1] >> 16u);
    let skin_mat: mat4x4<f32> =
	weights.x * joint_mats[j0]
	+ weights.y * joint_mats[j1]
	+ weights.z * joint_mats[j2]
	+ weights.w * joint_mats[j3];
    let skinned: vec4<f32> = skin_mat * vec4<f32>(position, 1.0);
    
    // view transform

    // only view-transform the xy part of the position
    let viewed = unif.model_view * vec3<f32>(skinned.xy, 1.0);
    // TODO: think about how best to scale depth so we stay within [0, 1].
    // maybe just include a z component in the view matrix
    out.position = vec4<f32>(viewed.xy, 0.5 - 0.001 * skinned.z, 1.0);

    return out;
}

@fragment
fn fs_main(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return in.color;
}
