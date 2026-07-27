//! Core types: error enum, plugin trait, undo stack, config loader, tool registry. Builds without GTK.

pub mod config_loader;
pub mod error;
pub mod tool_plugin;
pub mod tool_registry;
pub mod undo_stack;
