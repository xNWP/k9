use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use glow::HasContext;

use crate::{
    camera::{Angle, ScreenCamera},
    debug_ui::{self, EguiDebugUi},
    entity_component::{Entity, EntityTable},
    graphics::{GraphicsSystem, K9Renderer, SceneDirectorComponent},
    profile::ProfileSet,
    system::{FrameState, SystemCallbacks},
};

pub struct CreationArgs {
    pub max_fps: u32,
    pub user_systems: Vec<Box<dyn SystemCallbacks>>,
    pub loggers: Vec<Box<dyn log::Log>>,
    pub window_title: String,
    pub use_vsync: bool,
    pub dimensions: (u32, u32),
    pub fullscreen: bool,
}
impl Default for CreationArgs {
    fn default() -> Self {
        Self {
            max_fps: 240,
            user_systems: Vec::new(),
            loggers: Vec::new(),
            dimensions: (1280, 720),
            use_vsync: true,
            window_title: "k9 window".to_owned(),
            fullscreen: false,
        }
    }
}

pub fn run(args: Option<CreationArgs>) -> Result<(), String> {
    let args = match args {
        Some(x) => x,
        None => CreationArgs::default(),
    };

    // init logging
    let mut loggers = args.loggers;
    let dbg_console_logger = debug_ui::DebugConsoleLogger::new();
    let dbg_logger_shared = dbg_console_logger.get_shared();
    loggers.push(Box::new(dbg_console_logger));

    multi_log::MultiLogger::init(loggers, log::Level::Trace)
        .map_err(|e| format!("couldn't initialize logger: {e}"))?;

    // init entities
    let mut entities = EntityTable::new();
    let scene_dir = {
        let mut ent = Entity::new();
        let comp = SceneDirectorComponent::new();
        ent.add_component(comp);
        ent
    };
    entities.add_new_entity(scene_dir);

    // init sdl
    windows_dpi::enable_dpi();

    let sdl_ctx = sdl2::init().map_err(|e| format!("couldn't init sdl context: {e}"))?;
    let sdl_vss = sdl_ctx
        .video()
        .map_err(|e| format!("couldn't init sdl vss: {e}"))?;

    let gl_attr = sdl_vss.gl_attr();
    gl_attr.set_context_major_version(3);
    gl_attr.set_context_minor_version(3);
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);

    let mut sdl_wnd = sdl_vss
        .window("k9 window", args.dimensions.0, args.dimensions.1)
        .opengl()
        .position_centered()
        .build()
        .map_err(|e| format!("couldn't create window: {e}"))?;

    if args.fullscreen {
        sdl_wnd
            .set_fullscreen(sdl2::video::FullscreenType::True)
            .map_err(|e| log::error!("couldn't set window to fullscreen: {e}"))
            .ok();
    }

    let _gl_ctx = sdl_wnd
        .gl_create_context()
        .map_err(|e| format!("couldn't create OpenGL context: {e}"))?;
    let glow = unsafe {
        glow::Context::from_loader_function(|func_name| {
            sdl_vss.gl_get_proc_address(func_name).cast()
        })
    };

    sdl_vss
        .gl_set_swap_interval({
            if args.use_vsync {
                sdl2::video::SwapInterval::VSync
            } else {
                sdl2::video::SwapInterval::Immediate
            }
        })
        .map_err(|e| format!("couldn't set swap interval: {e}"))?;

    unsafe {
        // todo: this probably needs to be screen scaled
        glow.viewport(0, 0, args.dimensions.0 as i32, args.dimensions.1 as i32);
        glow.enable(glow::DEBUG_OUTPUT);
        glow.debug_message_callback(debug_callback);
    }

    let mut sdl_ep = sdl_ctx
        .event_pump()
        .map_err(|e| format!("couldn't create event pump: {e}"))?;

    sdl_wnd.show();

    let mut k9 =
        K9Renderer::new(&glow).map_err(|e| format!("couldn't init graphics system: {e}"))?;
    let mut gfx_system = GraphicsSystem::new();

    #[allow(unused_assignments)] // is used in log::info
    let mut is_frame_capped = false;

    let mut frame_profile = ProfileSet::new();
    let mut rc_gen_profile = ProfileSet::new();
    let mut gfx_profile = ProfileSet::new();
    let mut user_systems_profile = ProfileSet::new();
    let mut user_systems = args.user_systems;
    let mut sdl_events = Vec::new();

    let aspect_ratio = args.dimensions.0 as f32 / args.dimensions.1 as f32;
    let mut screen_camera = ScreenCamera::new(Angle::deg(45.0), aspect_ratio, (100.0, 5_000.0));

    let screen_dimensions = args.dimensions;
    let system_scale = {
        match sdl_wnd.display_index() {
            Ok(x) => match sdl_vss.display_dpi(x) {
                Ok((x, _, _)) => x / 96.0,
                Err(e) => {
                    log::error!("couldn't get display dpi: {e}");
                    1.0
                }
            },
            Err(e) => {
                log::error!("couldn't get display index: {e}");
                1.0
            }
        }
    };

    let mut ui_scale = system_scale;

    // setup some console commands
    let mut console_commands = BTreeMap::new();

    let mut current_render_commands = Some(Vec::new());
    let mut is_finished = false;

    let mut profile_update_time = Instant::now();

    let mut draw_debug_ui = false;
    let mut debug_ui = EguiDebugUi::new(&glow, ui_scale, &mut console_commands);

    let clipboard_util = sdl_vss.clipboard();

    // do first calls for systems
    for system in &mut user_systems {
        system.first_call(FrameState {
            ents: &mut entities,
            sdl_events: &sdl_events,
            screen_camera: &mut screen_camera,
            screen_dimensions,
            screen_scale: system_scale,
            console_commands: &mut console_commands,
        });
    }

    loop {
        // MAIN PROGRAM LOOP
        sdl_events = sdl_ep.poll_iter().collect();

        if draw_debug_ui {
            debug_ui.begin_frame(
                sdl_wnd.window_flags() & sdl2::sys::SDL_WindowFlags::SDL_WINDOW_INPUT_FOCUS as u32
                    != 0,
                &sdl_events,
                screen_dimensions,
                &clipboard_util,
            );
        }

        if is_finished {
            for system in &mut user_systems {
                system.exiting(FrameState {
                    ents: &mut entities,
                    sdl_events: &sdl_events,
                    screen_camera: &mut screen_camera,
                    screen_dimensions,
                    screen_scale: system_scale,
                    console_commands: &mut console_commands,
                });
            }
            gfx_system.exiting(FrameState {
                ents: &mut entities,
                sdl_events: &sdl_events,
                screen_camera: &mut screen_camera,
                screen_dimensions,
                screen_scale: system_scale,
                console_commands: &mut console_commands,
            });
            break;
        }

        frame_profile.scoped_run(|| {
            user_systems_profile.scoped_run(|| {
                for system in &mut user_systems {
                    system.update(FrameState {
                        ents: &mut entities,
                        sdl_events: &sdl_events,
                        screen_camera: &mut screen_camera,
                        screen_dimensions,
                        screen_scale: system_scale,
                        console_commands: &mut console_commands,
                    });
                }
            });

            let render_commands = rc_gen_profile.scoped_run(|| {
                gfx_system.update(FrameState {
                    ents: &mut entities,
                    sdl_events: &sdl_events,
                    screen_camera: &mut screen_camera,
                    screen_dimensions,
                    screen_scale: system_scale,
                    console_commands: &mut console_commands,
                });
                gfx_system.get_render_commands()
            });

            gfx_profile.scoped_run(|| {
                #[cfg(not(debug_assertions))]
                unsafe {
                    k9.render(&glow, current_render_commands.take().unwrap_unchecked())
                };
                #[cfg(debug_assertions)]
                k9.render(&glow, current_render_commands.take().unwrap());

                ui_scale = debug_ui.get_ui_scale();

                if k9.exit_called() {
                    is_finished = true;
                }
            });
            current_render_commands = Some(render_commands);
        });

        if draw_debug_ui {
            debug_ui.draw(screen_dimensions, &dbg_logger_shared, &mut console_commands);
            let (clipped_primitives, textures_delta, platform_output) = debug_ui.end_frame();
            debug_ui.handle_platform_output(platform_output, &clipboard_util);
            debug_ui.render(
                &glow,
                screen_dimensions,
                ui_scale,
                clipped_primitives,
                textures_delta,
            );
        }

        sdl_wnd.gl_swap_window();

        for event in &sdl_events {
            match event {
                sdl2::event::Event::Quit { timestamp: _ } => is_finished = true,
                sdl2::event::Event::KeyUp {
                    timestamp: _,
                    window_id: _,
                    keycode,
                    scancode: _,
                    keymod: _,
                    repeat: _,
                } => {
                    if let Some(kc) = keycode {
                        if *kc == sdl2::keyboard::Keycode::Backquote
                            && !debug_ui.wants_keyboard_input()
                        {
                            draw_debug_ui = !draw_debug_ui;
                            if draw_debug_ui {
                                debug_ui.set_console_focus();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut throttle_timer = Instant::now();

        // handle max_fps / throttling
        let min_frame_time_micros = 1_000_000 / args.max_fps as i128;
        let last_micros = unsafe { frame_profile.last().unwrap_unchecked().as_micros() as i128 };

        if last_micros < min_frame_time_micros {
            is_frame_capped = true;
            let mut sleep_micros = min_frame_time_micros - last_micros;
            loop {
                use std::thread::sleep;
                sleep_micros -= throttle_timer.elapsed().as_micros() as i128;
                throttle_timer = Instant::now();
                if sleep_micros < 1_000 {
                    break;
                } else if sleep_micros < 5_000 {
                    // 5 milliseconds
                    sleep(Duration::from_nanos(200));
                } else if sleep_micros < 50_000 {
                    // 50 milliseconds
                    sleep(Duration::from_micros(5_000));
                } else if sleep_micros < 500_000 {
                    // 500 milliseconds
                    sleep(Duration::from_micros(50_000));
                }
            }
        } else {
            is_frame_capped = false;
        }

        // handle profiling
        let sample_time = 20;
        if profile_update_time.elapsed().as_secs() >= sample_time {
            let mut fps_tag = "".to_owned();
            if args.use_vsync {
                fps_tag += " [vsync]";
            }
            if is_frame_capped {
                fps_tag += " [capped]";
            }
            log::info!(
                "\n{}fps{fps_tag}\
                \navg: {:.2?}, std.dev: {:.2?}\
                \nuser-sys avg: {:.2?}, std.dev: {:.2?}\
                \nrc-gen avg: {:.2?}, std.dev: {:.2?}\
                \ngfx avg: {:.2?}, std.dev: {:.2?}",
                frame_profile.run_count() / sample_time as usize,
                frame_profile.mean(),
                frame_profile.std_dev(),
                user_systems_profile.mean(),
                user_systems_profile.std_dev(),
                rc_gen_profile.mean(),
                rc_gen_profile.std_dev(),
                gfx_profile.mean(),
                gfx_profile.std_dev(),
            );

            gfx_profile.clear();
            rc_gen_profile.clear();
            frame_profile.clear();
            profile_update_time = Instant::now();
        }
    }

    Ok(())
}

fn debug_callback(_src: u32, ty: u32, _id: u32, severity: u32, msg: &str) {
    log::error!(
        "GL CALLBACK: {} type = 0x{:x}, sev = 0x{:x}, message = {:?}",
        {
            if ty == glow::DEBUG_TYPE_ERROR {
                "GL ERROR"
            } else {
                ""
            }
        },
        ty,
        severity,
        unescaper::unescape(msg).unwrap(),
    )
}