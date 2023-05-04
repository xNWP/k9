use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use bnf::{ParseTree, ParseTreeNode};
use bytemuck::{offset_of, Pod, Zeroable};
use egui::{
    epaint::{
        text::{cursor::RCursor, TextWrapping},
        Primitive,
    },
    text::LayoutJob,
    Align, Color32, FontId, Frame, PaintCallbackInfo, RichText, Sense, TextFormat,
};
use egui_extras::Column;
use glow::HasContext;
use k9_proc_macros::console_command_internal;
use sdl2::clipboard::ClipboardUtil;
use time::OffsetDateTime;

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

const SCROLL_SCALE: f32 = 20.0;

type Flag = bool;
pub struct EguiDebugUi {
    ctx: egui::Context,
    input: egui::RawInput,
    modifiers: ModifierTracker,
    mouse_pos: egui::Pos2,
    start_time: Instant,
    console_text: String,
    record_windows: Option<BTreeMap<usize, RecordWindow>>,
    ui_scale: f32,
    live_ui_scale: f32,
    set_console_focus: Flag,
    textures: BTreeMap<egui::TextureId, glow::NativeTexture>,
    program: glow::NativeProgram,
    u_screen_size: glow::NativeUniformLocation,
    u_sampler: glow::NativeUniformLocation,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    ebo: glow::NativeBuffer,
    sdl_cursor: Option<*mut sdl2::sys::SDL_Cursor>,
    delete_console_text: Flag,
    console_has_focus: bool,
    command_grammar: bnf::Grammar,
    ui_opacity: f32,
    visuals: egui::Visuals,
    debug_console_commands: Arc<Mutex<bool>>,
    selected_autocomplete_cmd: Option<(String, usize)>,
    preview_autocomplete_cmds: Vec<String>,
    draw_preview_commands_list: bool,
    last_console_window_height: f32,
}

pub struct ConsoleCommand {
    cb: Box<dyn FnMut(BTreeMap<String, CallbackArgumentValue>) -> Result<(), String> + 'static>,
    args: Vec<CallbackArgumentDefinition>,
}

#[derive(Debug)]
pub enum CallbackArgumentValue {
    Float32(f32),
    Float64(f64),
    Int32(i32),
    Int64(i64),
    String(String),
    Bool(bool),
    Flag(bool),
}

#[derive(Debug)]
pub struct CallbackArgumentDefinition {
    pub name: String,
    pub cba_type: CallbackArgumentType,
}

#[derive(Debug)]
pub enum CallbackArgumentType {
    Float32,
    Float64,
    Int32,
    Int64,
    String,
    Bool,
    Flag,
}
impl ConsoleCommand {
    pub fn new(
        cb: impl FnMut(BTreeMap<String, CallbackArgumentValue>) -> Result<(), String> + 'static,
        args: Vec<CallbackArgumentDefinition>,
    ) -> Self {
        Self {
            cb: Box::new(cb),
            args,
        }
    }
}

struct RecordWindow {
    record: DebugLogRecord,
    is_open: bool,
    wrap_text: bool,
    fake_text: String,
}

impl EguiDebugUi {
    pub fn new(
        glow: &glow::Context,
        default_ui_scale: f32,
        console_commands: &mut BTreeMap<String, ConsoleCommand>,
    ) -> Self {
        // draw code related stuffs
        let ctx = egui::Context::default();
        let input = egui::RawInput::default();

        let modifiers = ModifierTracker::new();
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

        ctx.set_visuals(visuals.clone());

        let ui_scale = default_ui_scale;
        ctx.set_pixels_per_point(ui_scale);

        // setup some console commands
        let debug_console_commands = Arc::new(Mutex::new(false));
        {
            let val = debug_console_commands.clone();
            let cc_debug_console_command = console_command_internal!({ value: bool }, |value| {
                *val.lock().unwrap() = value;
                Ok(())
            });
            console_commands.insert(
                "k9_debug_console_command".to_owned(),
                cc_debug_console_command,
            );
        }

        // internal opengl render related stuffs
        unsafe {
            let vert_shader = match glow.create_shader(glow::VERTEX_SHADER) {
                Ok(x) => x,
                Err(e) => {
                    panic!("failed to create egui debug ui vert shader: {e}");
                }
            };
            const VERT_SRC: &'static str = include_str!("k9_egui_debug_ui.vert.glsl");

            glow.shader_source(vert_shader, VERT_SRC);
            glow.compile_shader(vert_shader);

            if !glow.get_shader_compile_status(vert_shader) {
                let err = glow.get_shader_info_log(vert_shader);
                glow.delete_shader(vert_shader);
                panic!("egui debug ui shader compile error: {err}");
            }

            let frag_shader = match glow.create_shader(glow::FRAGMENT_SHADER) {
                Ok(x) => x,
                Err(e) => {
                    panic!("failed to create egui debug ui frag shader: {e}");
                }
            };
            const FRAG_SRC: &'static str = include_str!("k9_egui_debug_ui.frag.glsl");

            glow.shader_source(frag_shader, FRAG_SRC);
            glow.compile_shader(frag_shader);

            if !glow.get_shader_compile_status(frag_shader) {
                let err = glow.get_shader_info_log(frag_shader);
                glow.delete_shader(frag_shader);
                panic!("egui debug ui shader compile error: {err}");
            }

            let program = match glow.create_program() {
                Ok(x) => x,
                Err(e) => panic!("failed to create egui debug ui shader program: {e}"),
            };
            glow.attach_shader(program, vert_shader);
            glow.attach_shader(program, frag_shader);

            glow.link_program(program);
            glow.detach_shader(program, vert_shader);
            glow.detach_shader(program, frag_shader);
            glow.delete_shader(vert_shader);
            glow.delete_shader(frag_shader);

            if !glow.get_program_link_status(program) {
                let err = glow.get_program_info_log(program);
                panic!("couldn't link egui debug ui program: {err}");
            }

            let u_screen_size = glow.get_uniform_location(program, "u_screen_size").unwrap();
            let u_sampler = glow.get_uniform_location(program, "u_sampler").unwrap();

            let vao = glow.create_vertex_array().unwrap();
            let vbo = glow.create_buffer().unwrap();
            let ebo = glow.create_buffer().unwrap();

            glow.bind_vertex_array(Some(vao));
            glow.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            const VERTEX_SIZE: i32 = std::mem::size_of::<EguiVertexPod>() as i32;

            glow.vertex_attrib_pointer_f32(
                0,
                2,
                glow::FLOAT,
                false,
                VERTEX_SIZE,
                offset_of!(EguiVertexPod, pos) as i32,
            );
            glow.enable_vertex_attrib_array(0);
            glow.vertex_attrib_pointer_f32(
                1,
                2,
                glow::FLOAT,
                false,
                VERTEX_SIZE,
                offset_of!(EguiVertexPod, uv) as i32,
            );
            glow.enable_vertex_attrib_array(1);
            glow.vertex_attrib_pointer_f32(
                2,
                4,
                glow::UNSIGNED_BYTE,
                false,
                VERTEX_SIZE,
                offset_of!(EguiVertexPod, colour) as i32,
            );
            glow.enable_vertex_attrib_array(2);

            glow.bind_vertex_array(None);
            glow.bind_buffer(glow::ARRAY_BUFFER, None);

            const GRAMMAR: &'static str = include_str!("console_command.bnf");
            let command_grammar: bnf::Grammar = GRAMMAR.parse().unwrap();

            Self {
                ctx,
                input,
                modifiers,
                mouse_pos,
                start_time: Instant::now(),
                console_text: "".to_owned(),
                record_windows: Some(BTreeMap::new()),
                ui_scale,
                live_ui_scale: ui_scale,
                set_console_focus: false,
                ebo,
                program,
                sdl_cursor: None,
                textures: BTreeMap::new(),
                u_sampler,
                u_screen_size,
                vao,
                vbo,
                delete_console_text: false,
                console_has_focus: false,
                command_grammar,
                ui_opacity: 0.80,
                visuals,
                debug_console_commands,
                selected_autocomplete_cmd: None,
                preview_autocomplete_cmds: Vec::new(),
                draw_preview_commands_list: false,
                last_console_window_height: 0.0,
            }
        }
    }

