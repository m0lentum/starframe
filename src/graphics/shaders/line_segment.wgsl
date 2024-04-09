// Line segment shader without overlaps, to facilitate alpha-blended rendering.
// Loosely based on https://wwwtyro.net/2021/10/01/instanced-lines-part-2.html
// but adapted to draw segments and round joins in one pass

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
    // position of the individual vertex in the line segment instance
    // (x, y are normal coordinates, z corresponds to position in the join geometry)
    @location(0) pos_local: vec3<f32>,
    // start and end points of the segment from the instance buffer
    // (last element of the vec4 is the width of the line at the point)
    @location(1) prev_point: vec4<f32>,
    @location(2) start_point: vec4<f32>,
    @location(3) end_point: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    // doing the computations in 2D
    // and adding the z coordinate back at the end
    let x_basis = end_point.xy - start_point.xy;
    let x_basis_len = length(x_basis);
    let x_basis_n = x_basis / x_basis_len;
    let prev_x_basis = start_point.xy - prev_point.xy;

    let width = mix(start_point.w, end_point.w, pos_local.x);
    let radius = width / 2.;
    let z_coord = mix(start_point.z, end_point.z, pos_local.x);

    // y basis always oriented with the xy plane
    // (assumes the line isn't facing directly away from the viewer)
    var y_basis = vec2<f32>(-x_basis_n.y, x_basis_n.x);
    // orient y basis in the direction of the join geometry
    let bend_direction = sign(dot(y_basis, prev_x_basis));
    y_basis = bend_direction * y_basis;
    // compute the average tangent and normal vectors at the cusp
    // to figure out how to place the join
    let tangent = normalize(x_basis_n + normalize(prev_x_basis));
    let normal = bend_direction * vec2<f32>(-tangent.y, tangent.x);

    var pos_world: vec2<f32>;
    if pos_local.x == 0. {
        // we're at the join end of the half-segment
        if pos_local.y < 0. {
            // bottom vertex adjusted for overlap.
            // cap the adjustment to the maximum dimension of the segment
            // to minimize artifacts when the segments overlap completely
            // (this doesn't completely stop artifacts but makes them harder to notice)
            let max_offset = max(x_basis_len, width);
            let pos_local_adjusted = -normal * min(radius / dot(normal, y_basis), max_offset);
            pos_world = start_point.xy + pos_local_adjusted;
        } else {
            // one of the top vertices that make up the curved side of the join

            // clamp required because the dot product can have magnitude >1 
            // due to floating point error, producing undefined results
            let full_angle = acos(clamp(dot(normal, y_basis), -1., 1.));
            // constant resolution of 2 vertices on the join
            let vert_angle = pos_local.z * full_angle / 2.;
            let pos_local_rotated = radius * vec2<f32>(-sin(vert_angle), cos(vert_angle));

            let basis_mat = mat2x2<f32>(x_basis_n, y_basis);
            pos_world = start_point.xy + basis_mat * pos_local_rotated;
        }
    } else {
        // center end of the half-segment, just a simple transform
        let basis_mat = mat2x2<f32>(x_basis, width * y_basis);
        pos_world = start_point.xy + basis_mat * pos_local.xy;
    }

    out.clip_position = camera.view_proj * vec4<f32>(pos_world, z_coord, 1.);

    // simple uv mapping for now: each segment has the texture stretched to its length
    // and the endmost pixels are repeated for the turn.
    // this can do uniform textures that only vary in the width direction.
    // would be nice to have a texture that dynamically spreads over the line
    // so we could also have lengthwise variation
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
