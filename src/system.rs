use std::collections::BTreeMap;

use uuid::Uuid;

use crate::{
    camera::ScreenCamera,
    debug_ui::{console::DebugUiWindow, ConsoleCommand},
    entity_component::EntityTable,
};

pub trait System: SystemCallbacks {
    const UUID: Uuid;
}

pub trait SystemCallbacks {
    fn first_call(&mut self, first_call_state: FirstCallState, frame_state: FrameState);
    fn update(&mut self, state: FrameState);
    fn exiting(&mut self, state: FrameState);
}

pub struct FrameState<'a> {
    pub ents: &'a mut EntityTable,
    pub sdl_events: &'a Vec<sdl2::event::Event>,
    pub screen_camera: &'a mut ScreenCamera,
    pub screen_dimensions: (u32, u32),
    pub screen_scale: f32,
}

pub struct FirstCallState<'a> {
    pub console_commands: &'a mut BTreeMap<String, ConsoleCommand>,
    pub debug_windows: &'a mut BTreeMap<String, Box<dyn DebugUiWindow>>,
}
