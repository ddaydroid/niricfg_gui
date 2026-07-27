//! Output section widget: per-monitor configuration via `Adw.ExpanderRow`.
//!
//! Reads `output` nodes from the `ConfigDoc` at construction time and
//! creates one expander row per monitor, with a `Adw.SpinRow` for the
//! scale factor and an `Adw.EntryRow` for the mode string.
//!
//! # KDL structure
//!
//! ```kdl
//! output "eDP-1" {
//!     scale 1.5
//!     mode "1920x1080@144"
//! }
//! ```

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use std::str::FromStr;

use crate::tools::niri::NiriTool;

/// Build the "Output" section widget tree.
///
/// Iterates over all `output` nodes in the ConfigDoc and creates an
/// `Adw.ExpanderRow` for each, with editable scale and mode children.
pub fn build_output_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Output");
    group.set_description("Per-monitor display settings");

    let init_doc = tool.doc().unwrap_or_default();

    // Find all output nodes.
    let output_nodes: Vec<(String, kdl::KdlNode)> = init_doc
        .nodes()
        .iter()
        .filter(|n| n.name().value() == "output")
        .filter_map(|n| {
            let name = n
                .entries()
                .first()
                .and_then(|e| match e.value() {
                    KdlValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "unknown".to_string());
            Some((name, n.clone()))
        })
        .collect();

    if output_nodes.is_empty() {
        let empty_label = gtk4::Label::new(Some("No outputs configured"));
        empty_label.set_margin_top(8);
        empty_label.set_margin_bottom(8);
        empty_label.set_sensitive(false);
        group.add(&empty_label);
        return group.upcast();
    }

    let buf = text_buffer.clone();

    for (monitor_name, node) in &output_nodes {
        let monitor_row = adw::ExpanderRow::new();
        monitor_row.set_title(&monitor_name);

        // Scale SpinRow
        let scale_val = node
            .children()
            .iter()
            .flat_map(|c| c.nodes())
            .find(|n| n.name().value() == "scale")
            .and_then(|n| n.entries().first())
            .and_then(|e| match e.value() {
                KdlValue::Decimal(f) => Some(*f),
                KdlValue::Base10(i) => Some(*i as f64),
                _ => None,
            })
            .unwrap_or(1.0);

        let scale_adj = gtk4::Adjustment::new(scale_val, 0.25, 5.0, 0.25, 0.5, 0.0);
        let scale_row = adw::SpinRow::new(&scale_adj, 0.1, 2);
        scale_row.set_title("Scale");
        scale_row.set_tooltip_text(Some("Display scale factor (0.25–5.0)"));
        monitor_row.add_row(&scale_row);

        // Mode EntryRow
        let mode_val = node
            .children()
            .iter()
            .flat_map(|c| c.nodes())
            .find(|n| n.name().value() == "mode")
            .and_then(|n| n.entries().first())
            .and_then(|e| match e.value() {
                KdlValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let mode_row = adw::EntryRow::new();
        mode_row.set_title("Mode");
        mode_row.set_text(&mode_val);
        mode_row.set_tooltip_text(Some("Display mode (e.g. \"1920x1080@144\")"));
        monitor_row.add_row(&mode_row);

        // Wire scale changes
        let b = buf.clone();
        let mn = monitor_name.clone();
        scale_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                modify_output_scale(&mut doc, &mn, row.value());
                b.set_text(&doc.to_string());
            }
        });

        // Wire mode changes
        let b = buf.clone();
        let mn2 = monitor_name.clone();
        mode_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                modify_output_mode(&mut doc, &mn2, &row.text());
                b.set_text(&doc.to_string());
            }
        });

        group.add(&monitor_row);
    }

    group.upcast()
}
/// Update the scale value for a specific output monitor in the KDL doc.
fn modify_output_scale(doc: &mut KdlDocument, monitor: &str, scale: f64) {
    for node in doc.nodes_mut().iter_mut() {
        if node.name().value() == "output" {
            let is_match = node
                .entries()
                .first()
                .and_then(|e| match e.value() {
                    KdlValue::String(s) => Some(s == monitor),
                    _ => None,
                })
                .unwrap_or(false);

            if is_match {
                if let Some(children) = node.children_mut() {
                    for child in children.nodes_mut().iter_mut() {
                        if child.name().value() == "scale" {
                            let entry = KdlEntry::new(KdlValue::Decimal(scale));
                            if child.entries().is_empty() {
                                child.entries_mut().push(entry);
                            } else {
                                child.entries_mut()[0] = entry;
                            }
                            return;
                        }
                    }
                    // No scale node found — create one.
                    let mut new_node = KdlNode::new("scale");
                    new_node
                        .entries_mut()
                        .push(KdlEntry::new(KdlValue::Decimal(scale)));
                    children.nodes_mut().push(new_node);
                }
                return;
            }
        }
    }
}

/// Update the mode value for a specific output monitor in the KDL doc.
fn modify_output_mode(doc: &mut KdlDocument, monitor: &str, mode: &str) {
    for node in doc.nodes_mut().iter_mut() {
        if node.name().value() == "output" {
            let is_match = node
                .entries()
                .first()
                .and_then(|e| match e.value() {
                    KdlValue::String(s) => Some(s == monitor),
                    _ => None,
                })
                .unwrap_or(false);

            if is_match {
                if let Some(children) = node.children_mut() {
                    for child in children.nodes_mut().iter_mut() {
                        if child.name().value() == "mode" {
                            let entry = KdlEntry::new(KdlValue::String(mode.to_string()));
                            if child.entries().is_empty() {
                                child.entries_mut().push(entry);
                            } else {
                                child.entries_mut()[0] = entry;
                            }
                            return;
                        }
                    }
                    // No mode node found — create one.
                    let mut new_node = KdlNode::new("mode");
                    new_node
                        .entries_mut()
                        .push(KdlEntry::new(KdlValue::String(mode.to_string())));
                    children.nodes_mut().push(new_node);
                }
                return;
            }
        }
    }
}

/// Read all text from a buffer as a String.
fn get_buffer_text(buf: &gtk4::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}
