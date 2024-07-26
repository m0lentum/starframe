@group(0) @binding(0)
var mip_src: texture_2d<f32>;
@group(0) @binding(1)
var mip_dst: texture_storage_2d<rgba8unorm, write>;

const TILE_SIZE: u32 = 16u;

@compute
@workgroup_size(TILE_SIZE, TILE_SIZE)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let dst_texel_id = global_id.xy;
    let dst_tex_size = textureDimensions(mip_dst);
    if dst_texel_id.x >= dst_tex_size.x || dst_texel_id.y >= dst_tex_size.y {
        return;
    }

    let src_top_left = dst_texel_id * 2u;
    let tl = textureLoad(mip_src, src_top_left, 0);
    let tr = textureLoad(mip_src, src_top_left + vec2<u32>(1u, 0u), 0);
    let br = textureLoad(mip_src, src_top_left + vec2<u32>(1u, 1u), 0);
    let bl = textureLoad(mip_src, src_top_left + vec2<u32>(0u, 1u), 0);
    let avg = 0.25 * (tl + tr + br + bl);

    textureStore(mip_dst, dst_texel_id, avg);
}