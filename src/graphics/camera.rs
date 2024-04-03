use crate::math::{self as m, uv};

mod mouse_drag;
pub use mouse_drag::MouseDragCameraController;

/// A camera determines the area of space to draw when rendering.
#[derive(Clone, Copy, Debug)]
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
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            view_width: 20.,
            view_height: 10.,
            pose: Default::default(),
            zoom: 1.,
            z_near: -1000.,
            z_far: 1000.,
        }
    }
}

impl Camera {
    /// Viewport pixels per world unit, taking into consideration zoom level.
    fn pixels_per_world_unit(&self, viewport_size: (u32, u32)) -> f32 {
        let (vp_w, vp_h) = viewport_size;
        (vp_w as f32 / self.view_width).min(vp_h as f32 / self.view_height) * self.zoom
    }

    // TODO: this 3x3 matrix is still used in debug.rs,
    // remove this and replace with a 4x4 view-projection matrix
    pub fn view_matrix(&self, viewport_size: (u32, u32)) -> uv::DMat3 {
        let vp_scaling = uv::DMat3::from_nonuniform_scale_homogeneous(m::Vec2::new(
            2. / viewport_size.0 as f64,
            2. / viewport_size.1 as f64,
        ));
        let my_transform_inv = self.pose_as_2d().inversed().into_homogeneous_matrix();
        vp_scaling * my_transform_inv
    }

    pub fn view_proj_matrix(&self, viewport_size: (u32, u32)) -> uv::Mat4 {
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
    pub fn point_screen_to_world(
        &self,
        viewport_size: (u32, u32),
        point_screen: m::Vec2,
    ) -> m::Vec2 {
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
    pub fn vector_screen_to_world(
        &self,
        viewport_size: (u32, u32),
        vec_screen: m::Vec2,
    ) -> m::Vec2 {
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
