use uuid::Uuid;

use crate::{camera::ScreenCamera, entity_component::Component};

use super::system::GraphicsCommandInterface;

pub mod texquad;
pub use texquad::TexQuadBase;

pub enum GraphicsComponent {
    TexQuad(TexQuadBase),
}
impl GraphicsComponent {
    pub fn create(&mut self, k9cmd: &mut GraphicsCommandInterface, screen_camera: &ScreenCamera) {
        self.get_inner_mut().create(k9cmd, screen_camera)
    }
    pub fn delete(&mut self, k9cmd: &mut GraphicsCommandInterface, screen_camera: &ScreenCamera) {
        self.get_inner_mut().delete(k9cmd, screen_camera)
    }
    pub fn render(&mut self, k9cmd: &mut GraphicsCommandInterface, screen_camera: &ScreenCamera) {
        self.get_inner_mut().render(k9cmd, screen_camera)
    }

    pub fn get_inner(&self) -> &dyn GraphicsComponentImpl {
        match self {
            Self::TexQuad(base) => base as &dyn GraphicsComponentImpl,
        }
    }

    pub fn get_inner_mut(&mut self) -> &mut dyn GraphicsComponentImpl {
        match self {
            Self::TexQuad(base) => base as &mut dyn GraphicsComponentImpl,
        }
    }
}
impl Component for GraphicsComponent {
    const NAME: &'static str = "Graphics";
    const UUID: Uuid = uuid::uuid!("1adb66e9-89ca-4e84-aef1-32911d6bd104");
}

pub trait GraphicsComponentImpl {
    fn create(&mut self, _k9cmd: &mut GraphicsCommandInterface, _screen_camera: &ScreenCamera) {}
    fn delete(&mut self, _k9cmd: &mut GraphicsCommandInterface, _screen_camera: &ScreenCamera) {}
    fn render(&mut self, k9cmd: &mut GraphicsCommandInterface, screen_camera: &ScreenCamera);
}

pub enum RenderLocation {
    World(f32, f32, f32),
    Screen(f32, f32, f32),
}
