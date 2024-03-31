struct CameraUniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct LightUniforms {
    direct_color: vec3<f32>,
    ambient_color: vec3<f32>,
    direction: vec3<f32>,
}
@group(1) @binding(0)
var<uniform> light: LightUniforms;

@group(2) @binding(0)
var<storage> joint_mats: array<mat4x4<f32>>;

struct MaterialUniforms {
    base_color: vec4<f32>,
}
@group(3) @binding(0)
var<uniform> material: MaterialUniforms;
@group(3) @binding(1)
var t_diffuse: texture_2d<f32>;
@group(3) @binding(2)
var s_diffuse: sampler;
@group(3) @binding(3)
var t_normal: texture_2d<f32>;
@group(3) @binding(4)
var s_normal: sampler;

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

// vertex shader with skinning, joints and weights in a separate vertex buffer
@vertex
fn vs_skinned(
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    // instance variables: position in the joint buffer, model matrix
    @location(2) joint_offset: u32,
    @location(3) model_col0: vec3<f32>,
    @location(4) model_col1: vec3<f32>,
    @location(5) model_col2: vec3<f32>,
    @location(6) model_col3: vec3<f32>,
    // additional vertex data for skinning in a separate buffer
    // (u16 not supported in wgsl, so bit-twiddle joint indices from two u32s)
    @location(7) joints: vec2<u32>,
    @location(8) weights: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let model = mat4x4<f32>(
        vec4<f32>(model_col0, 0.),
        vec4<f32>(model_col1, 0.),
        vec4<f32>(model_col2, 0.),
        vec4<f32>(model_col3, 1.),
    );

    let joint_indices = vec4<u32>(joint_offset) + vec4<u32>(
        joints[0] & 0xFFFFu,
        joints[0] >> 16u,
        joints[1] & 0xFFFFu,
        joints[1] >> 16u
    );

    var pos = vec3<f32>(0.);
    var has_joints = false;
    // hardcoded normal and tangent in the x,y plane,
    // since we don't support general 3D rendering
    let normal = vec3<f32>(0., 0., -1.);
    let tangent = vec3<f32>(1., 0., 0.);
    var norm_skinned = vec3<f32>(0.);
    var tan_skinned = vec3<f32>(0.);

    for (var i = 0; i < 4; i++) {
        let weight = weights[i];
        if weight > 0. {
            has_joints = true;
            let ji = joint_indices[i];
            let joint_mat = joint_mats[ji];
            pos += weight * (joint_mat * vec4<f32>(position, 1.)).xyz;

            let joint_mat_3 = mat3x3<f32>(joint_mat[0].xyz, joint_mat[1].xyz, joint_mat[2].xyz);
            let inv_scaling = mat3_inv_scale_sq(joint_mat_3);
            let weight_scaled = weight * inv_scaling;
            norm_skinned += weight_scaled * (joint_mat_3 * normal);
            tan_skinned += weight_scaled * (joint_mat_3 * tangent);
        }
    }
    // if no joints had any weight, fallback to original values
    if !has_joints {
        pos = position;
        norm_skinned = normal;
        tan_skinned = tangent;
    }

    // transform skinned values with the model matrix
    let pos_model = model * vec4<f32>(pos, 1.);
    let model_3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let inv_scaling = mat3_inv_scale_sq(model_3);
    norm_skinned = inv_scaling * (model_3 * norm_skinned);
    tan_skinned = inv_scaling * (model_3 * tan_skinned);

    out.clip_position = camera.view_proj * pos_model;
    out.world_position = position;
    out.tex_coords = tex_coords;
    out.normal = normalize(norm_skinned);
    out.tangent = normalize(tan_skinned);

    return out;
}

// vertex shader without skinning
@vertex
fn vs_unskinned(
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    // instance variables: position in the joint buffer, model matrix
    @location(2) joint_offset: u32,
    @location(3) model_col0: vec3<f32>,
    @location(4) model_col1: vec3<f32>,
    @location(5) model_col2: vec3<f32>,
    @location(6) model_col3: vec3<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let model = mat4x4<f32>(
        vec4<f32>(model_col0, 0.),
        vec4<f32>(model_col1, 0.),
        vec4<f32>(model_col2, 0.),
        vec4<f32>(model_col3, 1.),
    );

    let normal = vec3<f32>(0., 0., -1.);
    let tangent = vec3<f32>(1., 0., 0.);

    let pos_model = model * vec4<f32>(position, 1.);
    let model_3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let inv_scaling = mat3_inv_scale_sq(model_3);
    let norm_transformed = inv_scaling * (model_3 * normal);
    let tan_transformed = inv_scaling * (model_3 * tangent);

    out.clip_position = camera.view_proj * pos_model;
    out.world_position = position;
    out.tex_coords = tex_coords;
    out.normal = normalize(norm_transformed);
    out.tangent = normalize(tan_transformed);

    return out;
}

struct FragmentOutput {
    @location(0) position: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) albedo: vec4<f32>,
}

@fragment
fn fs_main(
    in: VertexOutput
) -> FragmentOutput {
    var out: FragmentOutput;

    out.position = vec4<f32>(in.world_position, 1.);

    // color texture

    let tex_base_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    out.albedo = material.base_color * tex_base_color;
    // alpha clipping, no blending
    // because we want parts of the same mesh to be able to overlap
    if out.albedo.a < 0.5 {
	discard;
    }

    // normal mapping

    let normal = normalize(in.normal);
    let tangent = normalize(in.tangent);
    let bitangent = cross(tangent, normal);
    let tbn = mat3x3(tangent, bitangent, normal);

    let tex_normal = textureSample(t_normal, s_normal, in.tex_coords).xyz;
    let normal_mapped = tbn * normalize(tex_normal * 2. - 1.);
    out.normal = vec4<f32>(normal_mapped, 0.);

    return out;
}


// TODO: move the shading to another pass
@fragment
fn shade(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let tex_base_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    // alpha clipping, no blending
    // because we want parts of the same mesh to be able to overlap
    if tex_base_color.a < 0.5 {
	discard;
    }

    let normal = normalize(in.normal);
    let tangent = normalize(in.tangent);
    let bitangent = cross(tangent, normal);
    let tbn = mat3x3(tangent, bitangent, normal);

    let tex_normal = textureSample(t_normal, s_normal, in.tex_coords).xyz;
    let normal_mapped = tbn * normalize(tex_normal * 2. - 1.);

    // dot with the negative light direction
    // indicates how opposite to the light the normal is,
    // and hence the strength of the diffuse light
    let normal_dot_light = -dot(normal_mapped, light.direction);

    let diffuse_strength = max(normal_dot_light, 0.);
    let diffuse_light = diffuse_strength * light.direct_color;

    // stylized approximation: ambient light comes from the direction opposite to the main light
    let ambient_strength = 0.1 + 0.1 * max(-normal_dot_light, 0.);
    let ambient_light = light.ambient_color * ambient_strength;

    let full_color = material.base_color * vec4<f32>(ambient_light + diffuse_light, 1.) * tex_base_color;

    return full_color;
}