    pub fn set_console_focus(&mut self) {
        self.set_console_focus = true;
    }

    pub fn begin_frame(
        &mut self,
        window_has_focus: bool,
        sdl_events: &Vec<sdl2::event::Event>,
        screen_dimensions: (u32, u32),
        clipboard_util: &ClipboardUtil,
    ) {
        //let screen_scale = 1.0;
        self.input.time = Some(self.start_time.elapsed().as_secs_f64());
        self.input.has_focus = window_has_focus;
        let w_pts = screen_dimensions.0 as f32 / self.ui_scale; // changes scale
        let h_pts = screen_dimensions.1 as f32 / self.ui_scale; // changes scale
        self.input.screen_rect = Some(egui::Rect::from_two_pos(
            egui::pos2(0.0, 0.0),
            egui::pos2(w_pts, h_pts),
        ));
        self.input.pixels_per_point = Some(self.ui_scale); // changes draw res

        self.fire_egui_events(&sdl_events, clipboard_util); // changes input mapping

        self.ctx.begin_frame(self.input.clone());
    }

    pub fn end_frame(
        &mut self,
    ) -> (
        Vec<egui::ClippedPrimitive>,
        egui::TexturesDelta,
        egui::PlatformOutput,
    ) {
        let full_output = self.ctx.end_frame();
        let clipped_prims = self.ctx.tessellate(full_output.shapes);
        let texs_delta = full_output.textures_delta;

        self.input.events.clear();
        (clipped_prims, texs_delta, full_output.platform_output)
    }

