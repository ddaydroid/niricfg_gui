//! Startup section: spawn-at-startup commands.
//!
//! # KDL structure
//!
//! ```kdl
//! spawn-at-startup "foot"
//! spawn-at-startup "waybar"
//! ```
//!
//! Each `spawn-at-startup` is a top-level node whose single entry is a
//! string command. The section lists each as a row with an editable
//! EntryRow, matching the list pattern used in `workspaces.rs`.

use adw::prelude::*;
use kdl::{KdlDocument, KdlEntry, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::get_buffer_text;

/// Build the "Startup" section widget tree.
///
/// Lists all `spawn-at-startup` entries as rows with editable command
/// EntryRow widgets. Changes are applied by finding the matching node
/// by index and updating its first entry in-place.
pub fn build_startup_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Startup");
    group.set_description(Some("Commands to run when Niri starts"));

    let init_doc = tool.doc().unwrap_or_default();

    // Collect all spawn-at-startup entries.
    let startup_cmds: Vec<(usize, String)> = init_doc
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, n)| n.name().value() == "spawn-at-startup")
        .map(|(i, n)| {
            let cmd = n
                .entries()
                .first()
                .and_then(|e| match e.value() {
                    KdlValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (i, cmd)
        })
        .collect();

    if startup_cmds.is_empty() {
        let empty_label = gtk4::Label::new(Some("No startup commands configured"));
        empty_label.set_margin_top(8);
        empty_label.set_margin_bottom(8);
        empty_label.set_sensitive(false);
        group.add(&empty_label);
        return group.upcast();
    }

    let buf = text_buffer.clone();

    for (node_idx, cmd) in &startup_cmds {
        let cmd_row = adw::EntryRow::new();
        cmd_row.set_title("Command");
        cmd_row.set_text(cmd);
        cmd_row.set_tooltip_text(Some(
            "Command to execute at startup (e.g. \"foot\", \"waybar\")",
        ));

        // Wire command changes
        let b = buf.clone();
        let idx = *node_idx;
        cmd_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                // Find the nth spawn-at-startup node by scanning.
                let mut found = 0;
                for node in doc.nodes_mut().iter_mut() {
                    if node.name().value() == "spawn-at-startup" {
                        if found == idx {
                            let val = row.text();
                            if node.entries().is_empty() {
                                node.entries_mut()
                                    .push(KdlEntry::new(KdlValue::String(val.to_string())));
                            } else {
                                node.entries_mut()[0] =
                                    KdlEntry::new(KdlValue::String(val.to_string()));
                            }
                            b.set_text(&doc.to_string());
                            return;
                        }
                        found += 1;
                    }
                }
            }
        });

        group.add(&cmd_row);
    }

    group.upcast()
}
