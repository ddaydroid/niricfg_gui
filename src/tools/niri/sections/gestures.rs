//! Gestures section: swipe gesture and touchpad gesture settings.
//!
//! # KDL structure
//!
//! ```kdl
//! gesture-swipe-min-distance 20
//! gesture-swipe-finger-count 3
//! gesture-swipe-workspace-switch true
//! ```
//!
//! These are top-level nodes in the Niri config.

use adw::prelude::*;
use kdl::{KdlDocument, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::{get_buffer_text, get_kdl_f64, get_kdl_str, set_kdl_f64, set_kdl_value};

/// Build the "Gestures" section widget tree.
pub fn build_gestures_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Gestures");
    group.set_description(Some("Swipe and touchpad gesture settings"));

    let init_doc = tool.doc().unwrap_or_default();

    // --- Gesture Swipe Min Distance SpinRow ---
    let init_dist = get_kdl_f64(&init_doc, &["gesture-swipe-min-distance"]).unwrap_or(20.0);
    let dist_adj = gtk4::Adjustment::new(init_dist, 5.0, 200.0, 5.0, 10.0, 0.0);
    let dist_row = adw::SpinRow::new(Some(&dist_adj), 1.0, 0);
    dist_row.set_title("Min Swipe Distance (px)");
    dist_row.set_tooltip_text(Some(
        "Minimum distance in pixels to trigger a swipe gesture (5–200)",
    ));
    group.add(&dist_row);

    // --- Gesture Swipe Finger Count SpinRow ---
    let init_fingers = get_kdl_f64(&init_doc, &["gesture-swipe-finger-count"]).unwrap_or(3.0);
    let finger_adj = gtk4::Adjustment::new(init_fingers, 2.0, 4.0, 1.0, 1.0, 0.0);
    let finger_row = adw::SpinRow::new(Some(&finger_adj), 1.0, 0);
    finger_row.set_title("Finger Count");
    finger_row.set_tooltip_text(Some("Number of fingers for swipe gestures (2–4)"));
    group.add(&finger_row);

    // --- Gesture Swipe Workspace Switch SwitchRow ---
    let init_ws_switch = get_kdl_str(&init_doc, &["gesture-swipe-workspace-switch"])
        .map(|s| s == "true" || s == "1" || s == "yes")
        .unwrap_or(true);
    let ws_switch_row = adw::SwitchRow::new();
    ws_switch_row.set_title("Workspace Switch on Swipe");
    ws_switch_row.set_subtitle("Allow swipe gestures to switch workspaces");
    ws_switch_row.set_active(init_ws_switch);
    group.add(&ws_switch_row);

    // --- Gesture Swipe Fullscreen SwitchRow ---
    let init_fs_switch = get_kdl_str(&init_doc, &["gesture-swipe-fullscreen-switch"])
        .map(|s| s == "true" || s == "1" || s == "yes")
        .unwrap_or(true);
    let fs_switch_row = adw::SwitchRow::new();
    fs_switch_row.set_title("Fullscreen on Swipe");
    fs_switch_row.set_subtitle("Allow swipe gestures to toggle fullscreen");
    fs_switch_row.set_active(init_fs_switch);
    group.add(&fs_switch_row);

    // --- Wire change handlers ---
    let buf = text_buffer.clone();

    let b = buf.clone();
    dist_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["gesture-swipe-min-distance"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    finger_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["gesture-swipe-finger-count"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    ws_switch_row.connect_active_notify(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["gesture-swipe-workspace-switch"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf;
    fs_switch_row.connect_active_notify(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["gesture-swipe-fullscreen-switch"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    group.upcast()
}
