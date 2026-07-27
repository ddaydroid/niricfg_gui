//! Section widget modules for the Niri tool plugin.
//!
//! Each module provides a `build_*_section` function that reads from a
//! `ConfigDoc` (via `NiriTool::doc()`) and returns a `gtk4::Widget` tree
//! with libadwaita row widgets (SpinRow, SwitchRow, EntryRow, ExpanderRow)
//! for structured editing of that config section.
//!
//! All sections share a common `text_buffer` reference: when a widget value
//! changes, the section handler parses the buffer text, modifies the relevant
//! KDL node, re-serialises, and writes back to the buffer. This triggers the
//! shell's existing debounced validation loop.

pub mod input;
pub mod output;
