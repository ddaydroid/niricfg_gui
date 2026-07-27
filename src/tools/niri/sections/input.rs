//! Input section widget: keyboard + touchpad settings.
//!
//! Reads initial values from `NiriTool::doc()` at construction time.
//! When a widget value changes, the handler reads the current editor text
//! buffer, parses it to KDL, modifies the relevant node/entry in-place,
//! re-serialises to text, and writes back to the buffer. The buffer write
//! triggers the shell's existing debounced validation loop.
//!
//! This parse→modify→serialise→write cycle means comments in the text
//! are NOT preserved through section-widget edits (kdl v6 discards
//! comments on parse). Users who need comment preservation can use the
//! "Raw" text editor tab instead.

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use std::str::FromStr;

use crate::tools::niri::NiriTool;

// ---------------------------------------------------------------------------
// KDL tree helpers — navigate and modify a KDL document by dotted path
// ---------------------------------------------------------------------------

/// Read a string value from a leaf node at the given path
/// (e.g. `["input", "keyboard", "xkb-layout"]`).
fn get_kdl_str(doc: &KdlDocument, path: &[&str]) -> Option<String> {
    let mut current = doc;
    for (i, &segment) in path.iter().enumerate() {
        let node = current
            .nodes()
            .iter()
            .find(|n| n.name().value() == segment)?;
        if i == path.len() - 1 {
            return node.entries().first().and_then(|e| match e.value() {
                KdlValue::String(s) => Some(s.clone()),
                KdlValue::Identifier(s) => Some(s.clone()),
                KdlValue::Decimal(n) => Some(n.to_string()),
                KdlValue::Base10(n) => Some(n.to_string()),
                _ => None,
            });
        }
        current = node.children().as_ref()?;
    }
    None
}

/// Read a boolean value from a leaf node.
fn get_kdl_bool(doc: &KdlDocument, path: &[&str]) -> Option<bool> {
    get_kdl_str(doc, path).map(|s| s == "true" || s == "1" || s == "yes")
}

/// Read an f64 value from a leaf node.
fn get_kdl_f64(doc: &KdlDocument, path: &[&str]) -> Option<f64> {
    let mut current = doc;
    for (i, &segment) in path.iter().enumerate() {
        let node = current
            .nodes()
            .iter()
            .find(|n| n.name().value() == segment)?;
        if i == path.len() - 1 {
            return node.entries().first().and_then(|e| match e.value() {
                KdlValue::Decimal(n) => Some(*n),
                KdlValue::Base10(n) => Some(*n as f64),
                KdlValue::String(s) => s.parse::<f64>().ok(),
                _ => None,
            });
        }
        current = node.children().as_ref()?;
    }
    None
}

/// Navigate the KDL tree by path indices (not references) to set a leaf's
/// first entry to an f64 value. Returns the re-serialised document text.
fn set_kdl_value(doc: &mut KdlDocument, path: &[&str], value: &KdlValue) {
    // Walk down using indices to avoid borrow-checker conflicts.
    let mut node_indices: Vec<usize> = Vec::new();
    let mut current = doc;

    for &segment in path {
        let pos = current
            .nodes()
            .iter()
            .position(|n| n.name().value() == segment);
        match pos {
            Some(idx) => {
                node_indices.push(idx);
                // If not the last segment, step into children.
                if node_indices.len() < path.len() {
                    if let Some(children) = current.nodes()[idx].children() {
                        current = children;
                    } else {
                        // Path doesn't exist — create the remaining segments.
                        // For now, just return without modifying.
                        return;
                    }
                }
            }
            None => return, // Path doesn't exist
        }
    }

    // Now apply using the collected indices.
    let mut cur = doc;
    let last = path.len() - 1;
    for (depth, &idx) in node_indices.iter().enumerate() {
        if depth == last {
            // Leaf node — set the first entry.
            let node = &mut cur.nodes_mut()[idx];
            let entry = match value {
                KdlValue::Decimal(f) => KdlEntry::new(KdlValue::Decimal(*f)),
                KdlValue::String(s) => KdlEntry::new(KdlValue::String(s.clone())),
                _ => KdlEntry::new(value.clone()),
            };
            if node.entries().is_empty() {
                node.entries_mut().push(entry);
            } else {
                node.entries_mut()[0] = entry;
            }
        } else if let Some(children) = cur.nodes_mut()[idx].children_mut() {
            cur = children;
        }
    }
}

/// Convenience: set an f64 value.
fn set_kdl_f64(doc: &mut KdlDocument, path: &[&str], value: f64) {
    set_kdl_value(doc, path, &KdlValue::Decimal(value));
}

// ---------------------------------------------------------------------------
// Input section builder
// ---------------------------------------------------------------------------

