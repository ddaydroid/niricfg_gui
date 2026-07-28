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

use adw::prelude::*;
use gtk4::prelude::*;
use kdl::{KdlDocument, KdlEntry, KdlValue};
use libadwaita as adw;

pub mod animations;
pub mod binds;
pub mod gestures;
pub mod input;
pub mod layer_rules;
pub mod layout;
pub mod output;
pub mod startup;
pub mod workspaces;

// ---------------------------------------------------------------------------
// Shared KDL tree helpers — used by all section modules
// ---------------------------------------------------------------------------

/// Read a string value from a leaf node at the given path.
pub fn get_kdl_str(doc: &KdlDocument, path: &[&str]) -> Option<String> {
    let mut current = doc;
    for (i, &segment) in path.iter().enumerate() {
        let node = current
            .nodes()
            .iter()
            .find(|n| n.name().value() == segment)?;
        if i == path.len() - 1 {
            return node.entries().first().and_then(|e| match e.value() {
                KdlValue::String(s) => Some(s.clone()),
                KdlValue::Float(n) => Some(n.to_string()),
                KdlValue::Integer(n) => Some(n.to_string()),
                _ => None,
            });
        }
        current = node.children().as_ref()?;
    }
    None
}

/// Read a boolean value from a leaf node.
pub fn get_kdl_bool(doc: &KdlDocument, path: &[&str]) -> Option<bool> {
    get_kdl_str(doc, path).map(|s| s == "true" || s == "1" || s == "yes")
}

/// Read an f64 value from a leaf node.
pub fn get_kdl_f64(doc: &KdlDocument, path: &[&str]) -> Option<f64> {
    let mut current = doc;
    for (i, &segment) in path.iter().enumerate() {
        let node = current
            .nodes()
            .iter()
            .find(|n| n.name().value() == segment)?;
        if i == path.len() - 1 {
            return node.entries().first().and_then(|e| match e.value() {
                KdlValue::Float(n) => Some(*n),
                KdlValue::Integer(n) => Some(*n as f64),
                KdlValue::String(s) => s.parse::<f64>().ok(),
                _ => None,
            });
        }
        current = node.children().as_ref()?;
    }
    None
}

/// Recursive helper: navigate to a leaf node by index path and set its entry.
fn set_kdl_value_at(doc: &mut KdlDocument, indices: &[usize], entry: KdlEntry) {
    if indices.len() == 1 {
        let node = &mut doc.nodes_mut()[indices[0]];
        if node.entries().is_empty() {
            node.entries_mut().push(entry);
        } else {
            node.entries_mut()[0] = entry;
        }
    } else if let Some(children) = doc.nodes_mut()[indices[0]].children_mut() {
        set_kdl_value_at(children, &indices[1..], entry);
    }
}

/// Set a leaf node's first entry to a KDL value.
///
/// Navigation is done in two phases: first an immutable traversal collects
/// node indices, then a recursive mutable traversal applies the change.
/// This avoids borrow-checker conflicts from reassigning a `&mut` reference
/// while it is still borrowed by `children_mut()`.
pub fn set_kdl_value(doc: &mut KdlDocument, path: &[&str], value: &KdlValue) {
    // Phase 1: immutable traversal — collect node indices.
    let mut node_indices: Vec<usize> = Vec::new();
    {
        let mut current: &KdlDocument = doc;
        for &segment in path {
            let idx = current
                .nodes()
                .iter()
                .position(|n| n.name().value() == segment);
            match idx {
                Some(i) => {
                    node_indices.push(i);
                    if node_indices.len() < path.len() {
                        if let Some(c) = current.nodes()[i].children() {
                            current = c;
                        } else {
                            return;
                        }
                    }
                }
                None => return,
            }
        }
    }

    let entry = match value {
        KdlValue::Float(f) => KdlEntry::new(KdlValue::Float(*f)),
        KdlValue::String(s) => KdlEntry::new(KdlValue::String(s.clone())),
        _ => KdlEntry::new(value.clone()),
    };

    // Phase 2: recursive mutable traversal.
    set_kdl_value_at(doc, &node_indices, entry);
}

