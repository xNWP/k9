use bytemuck::{Pod, Zeroable};
use uuid::Uuid;

use crate::entity_component::Component;

pub mod component;
pub mod renderer;
pub mod system;

pub use component::GraphicsComponent;
pub use renderer::K9Renderer;
pub use system::GraphicsSystem;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Vertex {
    x: f32,
    y: f32,
    z: f32,
    u: f32,
    v: f32,
}
unsafe impl Pod for Vertex {}
unsafe impl Zeroable for Vertex {}

pub struct SceneDirectorComponent {
    active_camera: Option<Uuid>,
}
impl SceneDirectorComponent {
    pub fn new() -> Self {
        Self {
            active_camera: None,
        }
    }
}
impl Component for SceneDirectorComponent {
    const NAME: &'static str = "SceneDirector";
    const UUID: Uuid = uuid::uuid!("c4e971e4-5fbb-4fb8-94b3-fc092b78ba53");
}
