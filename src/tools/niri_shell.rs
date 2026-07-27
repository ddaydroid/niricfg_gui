//! GTK-gated extension that implements `ToolPluginUi` for `NiriTool`.
//!
//! `NiriTool` itself stays free of GTK types so `--no-default-features`
//! builds compile without system GTK deps. This file adds the
//! `create_shell_page` method that returns a widget tree with section-
//! specific editors (SpinRow, SwitchRow, EntryRow, ExpanderRow) for
//! structured editing of the niri config.
//!
//! # Wave 3 Step 10 — Sections Part 1 (Basic Layouts)
//!
//! First two sections: `input` (keyboard + touchpad) and `output`
//! (per-monitor expander rows). More sections added in Steps 11-12.

#![cfg(feature = "gtk")]

use adw::prelude::*;
use adw::traits::*;

use crate::core::tool_plugin::ToolPluginUi;
use crate::tools::niri::sections;
use crate::tools::niri::NiriTool;

/// Build the section widgets view: a scrollable list of section widget groups
/// wrapped in `Adw.Clamp` for a comfortable max-width reading experience.
fn build_shell_page(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(800.0);
    clamp.set_tightening_threshold(400.0);

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);

    // Input section — pass both tool (for initial values) and buffer (for
    // write-back when values change).
    let input_group = sections::input::build_input_section(tool, text_buffer);
    vbox.append(&input_group);

    // Output section
    let output_group = sections::output::build_output_section(tool, text_buffer);
    vbox.append(&output_group);

    clamp.set_child(Some(&vbox));

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    scrolled.set_child(Some(&clamp));

    scrolled.upcast()
}

/// Public helper: build section widgets for a NiriTool. The shell calls this
/// from `build_editor_page` when the tool's id() == "niri".
pub fn build_niri_sections(
    tool: &dyn crate::core::tool_plugin::ToolPlugin,
    text_buffer: &gtk4::TextBuffer,
) -> Option<gtk4::Widget> {
    let niri = tool.as_any().downcast_ref::<NiriTool>()?;
    Some(build_shell_page(niri, text_buffer))
}

impl ToolPluginUi for NiriTool {
    fn create_shell_page(&self, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
        build_shell_page(self, text_buffer)
    }
}
