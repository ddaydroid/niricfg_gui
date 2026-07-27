//! Workspaces section: preferred-workspace-for-output assignments.
//!
//! # KDL structure
//!
//! ```kdl
//! preferred-workspace-for-output "eDP-1" 1
//! preferred-workspace-for-output "HDMI-A-1" 2
//! ```

use kdl::{KdlDocument, KdlEntry, KdlValue};
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::{get_buffer_text, set_kdl_value};

/// Build the \"Workspaces\" section widget tree.
///
/// Lists all `preferred-workspace-for-output` entries as editable rows,
/// each showing the monitor name and workspace number.
pub fn build_workspaces_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Workspaces");
    group.set_description("Preferred workspace per output");

    let init_doc = tool.doc().unwrap_or_default();

    // Collect all preferred-workspace-for-output entries.
    struct WsEntry {
        monitor: String,
        ws_num: f64,
        node_idx: usize,
    }

    let ws_entries: Vec<WsEntry> = init_doc
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, n)| n.name().value() == "preferred-workspace-for-output")
        .filter_map(|(i, n)| {
            let monitor = n
                .entries()
                .first()
                .and_then(|e| match e.value() {
                    KdlValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let ws_num = n
                .entries()
                .get(1)
                .and_then(|e| match e.value() {
                    KdlValue::Decimal(f) => Some(*f),
                    KdlValue::Base10(i) => Some(*i as f64),
                    KdlValue::String(s) => s.parse::<f64>().ok(),
                    _ => None,
                })
                .unwrap_or(1.0);
            Some(WsEntry {
                monitor,
                ws_num,
                node_idx: i,
            })
        })
        .collect();

    if ws_entries.is_empty() {
        let empty_label = gtk4::Label::new(Some("No workspace preferences configured"));
        empty_label.set_margin_top(8);
        empty_label.set_margin_bottom(8);
        empty_label.set_sensitive(false);
        group.add(&empty_label);
        return group.upcast();
    }

    let buf = text_buffer.clone();

    for entry in &ws_entries {
        let ws_row = adw::ExpanderRow::new();
        ws_row.set_title(&format!("{} → Workspace {}", entry.monitor, entry.ws_num));

        // Monitor name EntryRow
        let mon_row = adw::EntryRow::new();
        mon_row.set_title("Output");
        mon_row.set_text(&entry.monitor);
        mon_row.set_editable(false); // Changing monitor might be complex; keep read-only for v1
        ws_row.add_row(&mon_row);

        // Workspace number SpinRow
        let ws_adj = gtk4::Adjustment::new(entry.ws_num, 1.0, 20.0, 1.0, 1.0, 0.0);
        let ws_num_row = adw::SpinRow::new(&ws_adj, 1.0, 0);
        ws_num_row.set_title("Workspace");
        ws_num_row.set_tooltip_text(Some("Workspace number (1–20)"));
        ws_row.add_row(&ws_num_row);

        // Wire workspace number changes
        let b = buf.clone();
        let mn = entry.monitor.clone();
        ws_num_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                // Find the matching node by monitor name and update its second entry.
                for node in doc.nodes_mut().iter_mut() {
                    if node.name().value() == "preferred-workspace-for-output" {
                        let is_match = node
                            .entries()
                            .first()
                            .and_then(|e| match e.value() {
                                KdlValue::String(s) => Some(s == &mn),
                                _ => None,
                            })
                            .unwrap_or(false);

                        if is_match {
                            let entry = KdlEntry::new(KdlValue::Decimal(row.value()));
                            if node.entries().len() < 2 {
                                node.entries_mut().push(entry);
                            } else {
                                node.entries_mut()[1] = entry;
                            }
                            b.set_text(&doc.to_string());
                            return;
                        }
                    }
                }
            }
        });

        group.add(&ws_row);
    }

    group.upcast()
}
