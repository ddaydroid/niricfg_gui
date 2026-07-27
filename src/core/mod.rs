//! Core types: error enum, plugin trait, undo stack, config loader, config writer, tool registry, file watcher. Builds without GTK.

pub mod config_loader;
pub mod config_writer;
pub mod error;
pub mod file_watcher;
pub mod tool_plugin;
pub mod tool_registry;
pub mod undo_stack;
