//! Layer rules section: layer-rule entries for layer-shell surface placement.
//!
//! # KDL structure
//!
//! ```kdl
//! layer-rules {
//!     match app-id="waybar" workspace="current"
//! }
//! ```

use kdl::{KdlDocument, KdlEntry, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::get_buffer_text;

/// Build the "Layer Rules" section widget tree.
///
/// Lists all `match` entries within `layer-rules` as editable rows,
/// each showing app-id filter and workspace assignment.
pub fn build_layer_rules_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Layer Rules");
    group.set_description("Layer-shell window placement rules");

    let init_doc = tool.doc().unwrap_or_default();

    // Find =layer-rules= node and its =match= children.
    let layer_rules_node = init_doc
        .nodes()
        .iter()
        .find(|n| n.name().value() == "layer-rules");
    let match_nodes: Vec<(usize, kdl::KdlNode)> = layer_rules_node
        .iter()
        .flat_map(|n| n.children().iter().flat_map(|c| c.nodes().iter().cloned()))
        .enumerate()
        .filter(|(_, n)| n.name().value() == "match")
        .collect();

    if match_nodes.is_empty() {
        let empty_label = gtk4::Label::new(Some("No layer rules configured"));
        empty_label.set_margin_top(8);
        empty_label.set_margin_bottom(8);
        empty_label.set_sensitive(false);
        group.add(&empty_label);
        return group.upcast();
    }

    let buf = text_buffer.clone();

    for (idx, node) in &match_nodes {
        let match_row = adw::ExpanderRow::new();

        // Extract app-id from properties (key=value entries).
        let app_id = node
            .entries()
            .iter()
            .find(|e| e.name().is_some_and(|n| n.value() == "app-id"))
            .and_then(|e| match e.value() {
                KdlValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        match_row.set_title(&format!(
            "Layer: {}",
            if app_id.is_empty() { "*" } else { &app_id }
        ));

        // App-id entry row
        let appid_row = adw::EntryRow::new();
        appid_row.set_title("App ID");
        appid_row.set_text(&app_id);
        appid_row.set_tooltip_text(Some("Filter by application ID (e.g. \"waybar\", \"mako\")"));
        match_row.add_row(&appid_row);

        // Workspace assignment
        let ws_val = node
            .entries()
            .iter()
            .find(|e| e.name().is_some_and(|n| n.value() == "workspace"))
            .and_then(|e| match e.value() {
                KdlValue::String(s) => Some(s.clone()),
                KdlValue::Decimal(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_default();

        let ws_row = adw::EntryRow::new();
        ws_row.set_title("Workspace");
        ws_row.set_text(&ws_val);
        ws_row.set_tooltip_text(Some(
            "Target workspace (number or name, e.g. \"1\" or \"current\")",
        ));
        match_row.add_row(&ws_row);

        // Wire app-id changes
        let b = buf.clone();
        let match_idx = *idx;
        appid_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                modify_match_prop(&mut doc, "layer-rules", match_idx, "app-id", &row.text());
                b.set_text(&doc.to_string());
            }
        });

        // Wire workspace changes
        let b = buf.clone();
        let match_idx2 = *idx;
        ws_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                modify_match_prop(
                    &mut doc,
                    "layer-rules",
                    match_idx2,
                    "workspace",
                    &row.text(),
                );
                b.set_text(&doc.to_string());
            }
        });

        group.add(&match_row);
    }

    group.upcast()
}

/// Update a property (key=value) on a =match= child within a rules node.
fn modify_match_prop(
    doc: &mut KdlDocument,
    rules_node: &str,
    match_idx: usize,
    prop_name: &str,
    prop_value: &str,
) {
    let Some(rules) = doc
        .nodes_mut()
        .iter_mut()
        .find(|n| n.name().value() == rules_node)
    else {
        return;
    };
    let Some(children) = rules.children_mut() else {
        return;
    };

    let matches: Vec<usize> = children
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, n)| n.name().value() == "match")
        .map(|(i, _)| i)
        .collect();

    let Some(&nth) = matches.get(match_idx) else {
        return;
    };

    let match_node = &mut children.nodes_mut()[nth];

    // Find existing entry with this property name and update it.
    for entry in match_node.entries_mut().iter_mut() {
        if let Some(name) = entry.name() {
            if name.value() == prop_name {
                entry.set_value(KdlValue::String(prop_value.to_string()));
                return;
            }
        }
    }

    // No existing entry — add one.
    let mut new_entry = KdlEntry::new(KdlValue::String(prop_value.to_string()));
    new_entry.set_name(Some(kdl::KdlIdentifier::new(prop_name)));
    match_node.entries_mut().push(new_entry);
}
