use crate::{
    graphics::util::GpuMat4,
    math::{self as m, uv},
};
use std::{mem::size_of, sync::OnceLock};
use zerocopy::{AsBytes, FromBytes};

mod mouse_drag;
pub use mouse_drag::MouseDragCameraController;

/// Bind group layout for camera uniforms
/// created when the first camera is made
static BIND_GROUP_LAYOUT: OnceLock<wgpu::BindGroupLayout> = OnceLock::new();

/// A camera determines the area of space to draw when rendering.
#[derive(Debug)]
pub struct Camera {
    /// Width of the minimum seen area at zoom level 0 in world units. Default: 20
    pub view_width: f32,
    /// Height of the minimum seen area at zoom level 0 in world units. Default: 10
    pub view_height: f32,
    /// Pose of the camera in 3D world space.
    pub pose: uv::Isometry3,
    /// Level of magnification from the default `view_width`, `view_height`.
    pub zoom: f32,
    /// Near plane of the orthographic projection. Default: -1000
    pub z_near: f32,
    /// Far plane of the orthographic projection. Default: 1000
    pub z_far: f32,
    // internal GPU resources
    uniform_buf: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
pub(crate) struct CameraUniforms {
    view_proj: GpuMat4,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    pub fn new() -> Self {
        let device = crate::Renderer::device();

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<CameraUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh camera"),
            mapped_at_creation: false,
        });

        let bind_group_layout = Self::bind_group_layout();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        Self {
            view_width: 20.,
            view_height: 10.,
            pose: Default::default(),
            zoom: 1.,
            z_near: -1000.,
            z_far: 1000.,
            uniform_buf,
            bind_group,
        }
    }

    pub(crate) fn bind_group_layout<'a>() -> &'a wgpu::BindGroupLayout {
        let device = crate::Renderer::device();
        BIND_GROUP_LAYOUT.get_or_init(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<CameraUniforms>() as _),
                    },
                    count: None,
                }],
                label: Some("camera"),
            })
        })
    }

    /// Upload the current state of the camera to the GPU.
    /// Call at the start of a frame.
    pub fn upload(&mut self) {
        let queue = crate::Renderer::queue();
        let unif = CameraUniforms {
            view_proj: self.view_proj_matrix().into(),
        };
        queue.write_buffer(&self.uniform_buf, 0, unif.as_bytes());
    }

    /// Viewport pixels per world unit, taking into consideration zoom level.
    fn pixels_per_world_unit(&self, viewport_size: (u32, u32)) -> f32 {
        let (vp_w, vp_h) = viewport_size;
        (vp_w as f32 / self.view_width).min(vp_h as f32 / self.view_height) * self.zoom
    }

    /// View-projection matrix of this camera.
    ///
    /// Usually doesn't need to be called directly,
    /// since the camera handles uploading this to the GPU internally
    /// (with [`upload`][Self::upload]).
    pub fn view_proj_matrix(&self) -> uv::Mat4 {
        let window = crate::Renderer::window();
        // This assumes that the viewport being drawn to is the game window,
        // which is currently the only target you can draw to.
        // if this changes, we'll have to rethink this architecture -
        // can one camera be used to draw to multiple viewports?
        // if so, it would need more than one uniform buffer.
        // alternatively the viewport size could be a different uniform
        // instead of being packaged into the projection matrix
        let viewport_size = window.inner_size().into();

        let view = self.pose.inversed().into_homogeneous_matrix();

        let ppwu = self.pixels_per_world_unit(viewport_size);
        let z_range_size = self.z_far - self.z_near;
        // orthographic projection from starframe's left-handed space
        // to the also left-handed wgpu device coordinates
        let projection = uv::Mat4::new(
            uv::Vec4::new(ppwu * 2. / viewport_size.0 as f32, 0., 0., 0.),
            uv::Vec4::new(0., ppwu * 2. / viewport_size.1 as f32, 0., 0.),
            uv::Vec4::new(0., 0., 1. / z_range_size, 0.),
            uv::Vec4::new(0., 0., -self.z_near / z_range_size, 1.),
        );

        projection * view
    }

    /// Transform a point from screen space into world space.
    ///
    /// This expects that the camera has not been rotated outside of the xy plane.
    /// Results will be incorrect otherwise.
    pub fn point_screen_to_world(&self, point_screen: m::Vec2) -> m::Vec2 {
        let window = crate::Renderer::window();
        let viewport_size = window.inner_size().into();

        let ppwu = self.pixels_per_world_unit(viewport_size) as f64;
        let half_vp_diag = m::Vec2::new(viewport_size.0 as f64 / 2.0, viewport_size.1 as f64 / 2.0);
        let point_screen_wrt_center = {
            let p = point_screen - half_vp_diag;
            m::Vec2::new(p.x, -p.y) / ppwu
        };

        self.pose_as_2d() * point_screen_wrt_center
    }

    /// Transform a displacement vector from screen space into world space.
    ///
    /// This expects that the camera has not been rotated outside of the xy plane.
    /// Results will be incorrect otherwise.
    pub fn vector_screen_to_world(&self, vec_screen: m::Vec2) -> m::Vec2 {
        let window = crate::Renderer::window();
        let viewport_size = window.inner_size().into();
        let ppwu = self.pixels_per_world_unit(viewport_size) as f64;
        m::Vec2::new(vec_screen.x, -vec_screen.y) / ppwu
    }

    fn pose_as_2d(&self) -> m::Pose {
        m::Pose::new(
            uv::DVec2::new(
                self.pose.translation.x as f64,
                self.pose.translation.y as f64,
            ),
            uv::DRotor2::new(
                self.pose.rotation.s as f64,
                uv::DBivec2::new(self.pose.rotation.bv.xy as f64),
            ),
        )
    }
}
