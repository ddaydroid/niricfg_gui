//! Binds section: keybinding entries with key-chord capture modal.
//!
//! # KDL structure
//!
//! ```kdl
//! binds {
//!     Mod+Return spawn "foot"
//!     Mod+Q close-window
//!     Mod+1 focus-workspace 1
//! }
//! ```
//!
//! Each child of `binds { }` is a node whose name is the key chord
//! (e.g. `Mod+Return`) and whose entries are the action name followed
//! by any arguments (e.g. `spawn "foot"`).
//!
//! # Key-chord capture
//!
//! A "Record" button opens an `Adw::Dialog` with a `gtk4::EventControllerKey`.
//! On key press the dialog captures the modifier state (Super → Mod, Ctrl,
//! Shift, Alt) and the key name, formats as `Mod+Key`, and updates the row.
//! The dialog is dismissed on Escape without saving.

use adw::prelude::*;
use kdl::{KdlDocument, KdlEntry, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::get_buffer_text;

/// Build the "Binds" section widget tree.
///
/// Lists all bind nodes inside the `binds { }` block. Each bind gets an
/// `Adw.ExpanderRow` showing the key chord as title, with:
/// - An `Adw.EntryRow` for the key chord (editable)
/// - A "Record" button that opens the key-capture dialog
/// - An `Adw.EntryRow` for the action + arguments
pub fn build_binds_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Key Bindings");
    group.set_description(Some("Keyboard shortcuts for window management and actions"));

    let init_doc = tool.doc().unwrap_or_default();

    // Find the =binds= node and collect its children.
    let binds_parent = init_doc
        .nodes()
        .iter()
        .find(|n| n.name().value() == "binds")
        .and_then(|n| n.children());

    // Collect (chord, action, index) for each child.
    struct BindEntry {
        chord: String,  // e.g. "Mod+Return"
        action: String, // e.g. "spawn \"foot\""
        index: usize,   // position inside =binds= children list
    }

    let entries: Vec<BindEntry> = match binds_parent {
        Some(children) => children
            .nodes()
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let chord = n.name().value().to_string();
                let action = n
                    .entries()
                    .iter()
                    .map(|e| match e.value() {
                        KdlValue::String(s) => {
                            if s.contains(' ') || s.is_empty() {
                                format!("\"{}\"", s)
                            } else {
                                s.clone()
                            }
                        }
                        KdlValue::Integer(v) => v.to_string(),
                        KdlValue::Float(v) => v.to_string(),
                        KdlValue::Bool(v) => v.to_string(),
                        KdlValue::Null => "null".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                BindEntry {
                    chord,
                    action,
                    index: i,
                }
            })
            .collect(),
        None => Vec::new(),
    };

    if entries.is_empty() {
        let empty_label = gtk4::Label::new(Some("No key bindings configured"));
        empty_label.set_margin_top(8);
        empty_label.set_margin_bottom(8);
        empty_label.set_sensitive(false);
        group.add(&empty_label);
        return group.upcast();
    }

    let buf = text_buffer.clone();

    for entry in &entries {
        let bind_row = adw::ExpanderRow::new();
        bind_row.set_title(&entry.chord);
        bind_row.set_subtitle(&entry.action);

        // --- Key chord EntryRow ---
        let chord_row = adw::EntryRow::new();
        chord_row.set_title("Key Chord");
        chord_row.set_text(&entry.chord);
        bind_row.add_row(&chord_row);

        // --- Record button ---
        let record_btn = gtk4::Button::with_label("Record…");
        record_btn.add_css_class("flat");
        record_btn.set_tooltip_text(Some("Press a key combination to set this binding"));

        let chord_idx = entry.index;

        record_btn.connect_clicked({
            let b = buf.clone();
            move |_| {
                let dialog = adw::Dialog::new();
                dialog.set_title("Record Key Binding");
                dialog.set_content_width(360);
                dialog.set_content_height(200);

                // Content: a label + the captured key display
                let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
                content.set_margin_top(24);
                content.set_margin_bottom(24);
                content.set_margin_start(24);
                content.set_margin_end(24);

                let instr_label =
                    gtk4::Label::new(Some("Press any key combination…\nPress Escape to cancel."));
                instr_label.set_halign(gtk4::Align::Center);
                content.append(&instr_label);

                let display = gtk4::Label::new(None::<&str>);
                display.add_css_class("title-1");
                display.set_halign(gtk4::Align::Center);
                content.append(&display);

                // Event controller for key capture
                let controller = gtk4::EventControllerKey::new();
                let dlg = dialog.downgrade();
                let display_clone = display.clone();
                let b3 = b.clone();

                controller.connect_key_pressed(move |_ctrl, key, _keycode, state| {
                    // Escape cancels
                    if key == gtk4::gdk::Key::Escape {
                        if let Some(d) = dlg.upgrade() {
                            d.close();
                        }
                        return glib::Propagation::Stop;
                    }

                    // Build modifiers string
                    let mut mods: Vec<&str> = Vec::new();
                    if state.contains(gtk4::gdk::ModifierType::SUPER_MASK) {
                        mods.push("Mod");
                    }
                    if state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                        mods.push("Ctrl");
                    }
                    if state.contains(gtk4::gdk::ModifierType::SHIFT_MASK) {
                        mods.push("Shift");
                    }
                    if state.contains(gtk4::gdk::ModifierType::ALT_MASK) {
                        mods.push("Alt");
                    }

                    // Get key name via the Key type's name() method
                    let key_name = key.name().unwrap_or_else(|| glib::GString::from("?"));

                    // For letter keys, use uppercase for Niri convention
                    let key_str = if key_name.len() == 1 {
                        let chars: Vec<char> = key_name.chars().collect();
                        if chars[0].is_ascii_lowercase() {
                            chars[0].to_ascii_uppercase().to_string()
                        } else {
                            key_name.to_string()
                        }
                    } else {
                        key_name.to_string()
                    };

                    let chord_str = if mods.is_empty() {
                        key_str.clone()
                    } else {
                        format!("{}+{}", mods.join("+"), key_str)
                    };

                    display_clone.set_text(&chord_str);

                    // Update the KDL buffer: rename the bind node
                    let text = get_buffer_text(&b3);
                    if let Ok(mut doc) = KdlDocument::from_str(&text) {
                        if let Some(binds_node) = doc
                            .nodes_mut()
                            .iter_mut()
                            .find(|n| n.name().value() == "binds")
                        {
                            if let Some(children) = binds_node.children_mut() {
                                if let Some(target) = children.nodes_mut().get_mut(chord_idx) {
                                    target.set_name(chord_str.as_str());
                                }
                            }
                        }
                        b3.set_text(&doc.to_string());
                    }

                    // Close dialog
                    if let Some(d) = dlg.upgrade() {
                        d.close();
                    }

                    glib::Propagation::Stop
                });

                // Add controller to content area
                content.add_controller(controller);
                dialog.set_child(Some(&content));

                // Present dialog
                dialog.present(None::<&gtk4::Window>);
            }
        });

        bind_row.add_row(&record_btn);

        // --- Action EntryRow ---
        let action_row = adw::EntryRow::new();
        action_row.set_title("Action");
        action_row.set_text(&entry.action);
        bind_row.add_row(&action_row);

        // Wire key chord changes (text entry)
        let b = buf.clone();
        let chord_idx2 = entry.index;
        chord_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                if let Some(binds_node) = doc
                    .nodes_mut()
                    .iter_mut()
                    .find(|n| n.name().value() == "binds")
                {
                    if let Some(children) = binds_node.children_mut() {
                        if let Some(target) = children.nodes_mut().get_mut(chord_idx2) {
                            target.set_name(row.text().as_str());
                        }
                    }
                }
                b.set_text(&doc.to_string());
            }
        });

        // Wire action changes
        let b = buf.clone();
        let chord_idx3 = entry.index;
        action_row.connect_changed(move |row| {
            let text = get_buffer_text(&b);
            if let Ok(mut doc) = KdlDocument::from_str(&text) {
                if let Some(binds_node) = doc
                    .nodes_mut()
                    .iter_mut()
                    .find(|n| n.name().value() == "binds")
                {
                    if let Some(children) = binds_node.children_mut() {
                        if let Some(target) = children.nodes_mut().get_mut(chord_idx3) {
                            // Parse the action text back into entries.
                            let action_text = row.text();
                            let parts: Vec<&str> = action_text.split_whitespace().collect();
                            let mut new_entries: Vec<KdlEntry> = Vec::new();
                            for part in parts {
                                if let Some(stripped) = part.strip_prefix('\"') {
                                    if let Some(end) = stripped.strip_suffix('\"') {
                                        new_entries
                                            .push(KdlEntry::new(KdlValue::String(end.to_string())));
                                    } else {
                                        new_entries.push(KdlEntry::new(KdlValue::String(
                                            stripped.to_string(),
                                        )));
                                    }
                                } else if let Ok(n) = part.parse::<i128>() {
                                    new_entries.push(KdlEntry::new(KdlValue::Integer(n)));
                                } else if let Ok(f) = part.parse::<f64>() {
                                    new_entries.push(KdlEntry::new(KdlValue::Float(f)));
                                } else {
                                    new_entries
                                        .push(KdlEntry::new(KdlValue::String(part.to_string())));
                                }
                            }
                            // Replace all entries
                            target.entries_mut().clear();
                            target.entries_mut().extend(new_entries);
                        }
                    }
                }
                b.set_text(&doc.to_string());
            }
        });

        group.add(&bind_row);
    }

    group.upcast()
}
