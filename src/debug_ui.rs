use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use egui::{Color32, RichText};
use sdl2::clipboard::ClipboardUtil;

pub use self::console::ConsoleCommand;
pub(super) use self::console::DebugConsoleLogger;
use self::{console::DebugConsole, egui_render_core::EguiRenderCore};

const BG_COLOUR: Color32 = Color32::from_rgb(26, 0, 15);
const BG_LIGHTER: Color32 = Color32::from_rgb(52, 1, 29);
const ACCENT_COLOUR: Color32 = Color32::from_rgb(252, 11, 146);
const ACCENT_DARKER: Color32 = Color32::from_rgb(208, 3, 118);
const ACCENT_DARKEST: Color32 = Color32::from_rgb(156, 2, 88);
const TEXT_COLOUR: Color32 = Color32::from_rgb(255, 231, 244);
const DIM_TEXT_COLOUR: Color32 = Color32::from_rgb(172, 130, 153);
const OFF_ACCENT_COLOUR: Color32 = Color32::from_rgb(11, 252, 117);
//const OFF2_ACCENT_COLOUR: Color32 = Color32::from_rgb(11, 252, 237);
//const OFF3_ACCENT_COLOUR: Color32 = Color32::from_rgb(252, 117, 11);
const OFF_BG_COLOUR: Color32 = Color32::from_rgb(16, 27, 36);
// 18 12 8

const BANNER_HEIGHT: f32 = 50.0;

pub mod console;
mod egui_render_core;

pub struct EguiDebugUi {
    egui_core: EguiRenderCore,
    console_core: DebugConsole,
    mouse_pos: egui::Pos2,
    ui_scale: f32,
    live_ui_scale: f32,
    ui_opacity: f32,
    visuals: egui::Visuals,
}

impl EguiDebugUi {
    pub fn new(
        glow: &glow::Context,
        default_ui_scale: f32,
        console_commands: BTreeMap<String, ConsoleCommand>,
        debug_windows: BTreeMap<String, Box<dyn console::DebugUiWindow>>,
    ) -> Self {
        let mouse_pos = egui::pos2(-100.0, -100.0); // offscreen so that it doesn't show until we get a valid mouse pos

        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT_COLOUR);
        visuals.window_stroke = egui::Stroke::new(0.8, ACCENT_COLOUR);
        visuals.extreme_bg_color = BG_LIGHTER;
        visuals.widgets.inactive.bg_fill = ACCENT_DARKEST;
        visuals.widgets.hovered.bg_fill = ACCENT_DARKER;
        visuals.widgets.active.bg_fill = ACCENT_COLOUR;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, ACCENT_COLOUR);

        let mut shadow = egui::epaint::Shadow::NONE;
        shadow.extrusion = 5.0;
        visuals.window_shadow = shadow;

        let egui_core = EguiRenderCore::new(glow, default_ui_scale);
        egui_core.ctx.set_visuals(visuals.clone());

        Self {
            egui_core,
            mouse_pos,
            ui_scale: default_ui_scale,
            live_ui_scale: default_ui_scale,
            ui_opacity: 0.80,
            visuals,
            console_core: DebugConsole::new(console_commands, debug_windows),
        }
    }

    pub fn set_console_focus(&mut self) {
        self.console_core.set_console_focus = true;
    }

    pub fn wants_keyboard_input(&self) -> bool {
        self.egui_core.ctx.wants_keyboard_input()
    }

    pub fn draw(
        &mut self,
        screen_dimensions: (u32, u32),
        logger: &Arc<RwLock<Vec<console::DebugLogRecord>>>,
    ) {
        // setup visuals
        self.visuals.window_fill =
            Color32::from_rgba_unmultiplied(BG_COLOUR.r(), BG_COLOUR.g(), BG_COLOUR.b(), {
                (self.ui_opacity * 255.0) as u8
            });
        self.egui_core.ctx.set_visuals(self.visuals.clone());

        let w = screen_dimensions.0 as f32 / self.ui_scale;
        let _h = screen_dimensions.1 as f32 / self.ui_scale;
        let banner_bg =
            Color32::from_rgba_unmultiplied(BG_COLOUR.r(), BG_COLOUR.g(), BG_COLOUR.b(), 128);

        // draw banner
        egui::TopBottomPanel::top("egui_debug_ui_top_panel")
            .frame(egui::Frame::none())
            .show(&self.egui_core.ctx, |ui| {
                let painter = ui.painter();
                painter.rect(
                    egui::Rect::from_two_pos(
                        egui::pos2(-20.0, -20.0),
                        egui::pos2(w + 20.0, BANNER_HEIGHT),
                    ),
                    egui::Rounding::none(),
                    banner_bg,
                    egui::Stroke::new(1.0, ACCENT_COLOUR),
                );

                ui.horizontal(|ui| {
                    ui.add_sized(
                        [64.0, BANNER_HEIGHT],
                        egui::Label::new(RichText::new("k9").size(36.0).strong()),
                    );
                });

                ui.allocate_ui_at_rect(
                    egui::Rect::from_two_pos(
                        egui::pos2(w - 310.0, 6.0),
                        egui::pos2(w, BANNER_HEIGHT),
                    ),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label("ui opacity");
                                ui.add(egui::Slider::new(&mut self.ui_opacity, 0.0..=1.0));
                            });
                            ui.vertical(|ui| {
                                ui.label("ui scale");
                                let resp =
                                    ui.add(egui::Slider::new(&mut self.live_ui_scale, 0.5..=2.0));

                                if (resp.changed() && !resp.dragged()) || resp.drag_released() {
                                    self.ui_scale = self.live_ui_scale;
                                    ui.ctx().set_pixels_per_point(self.ui_scale);
                                }
                            });
                        })
                    },
                );

                let mut debug_mouse_pos = self.mouse_pos;
                debug_mouse_pos.x = debug_mouse_pos.x / self.ui_scale + 16.0;
                debug_mouse_pos.y = debug_mouse_pos.y / self.ui_scale + 9.0;
                ui.ctx().debug_painter().debug_text(
                    debug_mouse_pos,
                    egui::Align2::LEFT_TOP,
                    TEXT_COLOUR,
                    format!("{:?}", self.mouse_pos),
                );
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(&self.egui_core.ctx, |ui| {
                // draw console
                self.console_core.draw(ui, logger, self.ui_opacity);
            });
    }

    pub fn render(
        &mut self,
        glow: &glow::Context,
        sdl_events: &Vec<sdl2::event::Event>,
        clipboard_util: &ClipboardUtil,
        screen_dimensions: (u32, u32),
        window_has_focus: bool,
        logger: &Arc<RwLock<Vec<console::DebugLogRecord>>>,
    ) {
        self.egui_core.begin_frame(
            window_has_focus,
            sdl_events,
            screen_dimensions,
            clipboard_util,
        );
        self.draw(screen_dimensions, logger);
        let (primitives, tex_delta, plat_output) = self.egui_core.end_frame();
        self.egui_core
            .handle_platform_output(plat_output, clipboard_util);
        self.egui_core
            .render(glow, screen_dimensions, primitives, tex_delta);
    }
}
