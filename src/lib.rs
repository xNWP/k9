#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(downcast_unchecked)]
#![feature(btree_drain_filter)]
#![feature(stmt_expr_attributes)]

pub mod process;
pub use process::run;

pub mod entity_component;
pub use entity_component::EntityTable;
pub mod graphics;
mod profile;
pub mod system;
pub use system::System;
pub use system::SystemCallbacks;
pub use uuid;
pub mod camera;
pub mod debug_ui;
pub use k9_proc_macros::console_command;

pub use egui;
pub use egui_extras;
