//! Core types: error enum, plugin trait, undo stack, config loader, config
//! writer, validator, semantic-path indexer, tool registry, file watcher,
//! diff engine, KDL highlighter. Builds without GTK.

pub mod config_loader;
pub mod config_writer;
pub mod diff;
pub mod error;
pub mod file_watcher;
pub mod semantic_path;
pub mod tool_plugin;
pub mod tool_registry;
pub mod undo_stack;
pub mod validator;

#[cfg(feature = "gtk")]
pub mod kdl_highlighter;
