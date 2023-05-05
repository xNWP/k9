use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, RwLock},
};

use bnf::{ParseTree, ParseTreeNode};
use egui::{
    epaint::text::{cursor::RCursor, TextWrapping},
    text::LayoutJob,
    Align, Color32, FontId, Frame, RichText, Sense, TextFormat,
};
use egui_extras::Column;
use k9_proc_macros::console_command_internal;
use time::OffsetDateTime;

use super::{DIM_TEXT_COLOUR, OFF_ACCENT_COLOUR, OFF_BG_COLOUR, TEXT_COLOUR};

type Flag = bool;
pub(super) struct DebugConsole {
    console_text: String,
    record_windows: Option<BTreeMap<usize, RecordWindow>>,
    pub set_console_focus: Flag,
    console_has_focus: bool,
    command_grammar: bnf::Grammar,
    debug_console_commands: Arc<Mutex<bool>>,
    selected_autocomplete_cmd: Option<(String, usize)>,
    preview_autocomplete_cmds: Vec<String>,
    draw_preview_commands_list: bool,
    last_console_window_height: f32,
    console_commands: BTreeMap<String, ConsoleCommand>,
    debug_windows: BTreeMap<String, (bool, Box<dyn DebugUiWindow>)>,
    last_cursor_idx: usize,
}
impl DebugConsole {
    pub fn new(
        mut console_commands: BTreeMap<String, ConsoleCommand>,
        debug_windows: BTreeMap<String, Box<dyn DebugUiWindow>>,
    ) -> Self {
        const GRAMMAR: &'static str = include_str!("../console_command.bnf");
        let command_grammar: bnf::Grammar = GRAMMAR.parse().unwrap();

        // setup some console commands
        let debug_console_commands = Arc::new(Mutex::new(false));
        {
            let val = debug_console_commands.clone();
            let cc_debug_console_command =
                console_command_internal!(
                    "displays debug information about the parsing run for a console command.",
                    { value: bool },
                    |ccf, value| {
                        *val.lock().unwrap() = value;
                        Ok(())
                    }
                );
            console_commands
                .entry("k9_debug_console_command".to_owned())
                .and_modify(|_| {
                    log::warn!("console command 'k9_debug_console_command' was overwritten.")
                })
                .or_insert(cc_debug_console_command);
        }

        Self {
            command_grammar,
            console_commands,
            console_has_focus: false,
            console_text: "".to_owned(),
            debug_console_commands,
            debug_windows: debug_windows
                .into_iter()
                .map(|(name, wnd)| (name, (false, wnd)))
                .collect(),
            draw_preview_commands_list: false,
            last_console_window_height: 0.0,
            preview_autocomplete_cmds: Vec::new(),
            record_windows: Some(BTreeMap::new()),
            selected_autocomplete_cmd: None,
            set_console_focus: false,
            last_cursor_idx: 0,
        }
    }

    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        logger: &Arc<RwLock<Vec<DebugLogRecord>>>,
        ui_opacity: f32,
    ) {
        // draw log record windows
        let record_wnds = self.record_windows.take().unwrap();
        let mut keep_wnds = BTreeMap::new();

        for (idx, mut wnd) in record_wnds {
            if wnd.is_open {
                egui::Window::new(format!("Debug Record #{}", wnd.record.idx))
                    .open(&mut wnd.is_open)
                    .default_size([320.0, 0.0])
                    .min_height(210.0)
                    .show(ui.ctx(), |ui| {
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
                                                ui.label(format!(
                                                    "{}",
                                                    debug_ui_offset_date_time_format(
                                                        &wnd.record.local_time
                                                    )
                                                ));
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
                                let w = ui.available_width();
                                egui::ScrollArea::both()
                                    .id_source("log_message_scroll_area")
                                    .auto_shrink([false, false])
                                    .min_scrolled_height(60.0)
                                    .show(ui, |ui| {
                                        if egui::TextEdit::multiline(&mut wnd.fake_text)
                                            .frame(false)
                                            .layouter(&mut |ui, text, _| {
                                                let mut lj = LayoutJob::default();
                                                let mut wrapping = TextWrapping::default();
                                                wrapping.max_width =
                                                    if wnd.wrap_text { w } else { f32::INFINITY };
                                                lj.wrap = wrapping;
                                                lj.append(
                                                    text,
                                                    0.0,
                                                    TextFormat::simple(
                                                        FontId::monospace(12.0),
                                                        TEXT_COLOUR,
                                                    ),
                                                );
                                                ui.fonts(|f| f.layout_job(lj))
                                            })
                                            .code_editor()
                                            .show(ui)
                                            .response
                                            .changed()
                                        {
                                            // fake text lets us keep this selectable
                                            // but keep it immutable... it's not a perfect solution
                                            // as the text can be changed for a frame or two, but it works
                                            wnd.fake_text = wnd.record.text.clone();
                                        }
                                        wnd.wrap_text
                                    });
                            });
                    });

                keep_wnds.insert(idx, wnd);
            }
        }

        self.record_windows = Some(keep_wnds);

        // draw debug windows
        for (_, (is_open, wnd)) in &mut self.debug_windows {
            if *is_open {
                egui::Window::new("aaaaaa").show(ui.ctx(), |ui| {
                    wnd.draw(ui);
                });
            }
        }

        // draw console
        egui::Window::new("k9 console")
            .default_size([640.0, 320.0])
            .min_height(100.0)
            .show(ui.ctx(), |ui| {
                egui::TopBottomPanel::bottom("k9_console_text_entry_panel")
                    .frame(egui::Frame::none())
                    .show_inside(ui, |ui| {
                        // handle autocomplete tab/right arrow complete
                        ui.input_mut(|input| {
                            // ArrowRight autocomplete
                            if input.key_pressed(egui::Key::ArrowRight) {
                                if let Some((cmd_text, _)) = &self.selected_autocomplete_cmd {
                                    if self.last_cursor_idx == self.console_text.len() { // at end
                                        self.console_text = cmd_text.clone();
                                        self.preview_autocomplete_cmds.clear();
                                        self.draw_preview_commands_list = false;
    
                                        input.events.push(egui::Event::Key {
                                            key: egui::Key::ArrowRight,
                                            pressed: true,
                                            repeat: false,
                                            modifiers: egui::Modifiers::CTRL,
                                        });
                                    }
                                }
                            }

                            // Ctrl+D delete
                            if input.consume_key(egui::Modifiers::CTRL, egui::Key::D) {
                                self.console_text.clear();
                                self.selected_autocomplete_cmd = None;
                                self.draw_preview_commands_list = false;
                                self.preview_autocomplete_cmds.clear();
                            }

                            // Tab autocomplete
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                                if self.console_has_focus {
                                    if self.selected_autocomplete_cmd.is_none() && !self.preview_autocomplete_cmds.is_empty() {
                                        log::debug!("probably never runs, TODO: get rid of this if this is true");
                                        self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[0].clone(), 0));
                                    } else if self.preview_autocomplete_cmds.len() == 1 {
                                        self.console_text = self.preview_autocomplete_cmds[0].clone();
                                        self.draw_preview_commands_list = false;
                                        self.preview_autocomplete_cmds.clear();

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

                        if let Some(crange) = te_output.cursor_range {
                            self.last_cursor_idx = crange.primary.ccursor.index;
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
                                    (ui_opacity * 255.0) as u8,
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
                                for cmd in self.console_commands.iter() {
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
                                                    self.console_commands.get_mut(&command)
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
                                                    let mut missed_mandatory = VecDeque::new();
                                                    let mut missed_optional = VecDeque::new();
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
                                                                    if def.optional {
                                                                        missed_optional.push_back(def);
                                                                    } else {
                                                                        missed_mandatory.push_back(def);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    
                                                    if !error { // match up any indexed_args
                                                        let mandatory_len = missed_mandatory.len();
                                                        let mut missed_defs = missed_mandatory;
                                                        missed_defs.append(&mut missed_optional);
                                                        let total_len = missed_defs.len();

                                                        let indexed_len = indexed_vals.len();
                                                        if indexed_len >= mandatory_len {
                                                            for _ in 0..indexed_len {
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
                                                            if indexed_len > total_len {
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
                                                        if let Err(e) = (cmd.cb)(ConsoleCommandInterface { debug_windows: &mut self.debug_windows }, complete_args) {
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
                                                    self.console_commands.get_mut(&command)
                                                {
                                                    if let Err(e) = (cmd.cb)(ConsoleCommandInterface { debug_windows: &mut self.debug_windows }, BTreeMap::new()) {
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

                        // handle up/down key navigation logic, includes autocomplete logic and history logic
                        ui.input_mut(|input| {
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                                if let Some((_, it)) = &self.selected_autocomplete_cmd {
                                    if *it == 0 {
                                        let len = self.preview_autocomplete_cmds.len();
                                        self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[len - 1].clone(), len - 1));
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
                                        self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[0].clone(), 0));
                                    } else {
                                        self.selected_autocomplete_cmd = Some((self.preview_autocomplete_cmds[it + 1].clone(), it + 1));
                                    }
                                } else {
                                    // todo: history
                                }
                            }
                        });
                    });

                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show_inside(ui, |ui| {
                        const TIMESTAMP_WIDTH: f32 = 64.0;
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
                                            (ui_opacity * 255.0 * 0.5) as u8
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
    }
}

pub struct ConsoleCommand {
    cb: Box<
        dyn FnMut(
                ConsoleCommandInterface,
                BTreeMap<String, CallbackArgumentValue>,
            ) -> Result<(), String>
            + 'static,
    >,
    args: Vec<CallbackArgumentDefinition>,
    description: String,
}

#[derive(Debug)]
pub struct CallbackArgumentDefinition {
    pub name: String,
    pub cba_type: CallbackArgumentType,
    pub optional: bool,
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

impl ConsoleCommand {
    pub fn new(
        cb: impl FnMut(
                ConsoleCommandInterface,
                BTreeMap<String, CallbackArgumentValue>,
            ) -> Result<(), String>
            + 'static,
        args: Vec<CallbackArgumentDefinition>,
        description: String,
    ) -> Self {
        Self {
            cb: Box::new(cb),
            args,
            description,
        }
    }
}

pub struct ConsoleCommandInterface<'a> {
    debug_windows: &'a mut BTreeMap<String, (bool, Box<dyn DebugUiWindow>)>,
}
impl<'a> ConsoleCommandInterface<'a> {
    pub fn open_debug_window(&mut self, id: &String) -> bool {
        if let Some((is_open, _)) = self.debug_windows.get_mut(id) {
            *is_open = true;
            true
        } else {
            false
        }
    }
    pub fn close_debug_window(&mut self, id: &String) -> bool {
        if let Some((is_open, _)) = self.debug_windows.get_mut(id) {
            *is_open = false;
            true
        } else {
            false
        }
    }
    pub fn set_open_debug_window(&mut self, id: &String, set_open: bool) -> bool {
        if let Some((is_open, _)) = self.debug_windows.get_mut(id) {
            *is_open = set_open;
            true
        } else {
            false
        }
    }
}

pub trait DebugUiWindow {
    fn draw(&mut self, ui: &mut egui::Ui);
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

struct RecordWindow {
    record: DebugLogRecord,
    is_open: bool,
    wrap_text: bool,
    fake_text: String,
}
