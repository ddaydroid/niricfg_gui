//! dotcfg-gui core library — shared types and traits for both the GUI shell
//! and headless tests. Anything GTK-specific lives under `shell`, which is
//! gated behind the `gtk` cargo feature so `--no-default-features` builds
//! stay free of GTK system deps.

pub mod core;

#[cfg(feature = "gtk")]
pub mod shell;

pub use crate::core::config_loader::{load_config, ConfigDoc};
pub use crate::core::config_writer::save_config;
pub use crate::core::error::{Error, ExternalChangeAction, Severity, ValidationIssue};
pub use crate::core::file_watcher::FileWatcher;
pub use crate::core::tool_plugin::{DynTool, ToolPlugin};
pub use crate::core::tool_registry::ToolRegistry;
pub use crate::core::undo_stack::{UndoCommand, UndoStack};

#[cfg(feature = "gtk")]
pub use crate::core::tool_plugin::ToolPluginUi;

#[cfg(feature = "gtk")]
pub use crate::shell::run_shell;
