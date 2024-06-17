struct Vertex {
    position: vec3<f32>,
    tex_coords: vec2<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
}

struct VertexJoints {
    joints: vec2<u32>,
    // weights split into two vec2s for alignment purposes
    weights_0: vec2<f32>,
    weights_1: vec2<f32>,
}

@group(0) @binding(0)
var<storage> joint_mats: array<mat4x4<f32>>;
@group(0) @binding(1)
var<storage> vertices: array<Vertex>;
@group(0) @binding(2)
var<storage> joints: array<VertexJoints>;
@group(0) @binding(3)
var<storage, read_write> out_buf: array<Vertex>;

// counteract the scaling effect of a transformation
// in order to transform normals correctly
fn mat3_inv_scale_sq(m: mat3x3<f32>) -> vec3<f32> {
    return vec3<f32>(
        1.0 / dot(m[0].xyz, m[0].xyz),
        1.0 / dot(m[1].xyz, m[1].xyz),
        1.0 / dot(m[2].xyz, m[2].xyz)
    );
}

@compute
@workgroup_size(256)
fn main(@builtin(global_invocation_id) inv_id: vec3<u32>) {
    let vert_idx = inv_id.x;
    let vert_count = arrayLength(&vertices);
    if vert_idx >= vert_count {
        return;
    }

    let in = vertices[vert_idx];
    let in_joints = joints[vert_idx].joints;
    let in_weights = vec4<f32>(joints[vert_idx].weights_0, joints[vert_idx].weights_1);
    // wgsl doesn't support u16 directly; unpack from two u32s
    let joint_indices = vec4<u32>(
        in_joints[0] & 0xFFFFu,
        in_joints[0] >> 16u,
        in_joints[1] & 0xFFFFu,
        in_joints[1] >> 16u
    );

    var out: Vertex;
    out.tex_coords = in.tex_coords;
    var has_joints = false;
    for (var i = 0; i < 4; i++) {
        let weight = in_weights[i];
        if weight > 0. {
            has_joints = true;
            let ji = joint_indices[i];
            let joint_mat = joint_mats[ji];
            out.position += weight * (joint_mat * vec4<f32>(in.position, 1.)).xyz;

            let joint_mat_3 = mat3x3<f32>(joint_mat[0].xyz, joint_mat[1].xyz, joint_mat[2].xyz);
            let inv_scaling = mat3_inv_scale_sq(joint_mat_3);
            let weight_scaled = weight * inv_scaling;
            out.normal += weight_scaled * (joint_mat_3 * in.normal);
            out.tangent += weight_scaled * (joint_mat_3 * in.tangent);
        }
    }
    // if no joints had any weight, fallback to original values
    if !has_joints {
        out.position = in.position;
        out.normal = in.normal;
        out.tangent = in.tangent;
    }

    out_buf[vert_idx] = out;
}
