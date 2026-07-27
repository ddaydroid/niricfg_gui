//! dotcfg-gui core library — shared types, traits, and tool implementations
//! for both the GUI shell and headless tests. Anything GTK-specific lives
//! under `shell`, which is gated behind the `gtk` cargo feature so
//! `--no-default-features` builds stay free of GTK system deps.

pub mod core;
pub mod tools;

#[cfg(feature = "gtk")]
pub mod shell;

pub use crate::core::config_loader::{load_config, ConfigDoc};
pub use crate::core::config_writer::save_config;
pub use crate::core::diff::{line_diff, DiffLine};
pub use crate::core::error::{Error, ExternalChangeAction, Severity, ValidationIssue};
pub use crate::core::file_watcher::FileWatcher;
pub use crate::core::semantic_path::{build_index, SemanticIndex, SemanticPath};
pub use crate::core::tool_plugin::{DynTool, KdlBackedTool, ToolPlugin};
pub use crate::core::tool_registry::ToolRegistry;
pub use crate::core::undo_stack::{UndoCommand, UndoStack};
pub use crate::core::validator::{BoxFuture, CannedValidator, NoopValidator, Validator};
pub use crate::tools::niri::NiriTool;
pub use crate::tools::niri_validator::NiriValidator;

#[cfg(feature = "gtk")]
pub use crate::core::kdl_highlighter;
#[cfg(feature = "gtk")]
pub use crate::core::tool_plugin::ToolPluginUi;

#[cfg(feature = "gtk")]
pub use crate::shell::run_shell;