/// Build the "Input" section widget tree.
///
/// `tool` is used to read initial ConfigDoc values at construction time.
/// `text_buffer` is the shared editor buffer: when any section widget value
/// changes, this function parses the buffer text, modifies the relevant KDL
/// node, re-serialises, and writes back to the buffer.
pub fn build_input_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Input");
    group.set_description("Keyboard and touchpad settings");

    // Snapshot initial values from the tool's current ConfigDoc.
    let init_doc = tool.doc().unwrap_or_default();

    // --- Keyboard sub-section ---
    let keyboard_row = adw::ExpanderRow::new();
    keyboard_row.set_title("Keyboard");

    let init_repeat_delay =
        get_kdl_f64(&init_doc, &["input", "keyboard", "repeat-delay"]).unwrap_or(250.0);
    let init_repeat_rate =
        get_kdl_f64(&init_doc, &["input", "keyboard", "repeat-rate"]).unwrap_or(33.0);
    let init_xkb_layout =
        get_kdl_str(&init_doc, &["input", "keyboard", "xkb-layout"]).unwrap_or_default();

    let repeat_delay_adj = gtk4::Adjustment::new(init_repeat_delay, 100.0, 2000.0, 25.0, 50.0, 0.0);
    let repeat_delay_row = adw::SpinRow::new(&repeat_delay_adj, 1.0, 0);
    repeat_delay_row.set_title("Repeat Delay (ms)");
    repeat_delay_row.set_tooltip_text(Some("Milliseconds before key repeat starts (100–2000)"));
    keyboard_row.add_row(&repeat_delay_row);

    let repeat_rate_adj = gtk4::Adjustment::new(init_repeat_rate, 10.0, 200.0, 1.0, 5.0, 0.0);
    let repeat_rate_row = adw::SpinRow::new(&repeat_rate_adj, 1.0, 0);
    repeat_rate_row.set_title("Repeat Rate (keys/s)");
    repeat_rate_row.set_tooltip_text(Some("Key repeat rate in characters per second (10–200)"));
    keyboard_row.add_row(&repeat_rate_row);

    let xkb_layout_row = adw::EntryRow::new();
    xkb_layout_row.set_title("XKB Layout");
    xkb_layout_row.set_text(&init_xkb_layout);
    xkb_layout_row.set_tooltip_text(Some(
        "Keyboard layout variant (e.g. \"us\", \"us,ru\", \"de\")",
    ));
    keyboard_row.add_row(&xkb_layout_row);

    group.add(&keyboard_row);

    // --- Touchpad sub-section ---
    let touchpad_row = adw::ExpanderRow::new();
    touchpad_row.set_title("Touchpad");

    let init_tap_to_click =
        get_kdl_bool(&init_doc, &["input", "touchpad", "tap-to-click"]).unwrap_or(true);
    let init_natural_scroll =
        get_kdl_bool(&init_doc, &["input", "touchpad", "natural-scroll"]).unwrap_or(true);
    let init_tap_and_drag =
        get_kdl_bool(&init_doc, &["input", "touchpad", "tap-and-drag"]).unwrap_or(true);

    let tap_to_click_row = adw::SwitchRow::new();
    tap_to_click_row.set_title("Tap to Click");
    tap_to_click_row.set_subtitle("Enable tap-to-click on the touchpad");
    tap_to_click_row.set_active(init_tap_to_click);
    touchpad_row.add_row(&tap_to_click_row);

    let natural_scroll_row = adw::SwitchRow::new();
    natural_scroll_row.set_title("Natural Scroll");
    natural_scroll_row.set_subtitle("Natural (inverted) scrolling direction");
    natural_scroll_row.set_active(init_natural_scroll);
    touchpad_row.add_row(&natural_scroll_row);

    let tap_and_drag_row = adw::SwitchRow::new();
    tap_and_drag_row.set_title("Tap and Drag");
    tap_and_drag_row.set_subtitle("Enable tap-and-drag on the touchpad");
    tap_and_drag_row.set_active(init_tap_and_drag);
    touchpad_row.add_row(&tap_and_drag_row);

    group.add(&touchpad_row);

    // --- Shared buffer reference for all change handlers ---
    // Each clone of glib::Object is reference-counted, not a deep copy.
    let buf = text_buffer.clone();

    // Helper: read current buffer text, modify one KDL value, write back.
    let buf_c = buf.clone();
    repeat_delay_row.connect_changed({
        let b = buf_c.clone();
        move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                set_kdl_f64(
                    &mut doc,
                    &["input", "keyboard", "repeat-delay"],
                    row.value(),
                );
                b.set_text(&doc.to_string());
            }
        }
    });

    let b = buf.clone();
    repeat_rate_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["input", "keyboard", "repeat-rate"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    xkb_layout_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = row.text();
            set_kdl_value(
                &mut doc,
                &["input", "keyboard", "xkb-layout"],
                &KdlValue::String(val),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    tap_to_click_row.connect_active_notified(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["input", "touchpad", "tap-to-click"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    natural_scroll_row.connect_active_notified(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["input", "touchpad", "natural-scroll"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf;
    tap_and_drag_row.connect_active_notified(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["input", "touchpad", "tap-and-drag"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    group.upcast()
}

/// Read all text from a buffer as a String.
fn get_buffer_text(buf: &gtk4::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}
