use std::{time::Instant, collections::BTreeMap};

use bytemuck::{offset_of, Pod, Zeroable};
use egui::{PaintCallbackInfo, epaint::Primitive};
use glow::HasContext;
use sdl2::clipboard::ClipboardUtil;


const SCROLL_SCALE: f32 = 20.0;


pub(super) struct EguiRenderCore {
    pub(super) ctx: egui::Context,
    input: egui::RawInput,
    modifiers: ModifierTracker,
    start_time: Instant,
    textures: BTreeMap<egui::TextureId, glow::NativeTexture>,
    program: glow::NativeProgram,
    u_screen_size: glow::NativeUniformLocation,
    u_sampler: glow::NativeUniformLocation,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    ebo: glow::NativeBuffer,
    sdl_cursor: Option<*mut sdl2::sys::SDL_Cursor>,
}
impl EguiRenderCore {
    pub fn new(glow: &glow::Context, default_ppt: f32) -> Self {
        let ctx = egui::Context::default();
        let input = egui::RawInput::default();
        let modifiers = ModifierTracker::new();

        ctx.set_pixels_per_point(default_ppt);

        unsafe {
            let vert_shader = match glow.create_shader(glow::VERTEX_SHADER) {
                Ok(x) => x,
                Err(e) => {
                    panic!("failed to create egui debug ui vert shader: {e}");
                }
            };
            const VERT_SRC: &'static str = include_str!("../k9_egui_debug_ui.vert.glsl");

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
            const FRAG_SRC: &'static str = include_str!("../k9_egui_debug_ui.frag.glsl");

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

            Self {
                ctx,
                input,
                modifiers,
                start_time: Instant::now(),
                textures: BTreeMap::new(),
                program,
                u_screen_size,
                u_sampler,
                vao,
                vbo,
                ebo,
                sdl_cursor: None,
            }
        }
    }

    pub fn begin_frame(
        &mut self,
        window_has_focus: bool,
        sdl_events: &Vec<sdl2::event::Event>,
        screen_dimensions: (u32, u32),
        clipboard_util: &ClipboardUtil,
    ) {
        let ppt = self.ctx.pixels_per_point();
        self.input.time = Some(self.start_time.elapsed().as_secs_f64());
        self.input.has_focus = window_has_focus;
        let w_pts = screen_dimensions.0 as f32 / ppt; // changes scale
        let h_pts = screen_dimensions.1 as f32 / ppt; // changes scale
        self.input.screen_rect = Some(egui::Rect::from_two_pos(
            egui::pos2(0.0, 0.0),
            egui::pos2(w_pts, h_pts),
        ));
        self.input.pixels_per_point = Some(ppt); // changes draw res

        self.fire_egui_events(&sdl_events, clipboard_util, ppt); // changes input mapping

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


    fn fire_egui_events(
        &mut self,
        sdl_events: &Vec<sdl2::event::Event>,
        clipboard_util: &ClipboardUtil,
        ppt: f32,
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

                    let pos = egui::pos2(*x as f32 / ppt, *y as f32 / ppt);

                    let button = match mouse_btn {
                            sdl2::mouse::MouseButton::Left => Some(egui::PointerButton::Primary),
                            sdl2::mouse::MouseButton::Right => Some(egui::PointerButton::Secondary),
                            sdl2::mouse::MouseButton::Middle => Some(egui::PointerButton::Middle),
                            sdl2::mouse::MouseButton::X1 => Some(egui::PointerButton::Extra1),
                            sdl2::mouse::MouseButton::X2 => Some(egui::PointerButton::Extra2),
                            sdl2::mouse::MouseButton::Unknown => {
                                log::warn!("unknown mouse button pressed");
                                None
                            }
                    };

                    if let Some(butt) = button {
                        self.input.events.push(egui::Event::PointerButton {
                            pos,
                            button: butt,
                            pressed: true,
                            modifiers: egui_modifiers,
                        });
                    }
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

                    let pos = egui::pos2(*x as f32 / ppt, *y as f32 / ppt);
                    let button = match mouse_btn {
                        sdl2::mouse::MouseButton::Left => Some(egui::PointerButton::Primary),
                        sdl2::mouse::MouseButton::Right => Some(egui::PointerButton::Secondary),
                        sdl2::mouse::MouseButton::Middle => Some(egui::PointerButton::Middle),
                        sdl2::mouse::MouseButton::X1 => Some(egui::PointerButton::Extra1),
                        sdl2::mouse::MouseButton::X2 => Some(egui::PointerButton::Extra2),
                        sdl2::mouse::MouseButton::Unknown => {
                            log::warn!("unknown mouse button pressed");
                            None
                        }
                };

                if let Some(butt) = button {
                    self.input.events.push(egui::Event::PointerButton {
                        pos,
                        button: butt,
                        pressed: false,
                        modifiers: egui_modifiers,
                    });
                }
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
                    let tf_mouse_pos =
                        egui::pos2(*x as f32 / ppt, *y as f32 / ppt);
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
                            // TODO: move to console
                            //sdl2::keyboard::Keycode::D => {
                            //    if self.modifiers.control && self.console_has_focus {
                            //        self.delete_console_text = true;
                            //    } else {
                            //        self.input.events.push(egui::Event::Key {
                            //            key: egui::Key::D,
                            //            pressed: true,
                            //            repeat: *repeat,
                            //            modifiers: egui_modifiers,
                            //        });
                            //    }
                            //}
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

    pub fn render(
        &mut self,
        glow: &glow::Context,
        screen_dimensions: (u32, u32),
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

        // todo: verify that switching checking the ctx scale for ui works here
        self.paint_primitives(glow, screen_dimensions, self.ctx.pixels_per_point(), clipped_primitives);

        // free textures
        for id in textures_delta.free {
            if let Some(tex) = self.textures.remove(&id) {
                unsafe {
                    glow.delete_texture(tex);
                }
            }
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

pub(super) struct CallbackFn {
    f: Box<dyn Fn(PaintCallbackInfo, &EguiRenderCore) + Sync + Send>,
}

impl CallbackFn {
    pub fn new<F: Fn(PaintCallbackInfo, &EguiRenderCore) + Sync + Send + 'static>(
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
