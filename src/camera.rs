use std::f32::consts::PI;

pub struct ScreenCamera {
    aspect_ratio: f32,
    fov: Angle,
    view_proj_matrix: glam::Mat4,
    z_near: f32,
    z_far: f32,
}
impl ScreenCamera {
    pub fn new(fov: Angle, aspect_ratio: f32, near_far: (f32, f32)) -> Self {
        let mut rval = Self {
            aspect_ratio,
            fov,
            view_proj_matrix: glam::Mat4::IDENTITY,
            z_near: near_far.0,
            z_far: near_far.1,
        };
        rval.compute_view_proj_matrix();
        rval
    }

    pub fn vertical_fov(&self) -> Angle {
        self.fov
    }
    pub fn set_vertical_fov(&mut self, fov: Angle) {
        self.fov = fov;
        self.compute_view_proj_matrix();
    }

    pub fn focal_length_35mm(&self) -> f32 {
        12.0 / (self.fov.as_rad() / 2.0).tan()
    }
    pub fn set_focal_length_35mm(&mut self, focal_length_mm: f32) {
        // afov = 2 * atan(vfov / (2 * f))
        // afov = 2 * atan(24 / (2 * f))        // 35mm film is 24mm tall
        // afov = 2 * atan(12 / f)
        self.fov = Angle::rad(2.0 * (12.0 / focal_length_mm).atan());
        self.compute_view_proj_matrix();
    }

    pub fn working_distance(&self) -> f32 {
        // vfov: in millimeters here, we always consider the screen to be 1080 units tall.
        // afov: angular fov, vertical angular fov
        // wd = vfov / (2 * tan(afov / 2))
        540.0 / (self.fov.as_rad() / 2.0).tan()
    }
    pub fn set_working_distance(&mut self, working_distance: f32) {
        self.fov = Angle::rad(2.0 * (540.0 / working_distance).atan());
        self.compute_view_proj_matrix();
    }

    pub fn near_far(&self) -> (f32, f32) {
        (self.z_near, self.z_far)
    }
    pub fn set_near_far(&mut self, near: f32, far: f32) {
        self.z_near = near;
        self.z_far = far;
        self.compute_view_proj_matrix();
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }
    pub fn set_aspect_ratio(&mut self, ratio: f32) {
        self.aspect_ratio = ratio;
        self.compute_view_proj_matrix();
    }

    fn compute_view_proj_matrix(&mut self) {
        let view_matrix =
            glam::Mat4::from_translation(glam::vec3(0.0, 0.0, -self.working_distance()));
        let proj_matrix = glam::Mat4::perspective_rh_gl(
            self.fov.as_rad(),
            self.aspect_ratio,
            self.z_near,
            self.z_far,
        );
        self.view_proj_matrix = proj_matrix * view_matrix;
    }

    pub fn view_proj_matrix(&self) -> glam::Mat4 {
        self.view_proj_matrix
    }
}

#[derive(Copy, Clone)]
pub struct Angle(f32);
impl Angle {
    pub fn rad(radians: f32) -> Self {
        Self(radians)
    }
    pub fn deg(degrees: f32) -> Self {
        Self(degrees * PI / 180.0)
    }
    pub fn as_deg(&self) -> f32 {
        self.0 * 180.0 / PI
    }
    pub fn as_rad(&self) -> f32 {
        self.0
    }
}
