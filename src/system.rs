use std::collections::BTreeMap;

use uuid::Uuid;

use crate::{camera::ScreenCamera, debug_ui::ConsoleCommand, entity_component::EntityTable};

pub trait System: SystemCallbacks {
    const UUID: Uuid;
}

pub trait SystemCallbacks {
    fn first_call(&mut self, state: FrameState);
    fn update(&mut self, state: FrameState);
    fn exiting(&mut self, state: FrameState);
}

pub struct FrameState<'a> {
    pub ents: &'a mut EntityTable,
    pub sdl_events: &'a Vec<sdl2::event::Event>,
    pub screen_camera: &'a mut ScreenCamera,
    pub screen_dimensions: (u32, u32),
    pub screen_scale: f32,
    pub console_commands: &'a mut BTreeMap<String, ConsoleCommand>,
}