/// Convenience: set an f64 value.
pub fn set_kdl_f64(doc: &mut KdlDocument, path: &[&str], value: f64) {
    set_kdl_value(doc, path, &KdlValue::Float(value));
}

/// Wrap a list widget (e.g. PreferencesGroup) in a searchable Adw::ToolbarView.
///
/// Adds a GtkSearchBar with GtkSearchEntry to the bottom bar. When the user
/// types, child rows whose title or subtitle do not contain the search text
/// are hidden; matching rows stay visible. An empty search shows all rows.
///
/// This is a lightweight alternative to gtk::FilterListModel that works with
/// the existing widget-per-row architecture (no gio::ListStore refactor needed).
pub fn wrap_searchable(content: gtk4::Widget) -> gtk4::Widget {
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search…"));
    search_entry.set_tooltip_text(Some("Type to filter rows"));

    let search_bar = gtk4::SearchBar::new();
    search_bar.set_child(Some(&search_entry));
    search_bar.set_show_close_button(true);
    search_bar.set_key_capture_widget(Some(&content));

    // Connect search text changes to filter child visibility
    search_entry.connect_search_changed({
        let content = content.clone();
        move |entry| {
            let query = entry.text();
            let query_lower = query.to_lowercase();

            // Check if a row widget matches the search query.
            fn row_matches_query(widget: &gtk4::Widget, query_lower: &str) -> bool {
                // Check ExpanderRow title + subtitle (returns glib::GString directly)
                if let Some(row) = widget.downcast_ref::<adw::ExpanderRow>() {
                    let title = row.title().to_lowercase();
                    let subtitle = row.subtitle().to_lowercase();
                    if title.contains(query_lower) || subtitle.contains(query_lower) {
                        return true;
                    }
                    // Check children recursively
                    let mut child = row.first_child();
                    while let Some(ref c) = child {
                        if row_matches_query(c, query_lower) {
                            return true;
                        }
                        child = c.next_sibling();
                    }
                    return false;
                }
                // Check ActionRow title + subtitle (returns Option<glib::GString>)
                if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                    let title = row.title().to_lowercase();
                    let subtitle = row.subtitle().map_or(String::new(), |s| s.to_lowercase());
                    return title.contains(query_lower) || subtitle.contains(query_lower);
                }
                // Check EntryRow: title + text content
                if let Some(entry_row) = widget.downcast_ref::<adw::EntryRow>() {
                    let title = entry_row.title().to_lowercase();
                    let text = entry_row.text().to_lowercase();
                    return title.contains(query_lower) || text.contains(query_lower);
                }
                // Check SwitchRow: title + optional subtitle
                if let Some(switch_row) = widget.downcast_ref::<adw::SwitchRow>() {
                    let title = switch_row.title().to_lowercase();
                    let subtitle = switch_row
                        .subtitle()
                        .map_or(String::new(), |s| s.to_lowercase());
                    return title.contains(query_lower) || subtitle.contains(query_lower);
                }
                // Check SpinRow: title only
                if let Some(spin_row) = widget.downcast_ref::<adw::SpinRow>() {
                    let title = spin_row.title().to_lowercase();
                    return title.contains(query_lower);
                }
                // Default: hide non-matching
                false
            }

            // Filter direct children of the content widget
            let mut child = content.first_child();
            while let Some(ref c) = child {
                let visible = query_lower.is_empty() || row_matches_query(c, &query_lower);
                c.set_visible(visible);
                child = c.next_sibling();
            }
        }
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.set_content(Some(&content));
    toolbar.add_top_bar(&search_bar);

    toolbar.upcast()
}

/// Read all text from a buffer as a String.
pub fn get_buffer_text(buf: &gtk4::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}
