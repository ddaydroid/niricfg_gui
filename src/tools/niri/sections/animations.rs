//! Animations section: animation duration and slow-animations settings.
//!
//! # KDL structure
//!
//! ```kdl
//! animation-duration 250
//! slow-animations false
//! ```
//!
//! These are top-level nodes in the Niri config, not nested under a parent
//! block. Each is a simple node with a single value entry.

use adw::prelude::*;
use kdl::{KdlDocument, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::{get_buffer_text, get_kdl_f64, get_kdl_str, set_kdl_f64, set_kdl_value};

/// Build the "Animations" section widget tree.
pub fn build_animations_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Animations");
    group.set_description(Some("Animation speed and visual effects"));

    let init_doc = tool.doc().unwrap_or_default();

    // --- Animation Duration SpinRow ---
    let init_duration = get_kdl_f64(&init_doc, &["animation-duration"]).unwrap_or(250.0);
    let dur_adj = gtk4::Adjustment::new(init_duration, 0.0, 2000.0, 10.0, 50.0, 0.0);
    let dur_row = adw::SpinRow::new(Some(&dur_adj), 1.0, 0);
    dur_row.set_title("Duration (ms)");
    dur_row.set_tooltip_text(Some("Animation duration in milliseconds (0–2000)"));
    group.add(&dur_row);

    // --- Slow Animations SwitchRow ---
    let init_slow = get_kdl_str(&init_doc, &["slow-animations"])
        .map(|s| s == "true" || s == "1" || s == "yes")
        .unwrap_or(false);
    let slow_row = adw::SwitchRow::new();
    slow_row.set_title("Slow Animations");
    slow_row.set_subtitle("Enable slowed-down animations for accessibility");
    slow_row.set_active(init_slow);
    group.add(&slow_row);

    // --- Slow Animation Factor SpinRow ---
    let init_factor = get_kdl_f64(&init_doc, &["slow-animation-factor"]).unwrap_or(3.0);
    let factor_adj = gtk4::Adjustment::new(init_factor, 1.0, 20.0, 0.5, 1.0, 0.0);
    let factor_row = adw::SpinRow::new(Some(&factor_adj), 0.5, 1);
    factor_row.set_title("Slow Factor");
    factor_row.set_tooltip_text(Some("Multiplier for slow animations (1.0–20.0)"));
    group.add(&factor_row);

    // --- Wire change handlers ---
    let buf = text_buffer.clone();

    let b = buf.clone();
    dur_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["animation-duration"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    slow_row.connect_active_notify(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            let val = if row.is_active() { "true" } else { "false" };
            set_kdl_value(
                &mut doc,
                &["slow-animations"],
                &KdlValue::String(val.to_string()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf;
    factor_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["slow-animation-factor"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    group.upcast()
}