    pub fn wants_keyboard_input(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    pub fn get_ui_scale(&self) -> f32 {
        self.ui_scale
    }

    pub fn draw(
        &mut self,
        screen_dimension: (u32, u32),
        logger: &Arc<RwLock<Vec<DebugLogRecord>>>,
        console_commands: &mut BTreeMap<String, ConsoleCommand>,
    ) {
        // setup visuals
        self.visuals.window_fill =
            Color32::from_rgba_unmultiplied(BG_COLOUR.r(), BG_COLOUR.g(), BG_COLOUR.b(), {
                (self.ui_opacity * 255.0) as u8
            });
        self.ctx.set_visuals(self.visuals.clone());

        let w = screen_dimension.0 as f32 / self.ui_scale;
        let _h = screen_dimension.1 as f32 / self.ui_scale;
        let banner_bg =
            Color32::from_rgba_unmultiplied(BG_COLOUR.r(), BG_COLOUR.g(), BG_COLOUR.b(), 128);

        // draw banner
        egui::TopBottomPanel::top("egui_debug_ui_top_panel")
            .frame(egui::Frame::none())
            .show(&self.ctx, |ui| {
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
            .show(&self.ctx, |_| {
                // draw log record windows
                let record_wnds = self.record_windows.take().unwrap();
                let mut keep_wnds = BTreeMap::new();

                for (idx, mut wnd) in record_wnds {
                    if wnd.is_open {
                        egui::Window::new(format!("Debug Record #{}", wnd.record.idx))
                            .open(&mut wnd.is_open)
                            .default_size([320.0, 0.0])
                            .min_height(210.0)
                            .show(&self.ctx, |ui| {
                                egui::TopBottomPanel::bottom(ui.next_auto_id())
                                    .frame(Frame::none())
                                    .show_inside(ui, |ui| {
                                        ui.add_space(6.0);
                                        ui.checkbox(&mut wnd.wrap_text, "wrap text");

                                        egui_extras::TableBuilder::new(ui)
                                            .column(Column::exact(64.0))
                                            .column(Column::remainder())
                                            .body(|mut body| {
                                                const ROW_HEIGHT: f32 = 12.0;
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("Level");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", &wnd.record.level));
                                                    });
                                                });
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("Target");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", &wnd.record.target));
                                                    });
                                                });
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("Time");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", debug_ui_offset_date_time_format(&wnd.record.local_time)));
                                                    });
                                                });
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("File");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", &wnd.record.file));
                                                    });
                                                });
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("Module");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", &wnd.record.module));
                                                    });
                                                });
                                                body.row(ROW_HEIGHT, |mut row| {
                                                    row.col(|ui| {
                                                        ui.label("Line");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(format!("{}", &wnd.record.line));
                                                    });
                                                });
                                            });

                                    });

                                egui::CentralPanel::default()
                                    .frame(Frame::none())
                                    .show_inside(ui, |ui| {
                                        ui.set_clip_rect(ui.available_rect_before_wrap());
                                        egui::ScrollArea::both()
                                            .id_source("log_message_scroll_area")
                                            .auto_shrink([false, false])
                                            .min_scrolled_height(60.0)
                                            .show(ui, |ui| {
                                                if egui::TextEdit::multiline(&mut wnd.fake_text)
                                                .frame(false)
                                                .code_editor()
                                                .show(ui).response.changed() {
                                                    // fake text lets us keep this selectable
                                                    // but keep it immutable... it's not a perfect solution
                                                    // as the text can be changed for a frame or two, but it works
                                                    wnd.fake_text = wnd.record.text.clone();
                                                }
                                            });
                                    });
                            });

                        keep_wnds.insert(idx, wnd);
                    }
                }

                self.record_windows = Some(keep_wnds);

                egui::Window::new("k9 console")
                    .default_size([640.0, 320.0])
                    .min_height(100.0)
                    .show(&self.ctx, |ui| {
                        egui::TopBottomPanel::bottom("k9_console_text_entry_panel")
                            .frame(egui::Frame::none())
                            .show_inside(ui, |ui| {
                                // handle up/down key navigation logic, includes autocomplete logic and history logic
                                ui.input_mut(|input| {
                                    if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                                        if let Some((_, it)) = &self.selected_autocomplete_cmd {
                                            if *it == 0 {
                                                self.selected_autocomplete_cmd = None;
                                                self.draw_preview_commands_list = false;
                                            } else {
                                                self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[it - 1].clone(), it - 1));
                                            }
                                        } else {
                                            // todo: history
                                        }
                                    }
                                    if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                                        if let Some((_, it)) = &self.selected_autocomplete_cmd {
                                            if *it as i32 == self.preview_autocomplete_cmds.len() as i32 - 1 { // cast to handle underflow
                                                self.selected_autocomplete_cmd = None;
                                                self.draw_preview_commands_list = false;
                                            } else {
                                                self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[it + 1].clone(), it + 1));
                                            }
                                        } else {
                                            // todo: history
                                        }
                                    }
                                });

                                // handle autocomplete tab/right arrow complete
                                ui.input_mut(|input| {
                                    if input.key_pressed(egui::Key::ArrowRight) {
                                        if let Some((cmd_text, _)) = &self.selected_autocomplete_cmd {
                                            self.console_text = cmd_text.clone();
                                            self.draw_preview_commands_list = false;

                                            input.events.push(egui::Event::Key {
                                                key: egui::Key::ArrowRight,
                                                pressed: true,
                                                repeat: false,
                                                modifiers: egui::Modifiers::CTRL,
                                            });
                                        }
                                    }

                                    if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                                        if self.selected_autocomplete_cmd.is_none() && !self.preview_autocomplete_cmds.is_empty() {
                                            self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[0].clone(), 0));
                                        } else if self.preview_autocomplete_cmds.len() == 1 {
                                            self.console_text = self.preview_autocomplete_cmds[0].clone();
                                            self.draw_preview_commands_list = false;

                                            input.events.push(egui::Event::Key {
                                                key: egui::Key::ArrowRight,
                                                pressed: true,
                                                repeat: false,
                                                modifiers: egui::Modifiers::CTRL,
                                            });
                                        } else {
                                            if self.draw_preview_commands_list {
                                                self.draw_preview_commands_list = false;
                                            } else {
                                                if !self.preview_autocomplete_cmds.is_empty() {
                                                    // must consume tab before textedit grabs it to avoid crash,
                                                    // but also need to draw the command list after drawing the
                                                    // textedit, we defer drawing the command list via this flag.
                                                    self.draw_preview_commands_list = true;
                                                }
                                            }
                                        }
                                    }
                                });

                                // draw console command text edit entry
                                ui.add_space(6.0);
                                let te_output = egui::TextEdit::singleline(&mut self.console_text)
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("enter command")
                                    .code_editor()
                                    .vertical_align(egui::Align::Center)
                                    .show(ui);

                                if self.delete_console_text {
                                    self.delete_console_text = false;
                                    self.console_text.clear();
                                }

                                let te_resp = te_output.response;
                                if self.set_console_focus {
                                    self.set_console_focus = false;
                                    te_resp.request_focus();
                                }

                                // draw preview command list
                                if self.draw_preview_commands_list {
                                    if self.preview_autocomplete_cmds.is_empty() {
                                        self.draw_preview_commands_list = false;
                                    } else {
                                        let mut cmds_text = "".to_owned();
                                        let mut cmds_text_full = "".to_owned();
                                        let mut active_text = ("".to_owned(), 0);

                                        if let Some((sel_cmd_txt, sel_cmd_it)) = &self.selected_autocomplete_cmd {
                                            let max_entries: isize = (self.last_console_window_height - 40.0) as isize / ui.text_style_height(&egui::TextStyle::Monospace) as isize;
                                            let max_entries_l1: isize = max_entries - 1;
                                            let mut preview_min: isize;
                                            let mut preview_max: isize;
                                            let mut more_above = None;
                                            let mut more_below = None;

                                            let prev_list_len = self.preview_autocomplete_cmds.len() as isize;
                                            let max_prev_list_idx = prev_list_len - 1;

                                            if prev_list_len <= max_entries {
                                                preview_min = 0;
                                                preview_max = max_prev_list_idx;
                                            } else {
                                                let div_2 = max_entries_l1 / 2;
                                                let preview_min_diff;
                                                let preview_max_diff;

                                                if max_entries_l1 % 2 == 0 {
                                                    preview_min_diff = div_2;
                                                    preview_max_diff = div_2;
                                                } else {
                                                    preview_min_diff = div_2 + 1;
                                                    preview_max_diff = div_2;
                                                }

                                                preview_min = *sel_cmd_it as isize - preview_min_diff;
                                                preview_max = *sel_cmd_it as isize + preview_max_diff;
                                                if preview_min < 0 {
                                                    preview_min = 0;
                                                    preview_max = max_entries_l1;
                                                    more_below = Some(prev_list_len - preview_max);
                                                } else if preview_max >= max_prev_list_idx {
                                                    preview_min = prev_list_len - max_entries;
                                                    preview_max = max_prev_list_idx;
                                                    more_above = Some(preview_min + 1);
                                                } else {
                                                    if preview_min != 0 {
                                                        more_above = Some(preview_min + 1);
                                                    }
                                                    more_below = Some(prev_list_len - preview_max);
                                                }
                                            }

                                            let mut it = 0;
                                            if let Some(x) = more_above {
                                                let msg = format!("<{x} more>\n");
                                                cmds_text += &msg;
                                                cmds_text_full += &msg;
                                                it += 1;
                                                preview_min += 1;
                                            }
                                            if more_below.is_some() {
                                                preview_max -= 1;
                                            }

                                            for j in preview_min..=preview_max {
                                                let cmd = &self.preview_autocomplete_cmds[j as usize];
                                                let add_text = format!("{j}: {cmd}\n");

                                                if sel_cmd_txt == cmd {
                                                    active_text = (format!("{j}: {cmd}"), it);
                                                    cmds_text_full += &add_text;
                                                    cmds_text += "\n";
                                                    continue;
                                                }
                                                cmds_text += &add_text;
                                                cmds_text_full += &add_text;
                                                it += 1;
                                            }

                                            if let Some(x) = more_below {
                                                let msg = format!("<{x} more>");
                                                cmds_text += &msg;
                                                cmds_text_full += &msg;
                                            } else {
                                                cmds_text.pop();
                                                cmds_text_full.pop();
                                            }
                                        }

                                        let draw_pos = te_output.text_draw_pos.to_vec2();
                                        let mut draw_pos = te_output.galley.rect.min + draw_pos;
                                        draw_pos.y -= 12.0;

                                        // we need to get a painter ref and determine our drawing location first,
                                        // then we can allocate an owned painter on the debug layer to draw ontop of
                                        // everything else.
                                        let painter_tmp = ui.painter();

                                        let galley = painter_tmp.layout(
                                            cmds_text_full,
                                            FontId::monospace(12.0),
                                            TEXT_COLOUR,
                                            f32::INFINITY,
                                        );

                                        let background_rect = galley.rect;
                                        let mut background_rect = background_rect.translate(draw_pos.to_vec2() + [0.0, -background_rect.height()].into()).expand(4.0);
                                        *background_rect.right_mut() += 16.0;

                                        let mut painter = ui.painter_at(background_rect);
                                        painter.set_layer_id(egui::LayerId::debug());

                                        let fill = Color32::from_rgba_unmultiplied(
                                            OFF_BG_COLOUR.r(),
                                            OFF_BG_COLOUR.g(),
                                            OFF_BG_COLOUR.b(),
                                            (self.ui_opacity * 255.0) as u8,
                                        );
                                        painter.rect(
                                            background_rect,
                                            0.0,
                                            fill,
                                            egui::Stroke::new(2.0, OFF_ACCENT_COLOUR),
                                        );

                                        painter.text(
                                            draw_pos,
                                            egui::Align2::LEFT_BOTTOM,
                                            cmds_text,
                                            FontId::monospace(12.0),
                                            TEXT_COLOUR,
                                        );

                                        let active_preview_rect = galley.pos_from_cursor(&galley.from_rcursor(RCursor { column: 0, row: active_text.1 }));
                                        draw_pos.y -= galley.rect.height() - active_preview_rect.bottom();

                                        painter.text(
                                            draw_pos,
                                            egui::Align2::LEFT_BOTTOM,
                                            active_text.0,
                                            FontId::monospace(12.0),
                                            OFF_ACCENT_COLOUR,
                                        );
                                    }
                                }

                                // draw autocomplete
                                if let Some((preview_txt, _)) = &self.selected_autocomplete_cmd {
                                    let input_len = self.console_text.len();
                                    if input_len < preview_txt.len() {
                                        let render_text = &preview_txt[self.console_text.len()..];
                                        let draw_pos = te_output.text_draw_pos.to_vec2();
                                        let draw_pos = te_output.galley.rect.max + draw_pos;

                                        ui.painter().text(
                                            draw_pos,
                                            egui::Align2::LEFT_BOTTOM,
                                            render_text,
                                            FontId::monospace(12.0),
                                            DIM_TEXT_COLOUR,
                                        );
                                    }
                                }

                                // autocomplete logic
                                if te_resp.changed() {
                                    let prev_selected = self.selected_autocomplete_cmd.take();
                                    self.preview_autocomplete_cmds.clear();

                                    if !self.console_text.is_empty() {
                                        // gather predictions
                                        let mut prev_index = None;
                                        let mut it = 0;
                                        for cmd in console_commands.iter() {
                                            if cmd.0.starts_with(&self.console_text) {
                                                self.preview_autocomplete_cmds.push(cmd.0.clone());
                                                if let Some((name, _)) = &prev_selected {
                                                    if *cmd.0 == *name {
                                                        prev_index = Some(it);
                                                    }
                                                }
                                                it += 1;
                                            }
                                        }

                                        if let Some(idx) = prev_index {
                                            self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[idx].clone(), idx));
                                        } else {
                                            if !self.preview_autocomplete_cmds.is_empty() {
                                                self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[0].clone(), 0));
                                            }
                                        }
                                    }
                                }

                                // handle sending command
                                if te_resp.lost_focus() {
                                    ui.input(|input| {
                                        if input.key_pressed(egui::Key::Enter) {
                                            {
                                                let debug_log = *self.debug_console_commands.lock().unwrap();

                                                self.console_text =
                                                    self.console_text.trim().to_owned();

                                                let parse_tree = {
                                                    if debug_log {
                                                        log::trace!("trying to parse command: {}", self.console_text);
                                                    }

                                                    let mut parse_trees = self
                                                        .command_grammar
                                                        .parse_input(&self.console_text);

                                                    let mut val = None;
                                                    let mut pt_count = 0;

                                                    let mut debug_msg = "== Parse Trees ==".to_owned();
                                                    while let Some(pt) = parse_trees.next() {
                                                        if debug_log {
                                                            debug_msg += &format!("\n{pt_count} =>\n{pt}");
                                                        }
                                                        val = Some(pt);
                                                        pt_count += 1;
                                                    }

                                                    if debug_log {
                                                        log::trace!("{debug_msg}");
                                                    }

                                                    if pt_count != 1 {
                                                        log::error!("ambigious command, multiple valid parse trees.");
                                                        val = None;
                                                    }

                                                    val
                                                };

                                                if let Some(pt) = parse_tree {
                                                    let mut nodes = pt.rhs_iter();
                                                    let command = expand_parse_tree_node(
                                                        nodes.next().unwrap(),
                                                    );

                                                    if debug_log {
                                                        log::trace!("Parsed Command: {command}");
                                                    }

                                                    if nodes.next().is_some() { // whitespace, args follow
                                                        let args_node = nodes.next().unwrap();
                                                        let args =
                                                            if let ParseTreeNode::Nonterminal(nt) =
                                                                args_node
                                                            {
                                                                expand_command_parameters(nt)
                                                            } else {
                                                                panic!(
                                                                    "unexpected console command parse"
                                                                );
                                                            };

                                                        if let Some(cmd) =
                                                            console_commands.get_mut(&command)
                                                        {
                                                            let mut error = false;

                                                            // collect named args, indexed args, and flags*
                                                            // *flags are actually just named values set to true
                                                            let mut named_args = BTreeMap::new();
                                                            let mut indexed_vals = VecDeque::new();
                                                            for arg in args {
                                                                if arg.0.is_empty() {
                                                                    indexed_vals.push_back(arg.1);
                                                                } else {
                                                                    let name = arg.0.clone();
                                                                    if named_args.insert(arg.0, arg.1).is_some() {
                                                                        log::error!("duplicate command parameter: {name}");
                                                                        error = true;
                                                                        break;
                                                                    }
                                                                }
                                                            }

                                                            // construct final parameters
                                                            let mut missed_defs = VecDeque::new();
                                                            let mut complete_args = BTreeMap::new();
                                                            if !error {
                                                                for def in &cmd.args {
                                                                    if let Some(value) = named_args.remove(&def.name) {
                                                                        let arg_value = parse_value_via_definition(&value, def);
                                                                        if let Some(arg_value) = arg_value {
                                                                            complete_args.insert(def.name.clone(), arg_value);
                                                                        } else {
                                                                            error = true;
                                                                            break;
                                                                        }
                                                                    } else {
                                                                        if let CallbackArgumentType::Flag = def.cba_type { // default missing flags to false
                                                                            complete_args.insert(def.name.clone(), CallbackArgumentValue::Flag(false));
                                                                        } else {
                                                                            missed_defs.push_back(def);
                                                                        }
                                                                    }
                                                                }
                                                            }

                                                            if !error { // match up any indexed_args
                                                                let lens = (indexed_vals.len(), missed_defs.len());
                                                                if lens.0 == lens.1 {
                                                                    for _ in 0..lens.0 {
                                                                        let indexed_val = indexed_vals.pop_front().unwrap();
                                                                        let missed_def = missed_defs.pop_front().unwrap();

                                                                        let arg_value = parse_value_via_definition(&indexed_val, missed_def);
                                                                        if let Some(arg_value) = arg_value {
                                                                            complete_args.insert(missed_def.name.clone(), arg_value);
                                                                        } else {
                                                                            log::error!("couldn't parse argument '{indexed_val}' with definition '{missed_def:?}'.");
                                                                            error = true;
                                                                            break;
                                                                        }
                                                                    }
                                                                } else {
                                                                    error = true;
                                                                    if lens.0 > lens.1 {
                                                                        log::error!("too many arguments.");
                                                                    } else {
                                                                        log::error!("too few arguments.");
                                                                    }
                                                                }
                                                            }

                                                            if debug_log {
                                                                log::trace!("completed arguments =>\n{complete_args:#?}");
                                                            }

                                                            if !error {
                                                                if let Err(e) = (cmd.cb)(complete_args) {
                                                                    log::error!("command error: {e}");
                                                                }
                                                            }
                                                        } else {
                                                            log::error!("command not found: {command}");
                                                        }
                                                    } else {
                                                        // no args passed
                                                        if debug_log {
                                                            log::trace!("no args passed");
                                                        }

                                                        if let Some(cmd) =
                                                            console_commands.get_mut(&command)
                                                        {
                                                            if let Err(e) = (cmd.cb)(BTreeMap::new()) {
                                                                log::error!("command error: {e}");
                                                            }
                                                        } else {
                                                            log::error!("command not found: {command}");
                                                        }
                                                    }
                                                } else {
                                                    log::error!(
                                                        "invalid console command: {}",
                                                        self.console_text
                                                    );
                                                }
                                            }

                                            self.console_text.clear();
                                            self.set_console_focus = true;
                                            self.selected_autocomplete_cmd = None;
                                            self.preview_autocomplete_cmds.clear();
                                            self.draw_preview_commands_list = false;
                                        }
                                    });
                                }
                                self.console_has_focus = te_resp.has_focus();
                            });

                        egui::CentralPanel::default()
                            .frame(egui::Frame::none())
                            .show_inside(ui, |ui| {
                                const TIMESTAMP_WIDTH: f32 = 60.0;
                                let main_width = ui.available_width() - TIMESTAMP_WIDTH;

                                ui.set_clip_rect(ui.available_rect_before_wrap());

                                egui_extras::TableBuilder::new(ui)
                                    .stick_to_bottom(true)
                                    .column(Column::exact(main_width))
                                    .column(Column::exact(TIMESTAMP_WIDTH))
                                    .auto_shrink([false, false])
                                    .min_scrolled_height(60.0)
                                    .body(|body| {
                                        const ROW_HEIGHT: f32 = 18.0;
                                        let records = logger.read().unwrap();
                                        let num_rows = records.len();

                                        body.rows(ROW_HEIGHT, num_rows, |idx, mut row| {
                                            let record = &records[idx];
                                            row.col(|ui| {
                                                // draw warn/error background bar
                                                let painter = ui.painter();
                                                let mut avail = ui.available_rect_before_wrap();
                                                *avail.top_mut() -= 7.0;
                                                *avail.bottom_mut() += 1.0;

                                                let bar_opacity = {
                                                    (self.ui_opacity * 255.0 * 0.5) as u8
                                                };

                                                match &record.level {
                                                    log::Level::Error => painter.rect_filled(
                                                        avail,
                                                        0.0,
                                                        Color32::from_rgba_unmultiplied(
                                                            64, 8, 8, bar_opacity,
                                                        ),
                                                    ),
                                                    log::Level::Warn => painter.rect_filled(
                                                            avail,
                                                        0.0,
                                                        Color32::from_rgba_unmultiplied(
                                                            64, 64, 0, bar_opacity,
                                                        ),
                                                    ),
                                                    _ => {}
                                                }

                                                let mut job = LayoutJob::default();
                                                job.wrap = TextWrapping {
                                                    max_rows: 1,
                                                    ..Default::default()
                                                };

                                                let mut format = TextFormat::default();
                                                format.color = TEXT_COLOUR;
                                                format.valign = Align::BOTTOM;
                                                format.font_id = FontId::monospace(14.0);

                                                job.append("[", 0.0, format.clone());

                                                format.color = DIM_TEXT_COLOUR;
                                                job.append(
                                                    &format!("{}", &record.idx),
                                                    0.0,
                                                    format.clone(),
                                                );

                                                format.color = TEXT_COLOUR;
                                                job.append(":", 0.0, format.clone());

                                                match &record.level {
                                                    log::Level::Debug => {
                                                        format.color = Color32::GOLD
                                                    }
                                                    log::Level::Error => {
                                                        format.color = Color32::LIGHT_RED
                                                    }
                                                    log::Level::Warn => {
                                                        format.color = Color32::LIGHT_YELLOW
                                                    }
                                                    log::Level::Info => {
                                                        format.color = Color32::LIGHT_GREEN
                                                    }
                                                    log::Level::Trace => {
                                                        format.color = Color32::LIGHT_BLUE
                                                    }
                                                }

                                                job.append(
                                                    &format!("{}", &record.level),
                                                    0.0,
                                                    format.clone(),
                                                );

                                                format.color = TEXT_COLOUR;
                                                job.append("] ", 0.0, format.clone());

                                                format.color = DIM_TEXT_COLOUR;
                                                format.italics = true;

                                                job.append(&record.debug_text, 0.0, format);

                                                if ui
                                                    .add(
                                                        egui::Label::new(job).sense(Sense::click()),
                                                    )
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    let fake_text = records[idx].text.clone();
                                                    self.record_windows.as_mut().unwrap().insert(
                                                        idx,
                                                        RecordWindow {
                                                            record: records[idx].clone(),
                                                            is_open: true,
                                                            wrap_text: false,
                                                            fake_text,
                                                        },
                                                    );
                                                }
                                            });
                                            row.col(|ui| {
                                                let time = record.local_time;
                                                ui.label(RichText::new(format!("{:02}:{:02}:{:02}",
                                                time.hour(), time.minute(), time.second())).color(OFF_ACCENT_COLOUR));
                                            });
                                        });
                                    });
                            });

                        self.last_console_window_height = ui.cursor().height();
                    });
            });
    }

    pub fn handle_platform_output(
        &mut self,
        output: egui::PlatformOutput,
        clipboard_util: &ClipboardUtil,
    ) {
        // handle clipboard
        if !output.copied_text.is_empty() {
            if let Err(e) = clipboard_util.set_clipboard_text(&output.copied_text) {
                log::error!("couldn't set clipboard text: {e}");
            }
        }

        // handle cursor
        type EguiCursor = egui::CursorIcon;
        type SdlCursor = sdl2::sys::SDL_SystemCursor;
        let sys_cursor = match output.cursor_icon {
            EguiCursor::ResizeEast
            | EguiCursor::ResizeWest
            | EguiCursor::ResizeColumn
            | EguiCursor::ResizeHorizontal => SdlCursor::SDL_SYSTEM_CURSOR_SIZEWE,
            EguiCursor::ResizeNorth
            | EguiCursor::ResizeSouth
            | EguiCursor::ResizeRow
            | EguiCursor::ResizeVertical => SdlCursor::SDL_SYSTEM_CURSOR_SIZENS,
            EguiCursor::ResizeNeSw | EguiCursor::ResizeNorthEast | EguiCursor::ResizeSouthEast => {
                SdlCursor::SDL_SYSTEM_CURSOR_SIZENESW
            }
            EguiCursor::ResizeNwSe | EguiCursor::ResizeNorthWest | EguiCursor::ResizeSouthWest => {
                SdlCursor::SDL_SYSTEM_CURSOR_SIZENWSE
            }
            EguiCursor::Move | EguiCursor::Crosshair => SdlCursor::SDL_SYSTEM_CURSOR_CROSSHAIR,
            EguiCursor::AllScroll => SdlCursor::SDL_SYSTEM_CURSOR_SIZEALL,
            EguiCursor::NoDrop | EguiCursor::NotAllowed => SdlCursor::SDL_SYSTEM_CURSOR_NO,
            EguiCursor::Progress | EguiCursor::Wait => SdlCursor::SDL_SYSTEM_CURSOR_WAIT,
            EguiCursor::Text | EguiCursor::VerticalText => SdlCursor::SDL_SYSTEM_CURSOR_IBEAM,
            EguiCursor::PointingHand => SdlCursor::SDL_SYSTEM_CURSOR_HAND,
            _ => SdlCursor::SDL_SYSTEM_CURSOR_ARROW,
        };

        unsafe {
            let new_cursor = sdl2::sys::SDL_CreateSystemCursor(sys_cursor);
            sdl2::sys::SDL_SetCursor(new_cursor);
            if let Some(old_cursor) = self.sdl_cursor.take() {
                sdl2::sys::SDL_FreeCursor(old_cursor);
            }
            self.sdl_cursor = Some(new_cursor);
        }
    }

    fn paint_primitives(
        &mut self,
        glow: &glow::Context,
        screen_size_px: (u32, u32),
        screen_scale: f32,
        clipped_primitives: Vec<egui::ClippedPrimitive>,
    ) {
        self.prepare_painting(glow, screen_size_px, screen_scale);

        for egui::ClippedPrimitive {
            clip_rect,
            primitive,
        } in clipped_primitives
        {
            self.set_clip_rect(glow, screen_size_px, screen_scale, clip_rect);

            match primitive {
                Primitive::Mesh(mesh) => {
                    self.paint_mesh(glow, mesh);
                }
                Primitive::Callback(callback) => {
                    if callback.rect.is_positive() {
                        // Transform callback rect to physical pixels:
                        let rect_min_x = screen_scale * callback.rect.min.x;
                        let rect_min_y = screen_scale * callback.rect.min.y;
                        let rect_max_x = screen_scale * callback.rect.max.x;
                        let rect_max_y = screen_scale * callback.rect.max.y;

                        let rect_min_x = rect_min_x.round() as i32;
                        let rect_min_y = rect_min_y.round() as i32;
                        let rect_max_x = rect_max_x.round() as i32;
                        let rect_max_y = rect_max_y.round() as i32;

                        unsafe {
                            glow.viewport(
                                rect_min_x,
                                screen_size_px.1 as i32 - rect_max_y,
                                rect_max_x - rect_min_x,
                                rect_max_y - rect_min_y,
                            );
                        }

                        let screen_size_px_buff: [u32; 2] = [screen_size_px.0, screen_size_px.1];
                        let info = egui::PaintCallbackInfo {
                            viewport: callback.rect,
                            clip_rect,
                            pixels_per_point: screen_scale,
                            screen_size_px: screen_size_px_buff,
                        };

                        if let Some(callback) = callback.callback.downcast_ref::<CallbackFn>() {
                            (callback.f)(info, self);
                        } else {
                            log::warn!("Warning: Unsupported render callback. Expected CallbackFn");
                        }

                        // Restore state:
                        self.prepare_painting(glow, screen_size_px, screen_scale);
                    }
                }
            }
        }
        unsafe {
            glow.bind_vertex_array(None);
            glow.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            glow.disable(glow::SCISSOR_TEST);
        }
    }

    fn paint_mesh(&mut self, glow: &glow::Context, mesh: egui::Mesh) {
        if let Some(texture) = self.textures.get(&mesh.texture_id) {
            unsafe {
                let vertices: Vec<EguiVertexPod> = mesh
                    .vertices
                    .into_iter()
                    .map(|e| EguiVertexPod::from(e))
                    .collect();
                let vertices_ref: &[u8] = bytemuck::cast_slice(vertices.as_slice());
                glow.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                glow.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertices_ref, glow::STREAM_DRAW);

                glow.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
                glow.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    bytemuck::cast_slice(&mesh.indices),
                    glow::STREAM_DRAW,
                );

                glow.bind_texture(glow::TEXTURE_2D, Some(*texture));
            }

            unsafe {
                glow.draw_elements(
                    glow::TRIANGLES,
                    mesh.indices.len() as i32,
                    glow::UNSIGNED_INT,
                    0,
                );
            }
        } else {
            log::error!("egui failed to find texture {:?}", mesh.texture_id);
        }
    }

    fn set_clip_rect(
        &self,
        glow: &glow::Context,
        size_px: (u32, u32),
        screen_scale: f32,
        clip_rect: egui::Rect,
    ) {
        // Transform clip rect to physical pixels:
        let clip_min_x = screen_scale * clip_rect.min.x;
        let clip_min_y = screen_scale * clip_rect.min.y;
        let clip_max_x = screen_scale * clip_rect.max.x;
        let clip_max_y = screen_scale * clip_rect.max.y;

        // Round to integer:
        let clip_min_x = clip_min_x.round() as i32;
        let clip_min_y = clip_min_y.round() as i32;
        let clip_max_x = clip_max_x.round() as i32;
        let clip_max_y = clip_max_y.round() as i32;

        // Clamp:
        let clip_min_x = clip_min_x.clamp(0, size_px.0 as i32);
        let clip_min_y = clip_min_y.clamp(0, size_px.1 as i32);
        let clip_max_x = clip_max_x.clamp(clip_min_x, size_px.0 as i32);
        let clip_max_y = clip_max_y.clamp(clip_min_y, size_px.1 as i32);

        unsafe {
            glow.scissor(
                clip_min_x,
                size_px.1 as i32 - clip_max_y,
                clip_max_x - clip_min_x,
                clip_max_y - clip_min_y,
            );
        }
    }

    fn prepare_painting(&mut self, glow: &glow::Context, (w, h): (u32, u32), screen_scale: f32) {
        unsafe {
            glow.enable(glow::SCISSOR_TEST);
            glow.disable(glow::CULL_FACE);
            glow.disable(glow::DEPTH_TEST);
            glow.color_mask(true, true, true, true);
            glow.enable(glow::BLEND);
            glow.blend_equation_separate(glow::FUNC_ADD, glow::FUNC_ADD);
            glow.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE_MINUS_DST_ALPHA,
                glow::ONE,
            );

            let w_pts = w as f32 / screen_scale;
            let h_pts = h as f32 / screen_scale;

            glow.viewport(0, 0, w as i32, h as i32);
            glow.use_program(Some(self.program));
            glow.uniform_2_f32(Some(&self.u_screen_size), w_pts, h_pts);
            glow.uniform_1_i32(Some(&self.u_sampler), 0);
            glow.active_texture(glow::TEXTURE0);
            glow.bind_vertex_array(Some(self.vao));
            glow.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
        }
    }

    fn upload_texture_rgb(
        &mut self,
        glow: &glow::Context,
        pos: Option<[usize; 2]>,
        [w, h]: [usize; 2],
        options: egui::TextureOptions,
        data: &[u8],
    ) {
        unsafe {
            glow.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                match options.magnification {
                    egui::TextureFilter::Linear => glow::LINEAR,
                    egui::TextureFilter::Nearest => glow::NEAREST,
                } as i32,
            );
            glow.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                match options.minification {
                    egui::TextureFilter::Linear => glow::LINEAR,
                    egui::TextureFilter::Nearest => glow::NEAREST,
                } as i32,
            );

            glow.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            glow.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            glow.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            if let Some([x, y]) = pos {
                glow.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x as _,
                    y as _,
                    w as _,
                    h as _,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(data),
                );
            } else {
                glow.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as _,
                    w as _,
                    h as _,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    Some(data),
                );
            }
        }
    }

    pub fn render(
        &mut self,
        glow: &glow::Context,
        screen_dimensions: (u32, u32),
        screen_scale: f32,
        clipped_primitives: Vec<egui::ClippedPrimitive>,
        textures_delta: egui::TexturesDelta,
    ) {
        // set textures
        for (id, delta) in textures_delta.set {
            let tex = *self
                .textures
                .entry(id)
                .or_insert_with(|| unsafe { glow.create_texture().unwrap() });
            unsafe {
                glow.bind_texture(glow::TEXTURE_2D, Some(tex));
            }
            match &delta.image {
                egui::ImageData::Color(image) => {
                    let data: Vec<EguiColor32Pod> = image
                        .pixels
                        .iter()
                        .map(|e| EguiColor32Pod::from(*e))
                        .collect();

                    let data_ref: &[u8] = bytemuck::cast_slice(data.as_slice());
                    self.upload_texture_rgb(glow, delta.pos, image.size, delta.options, data_ref);
                }
                egui::ImageData::Font(image) => {
                    let data: Vec<u8> = image
                        .srgba_pixels(None)
                        .flat_map(|a| a.to_array())
                        .collect();
                    self.upload_texture_rgb(glow, delta.pos, image.size, delta.options, &data);
                }
            }
        }

        self.paint_primitives(glow, screen_dimensions, screen_scale, clipped_primitives);

        // free textures
        for id in textures_delta.free {
            if let Some(tex) = self.textures.remove(&id) {
                unsafe {
                    glow.delete_texture(tex);
                }
            }
        }
    }

    fn fire_egui_events(
        &mut self,
        sdl_events: &Vec<sdl2::event::Event>,
        clipboard_util: &ClipboardUtil,
    ) {
        let egui_modifiers = self.modifiers.get_modifiers();
        self.input.modifiers = egui_modifiers;

        for event in sdl_events {
            match event {
                sdl2::event::Event::MouseButtonDown {
                    timestamp: _,
                    window_id: _,
                    which: _,
                    mouse_btn,
                    clicks: _,
                    x,
                    y,
                } => {
                    if *mouse_btn == sdl2::mouse::MouseButton::Unknown {
                        log::warn!("egui debug ui unknown mouse button");
                        continue;
                    }

                    let pos = egui::pos2(*x as f32 / self.ui_scale, *y as f32 / self.ui_scale);
                    self.input.events.push(egui::Event::PointerButton {
                        pos,
                        button: match mouse_btn {
                            sdl2::mouse::MouseButton::Left => egui::PointerButton::Primary,
                            sdl2::mouse::MouseButton::Right => egui::PointerButton::Secondary,
                            sdl2::mouse::MouseButton::Middle => egui::PointerButton::Middle,
                            sdl2::mouse::MouseButton::X1 => egui::PointerButton::Extra1,
                            sdl2::mouse::MouseButton::X2 => egui::PointerButton::Extra2,
                            sdl2::mouse::MouseButton::Unknown => panic!(),
                        },
                        pressed: true,
                        modifiers: egui_modifiers,
                    });
                }
                sdl2::event::Event::MouseButtonUp {
                    timestamp: _,
                    window_id: _,
                    which: _,
                    mouse_btn,
                    clicks: _,
                    x,
                    y,
                } => {
                    if *mouse_btn == sdl2::mouse::MouseButton::Unknown {
                        continue;
                    }

                    let pos = egui::pos2(*x as f32 / self.ui_scale, *y as f32 / self.ui_scale);
                    self.input.events.push(egui::Event::PointerButton {
                        pos,
                        button: match mouse_btn {
                            sdl2::mouse::MouseButton::Left => egui::PointerButton::Primary,
                            sdl2::mouse::MouseButton::Right => egui::PointerButton::Secondary,
                            sdl2::mouse::MouseButton::Middle => egui::PointerButton::Middle,
                            sdl2::mouse::MouseButton::X1 => egui::PointerButton::Extra1,
                            sdl2::mouse::MouseButton::X2 => egui::PointerButton::Extra2,
                            sdl2::mouse::MouseButton::Unknown => panic!(),
                        },
                        pressed: false,
                        modifiers: egui_modifiers,
                    });
                }
                sdl2::event::Event::MouseMotion {
                    timestamp: _,
                    window_id: _,
                    which: _,
                    mousestate: _,
                    x,
                    y,
                    xrel: _,
                    yrel: _,
                } => {
                    self.mouse_pos = egui::pos2(*x as f32, *y as f32);
                    let tf_mouse_pos =
                        egui::pos2(*x as f32 / self.ui_scale, *y as f32 / self.ui_scale);
                    self.input
                        .events
                        .push(egui::Event::PointerMoved(tf_mouse_pos));
                }
                sdl2::event::Event::MouseWheel {
                    timestamp: _,
                    window_id: _,
                    which: _,
                    x,
                    y,
                    direction: _,
                } => {
                    if self.modifiers.control {
                        self.input.events.push(egui::Event::Zoom(*y as f32));
                    } else {
                        let scroll: egui::Vec2 = SCROLL_SCALE * egui::vec2(-*x as f32, *y as f32);
                        self.input.events.push(egui::Event::Scroll(scroll));
                    }
                }
                sdl2::event::Event::KeyDown {
                    timestamp: _,
                    window_id: _,
                    keycode,
                    scancode: _,
                    keymod: _,
                    repeat,
                } => {
                    if let Some(kc) = keycode {
                        match kc {
                            sdl2::keyboard::Keycode::LShift | sdl2::keyboard::Keycode::RShift => {
                                self.modifiers.set_shift()
                            }
                            sdl2::keyboard::Keycode::LCtrl | sdl2::keyboard::Keycode::RCtrl => {
                                self.modifiers.set_control()
                            }
                            sdl2::keyboard::Keycode::LAlt | sdl2::keyboard::Keycode::RAlt => {
                                self.modifiers.set_alt()
                            }
                            sdl2::keyboard::Keycode::X => {
                                if self.modifiers.control {
                                    self.input.events.push(egui::Event::Cut);
                                } else {
                                    self.input.events.push(egui::Event::Key {
                                        key: egui::Key::X,
                                        pressed: true,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                            sdl2::keyboard::Keycode::C => {
                                if self.modifiers.control {
                                    self.input.events.push(egui::Event::Copy);
                                } else {
                                    self.input.events.push(egui::Event::Key {
                                        key: egui::Key::C,
                                        pressed: true,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                            sdl2::keyboard::Keycode::V => {
                                if self.modifiers.control {
                                    self.input.events.push(egui::Event::Paste({
                                        match clipboard_util.clipboard_text() {
                                            Ok(x) => x.clone(),
                                            Err(e) => {
                                                log::error!("couldn't get clipboard text: {e}");
                                                "".to_owned()
                                            }
                                        }
                                    }));
                                } else {
                                    self.input.events.push(egui::Event::Key {
                                        key: egui::Key::V,
                                        pressed: true,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                            sdl2::keyboard::Keycode::D => {
                                if self.modifiers.control && self.console_has_focus {
                                    self.delete_console_text = true;
                                } else {
                                    self.input.events.push(egui::Event::Key {
                                        key: egui::Key::D,
                                        pressed: true,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                            kc => {
                                if let Some(ekey) = sdl_keycode_to_egui_key(kc) {
                                    self.input.events.push(egui::Event::Key {
                                        key: ekey,
                                        pressed: true,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                        }
                    }
                }
                sdl2::event::Event::KeyUp {
                    timestamp: _,
                    window_id: _,
                    keycode,
                    scancode: _,
                    keymod: _,
                    repeat,
                } => {
                    if let Some(kc) = keycode {
                        match kc {
                            sdl2::keyboard::Keycode::LShift | sdl2::keyboard::Keycode::RShift => {
                                self.modifiers.unset_shift()
                            }
                            sdl2::keyboard::Keycode::LCtrl | sdl2::keyboard::Keycode::RCtrl => {
                                self.modifiers.unset_control()
                            }
                            sdl2::keyboard::Keycode::LAlt | sdl2::keyboard::Keycode::RAlt => {
                                self.modifiers.unset_alt()
                            }
                            kc => {
                                if let Some(ekey) = sdl_keycode_to_egui_key(kc) {
                                    self.input.events.push(egui::Event::Key {
                                        key: ekey,
                                        pressed: false,
                                        repeat: *repeat,
                                        modifiers: egui_modifiers,
                                    });
                                }
                            }
                        }
                    }
                }
                sdl2::event::Event::TextInput {
                    timestamp: _,
                    window_id: _,
                    text,
                } => {
                    self.input.events.push(egui::Event::Text(text.clone()));
                }
                _ => {}
            }
        }
    }
}

struct ModifierTracker {
    shift: bool,
    control: bool,
    alt: bool,
}
impl ModifierTracker {
    pub fn new() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
        }
    }
    pub fn get_modifiers(&self) -> egui::Modifiers {
        let mut mods = egui::Modifiers::NONE;
        if self.shift {
            mods = mods.plus(egui::Modifiers::SHIFT);
        }
        if self.control {
            mods = mods
                .plus(egui::Modifiers::CTRL)
                .plus(egui::Modifiers::COMMAND);
        }
        if self.alt {
            mods = mods.plus(egui::Modifiers::ALT);
        }
        mods
    }
    pub fn set_shift(&mut self) {
        self.shift = true;
    }
    pub fn unset_shift(&mut self) {
        self.shift = false;
    }
    pub fn set_control(&mut self) {
        self.control = true;
    }
    pub fn unset_control(&mut self) {
        self.control = false;
    }
    pub fn set_alt(&mut self) {
        self.alt = true;
    }
    pub fn unset_alt(&mut self) {
        self.alt = false;
    }
}

pub struct DebugConsoleLogger {
    records: Arc<RwLock<Vec<DebugLogRecord>>>,
}
impl DebugConsoleLogger {
    pub fn new() -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn get_shared(&self) -> Arc<RwLock<Vec<DebugLogRecord>>> {
        self.records.clone()
    }
}
impl log::Log for DebugConsoleLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn flush(&self) {}

    fn log(&self, record: &log::Record) {
        let mut records = self.records.write().unwrap();
        let idx = records.len();
        let text = record.args().to_string();
        let debug_text: String = text.clone().replace("\r\n", "\n").replace("\n", "\\n");
        records.push(DebugLogRecord {
            idx,
            debug_text,
            text,
            level: record.level(),
            file: record
                .file()
                .and_then(|f| Some(f.to_string()))
                .unwrap_or_default(),
            line: record.line().unwrap_or_default(),
            module: record
                .module_path()
                .and_then(|p| Some(p.to_string()))
                .unwrap_or_default(),
            target: record.target().to_string(),
            local_time: OffsetDateTime::now_local()
                .map_err(|e| {
                    log::error!("couldn't get local time: {e}");
                })
                .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        });
    }
}

fn debug_ui_offset_date_time_format(time: &OffsetDateTime) -> String {
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z{}",
        time.year(),
        time.month() as i32,
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.offset().whole_hours(),
    )
}

fn sdl_keycode_to_egui_key(keycode: &sdl2::keyboard::Keycode) -> Option<egui::Key> {
    // while I could do some range mapping and treat these enums as integers (last I checked they are),
    // I'm going to assume that they're not and instead explicitly create the mappings.
    // todo: if speed becomes an issue here, optimize this to integers
    type A = sdl2::keyboard::Keycode;
    type B = egui::Key;
    match keycode {
        A::A => Some(B::A),
        A::B => Some(B::B),
        A::C => Some(B::C),
        A::D => Some(B::D),
        A::E => Some(B::E),
        A::F => Some(B::F),
        A::G => Some(B::G),
        A::H => Some(B::H),
        A::I => Some(B::I),
        A::J => Some(B::J),
        A::K => Some(B::K),
        A::L => Some(B::L),
        A::M => Some(B::M),
        A::N => Some(B::N),
        A::O => Some(B::O),
        A::P => Some(B::P),
        A::Q => Some(B::Q),
        A::R => Some(B::R),
        A::S => Some(B::S),
        A::T => Some(B::T),
        A::U => Some(B::U),
        A::V => Some(B::V),
        A::W => Some(B::W),
        A::X => Some(B::X),
        A::Y => Some(B::Y),
        A::Z => Some(B::Z),
        A::Num0 => Some(B::Num0),
        A::Num1 => Some(B::Num1),
        A::Num2 => Some(B::Num2),
        A::Num3 => Some(B::Num3),
        A::Num4 => Some(B::Num4),
        A::Num5 => Some(B::Num5),
        A::Num6 => Some(B::Num6),
        A::Num7 => Some(B::Num7),
        A::Num8 => Some(B::Num8),
        A::Num9 => Some(B::Num9),
        A::F1 => Some(B::F1),
        A::F2 => Some(B::F2),
        A::F3 => Some(B::F3),
        A::F4 => Some(B::F4),
        A::F5 => Some(B::F5),
        A::F6 => Some(B::F6),
        A::F7 => Some(B::F7),
        A::F8 => Some(B::F8),
        A::F9 => Some(B::F9),
        A::F10 => Some(B::F10),
        A::F11 => Some(B::F11),
        A::F12 => Some(B::F12),
        A::F13 => Some(B::F13),
        A::F14 => Some(B::F14),
        A::F15 => Some(B::F15),
        A::F16 => Some(B::F16),
        A::F17 => Some(B::F17),
        A::F18 => Some(B::F18),
        A::F19 => Some(B::F19),
        A::F20 => Some(B::F20),
        A::Up => Some(B::ArrowUp),
        A::Down => Some(B::ArrowDown),
        A::Left => Some(B::ArrowLeft),
        A::Right => Some(B::ArrowRight),
        A::PageUp => Some(B::PageUp),
        A::PageDown => Some(B::PageDown),
        A::AltErase | A::Backspace => Some(B::Backspace),
        A::Delete => Some(B::Delete),
        A::End => Some(B::End),
        A::Return | A::Return2 | A::KpEnter => Some(B::Enter),
        A::Home => Some(B::Home),
        A::Insert => Some(B::Insert),
        A::Escape => Some(B::Escape),
        A::Minus => Some(B::Minus),
        A::Plus => Some(B::PlusEquals),
        A::Space => Some(B::Space),
        A::Tab => Some(B::Tab),
        _ => None,
    }
}

#[derive(Clone)]
pub struct DebugLogRecord {
    idx: usize,
    debug_text: String,
    text: String,
    level: log::Level,
    file: String,
    line: u32,
    module: String,
    target: String,
    local_time: time::OffsetDateTime,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct EguiVertexPod {
    pos: egui::Pos2,
    uv: egui::Pos2,
    colour: egui::Color32,
}
unsafe impl Pod for EguiVertexPod {}
unsafe impl Zeroable for EguiVertexPod {}
impl From<egui::epaint::Vertex> for EguiVertexPod {
    fn from(value: egui::epaint::Vertex) -> Self {
        Self {
            pos: value.pos,
            uv: value.uv,
            colour: value.color,
        }
    }
}
impl Default for EguiVertexPod {
    fn default() -> Self {
        Self {
            pos: egui::Pos2::default(),
            uv: egui::Pos2::default(),
            colour: egui::Color32::default(),
        }
    }
}

pub struct CallbackFn {
    f: Box<dyn Fn(PaintCallbackInfo, &EguiDebugUi) + Sync + Send>,
}

impl CallbackFn {
    pub fn new<F: Fn(PaintCallbackInfo, &EguiDebugUi) + Sync + Send + 'static>(
        callback: F,
    ) -> Self {
        let f = Box::new(callback);
        CallbackFn { f }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct EguiColor32Pod {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
}
unsafe impl Pod for EguiColor32Pod {}
unsafe impl Zeroable for EguiColor32Pod {}
impl From<egui::Color32> for EguiColor32Pod {
    fn from(value: egui::Color32) -> Self {
        let (a, b, c, d) = value.to_tuple();
        Self { a, b, c, d }
    }
}

fn expand_parse_tree_node(node: &ParseTreeNode) -> String {
    let mut val = "".to_owned();

    match node {
        ParseTreeNode::Nonterminal(nt) => {
            if nt.lhs.to_string() == "<escape_char>" {
                let mut nodes = nt.rhs_iter();
                nodes.next().unwrap(); // "\"
                let c = expand_parse_tree_node(nodes.next().unwrap());

                if c == "\"" {
                    val += "\"";
                } else if c == "\r" {
                    val += "\r";
                } else if c == "\n" {
                    val += "\n";
                } else if c == "\t" {
                    val += "\t";
                } else if c == "\\" {
                    val += "\\";
                } else {
                    log::warn!("unrecognized escape sequence: \\{c}");
                    val += &c;
                }
            } else {
                let mut rhs = nt.rhs_iter();
                while let Some(x) = rhs.next() {
                    val += expand_parse_tree_node(x).as_str();
                }
            }
        }
        ParseTreeNode::Terminal(t) => {
            val += t;
        }
    }
    val
}

fn expand_command_parameters(tree: &ParseTree) -> Vec<(String, String)> {
    let mut params = Vec::new();

    let mut nodes = tree.rhs_iter();
    let first = nodes.next().unwrap();

    if let ParseTreeNode::Nonterminal(nt) = first {
        // <command_parameters> <ws_plus> <command_param> | <command_param>
        if nt.lhs.to_string() == "<command_param>" {
            params.push(expand_command_param(nt));
        } else {
            let mut x = expand_command_parameters(nt);
            params.append(&mut x);

            nodes.next().unwrap(); // <ws_plus>

            let command_param = nodes.next().unwrap();
            if let ParseTreeNode::Nonterminal(x) = command_param {
                params.push(expand_command_param(x));
            } else {
                panic!("unexpected console command parse");
            }
        }
    }

    params
}

fn expand_command_param(command_param: &ParseTree) -> (String, String) {
    let param_type_node = command_param.rhs_iter().next().unwrap();
    if let ParseTreeNode::Nonterminal(nt) = param_type_node {
        let nt_name = nt.lhs.to_string();
        if nt_name == "<name_value_pair>" {
            expand_command_param_name_value_pair(nt)
        } else if nt_name == "<flag>" {
            expand_command_param_flag(nt)
        } else if nt_name == "<indexed_value>" {
            expand_command_param_indexed_value(nt)
        } else {
            panic!(
                "unexpected console command parse, unknown <value> non-terminal: {}",
                nt.lhs
            );
        }
    } else {
        panic!("unexpected console command parse");
    }
}

fn expand_command_param_indexed_value(parse_tree: &ParseTree) -> (String, String) {
    let node = parse_tree.rhs_iter().next().unwrap();
    let indexed_tree = if let ParseTreeNode::Nonterminal(nt) = node {
        nt
    } else {
        panic!("unexpected console command parse");
    };
    ("".to_owned(), expand_command_param_value(indexed_tree))
}

fn expand_command_param_flag(parse_tree: &ParseTree) -> (String, String) {
    let mut node = parse_tree.rhs_iter();
    node.next().unwrap(); // --
    (
        expand_parse_tree_node(node.next().unwrap()),
        "true".to_owned(),
    )
}

fn expand_command_param_name_value_pair(parse_tree: &ParseTree) -> (String, String) {
    let mut param_nodes = parse_tree.rhs_iter();
    let name = expand_parse_tree_node(param_nodes.next().unwrap());

    param_nodes.next().unwrap(); // <ws_star>
    param_nodes.next().unwrap(); // ":"
    param_nodes.next().unwrap(); // <ws_star>

    let value_node = param_nodes.next().unwrap();
    let value = if let ParseTreeNode::Nonterminal(nt) = value_node {
        expand_command_param_value(nt)
    } else {
        panic!("unexpected console command parse");
    };

    (name, value)
}

fn expand_command_param_value(parse_tree: &ParseTree) -> String {
    let mut value_nodes = parse_tree.rhs_iter();
    let first = value_nodes.next().unwrap();

    match first {
        ParseTreeNode::Nonterminal(_) => {
            // <string_implicit>
            expand_parse_tree_node(first)
        }
        ParseTreeNode::Terminal(t) => {
            if *t == "\"\"" {
                "".to_owned()
            } else {
                let string_explicit = value_nodes.next().unwrap();
                expand_parse_tree_node(string_explicit)
            }
        }
    }
}

fn parse_value_via_definition(
    value: &String,
    def: &CallbackArgumentDefinition,
) -> Option<CallbackArgumentValue> {
    match def.cba_type {
        CallbackArgumentType::Int32 => match value.parse::<i32>() {
            Ok(x) => Some(CallbackArgumentValue::Int32(x)),
            Err(e) => {
                log::error!("couldn't parse argument '{}' as a valid i32: {e}", def.name);
                None
            }
        },
        CallbackArgumentType::Int64 => match value.parse::<i64>() {
            Ok(x) => Some(CallbackArgumentValue::Int64(x)),
            Err(e) => {
                log::error!("couldn't parse argument '{}' as a valid i64: {e}", def.name);
                None
            }
        },
        CallbackArgumentType::Float32 => match value.parse::<f32>() {
            Ok(x) => Some(CallbackArgumentValue::Float32(x)),
            Err(e) => {
                log::error!("couldn't parse argument '{}' as a valid f32: {e}", def.name);
                None
            }
        },
        CallbackArgumentType::Float64 => match value.parse::<f64>() {
            Ok(x) => Some(CallbackArgumentValue::Float64(x)),
            Err(e) => {
                log::error!("couldn't parse argument '{}' as a valid f64: {e}", def.name);
                None
            }
        },
        CallbackArgumentType::String => Some(CallbackArgumentValue::String(value.clone())),
        CallbackArgumentType::Bool => match value.parse::<bool>() {
            Ok(x) => Some(CallbackArgumentValue::Bool(x)),
            Err(e) => {
                if value.len() == 1 {
                    if value.starts_with("0") {
                        return Some(CallbackArgumentValue::Bool(false));
                    } else if value.starts_with("1") {
                        return Some(CallbackArgumentValue::Bool(true));
                    }
                }
                log::error!(
                    "couldn't parse argument '{}' as a valid bool: {e}",
                    def.name
                );
                None
            }
        },
        CallbackArgumentType::Flag => Some(CallbackArgumentValue::Flag(true)),
    }
}
