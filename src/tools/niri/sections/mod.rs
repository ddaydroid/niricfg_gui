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

use gtk4::prelude::*;
use kdl::{KdlDocument, KdlEntry, KdlValue};

pub mod animations;
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

/// Read all text from a buffer as a String.
pub fn get_buffer_text(buf: &gtk4::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}
