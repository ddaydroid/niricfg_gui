//! Layout section widget: gap, focus-ring, and border settings.
//!
//! # KDL structure
//!
//! ```kdl
//! layout {
//!     gap 8
//!     focus-ring {
//!         width 2
//!         active-color "#5294e2"
//!         off-color "#cccccc"
//!     }
//!     border {
//!         width 1
//!         active-color "#5294e2"
//!         inactive-color "#555555"
//!     }
//! }
//! ```

use kdl::{KdlDocument, KdlValue};
use libadwaita as adw;
use std::str::FromStr;

use crate::tools::niri::NiriTool;

use super::{get_buffer_text, get_kdl_f64, get_kdl_str, set_kdl_f64, set_kdl_value};

/// Build the "Layout" section widget tree.
pub fn build_layout_section(tool: &NiriTool, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Layout");
    group.set_description("Window gaps, focus ring, and border appearance");

    let init_doc = tool.doc().unwrap_or_default();

    // --- Gap SpinRow ---
    let gap_adj = gtk4::Adjustment::new(
        get_kdl_f64(&init_doc, &["layout", "gap"]).unwrap_or(8.0),
        0.0,
        64.0,
        1.0,
        4.0,
        0.0,
    );
    let gap_row = adw::SpinRow::new(&gap_adj, 1.0, 0);
    gap_row.set_title("Gap (px)");
    gap_row.set_tooltip_text(Some("Window gap in pixels (0–64)"));
    group.add(&gap_row);

    // --- Focus Ring sub-section ---
    let focus_row = adw::ExpanderRow::new();
    focus_row.set_title("Focus Ring");

    let init_fw = get_kdl_f64(&init_doc, &["layout", "focus-ring", "width"]).unwrap_or(2.0);
    let init_active = get_kdl_str(&init_doc, &["layout", "focus-ring", "active-color"])
        .unwrap_or_else(|| "#5294e2".to_string());
    let init_off = get_kdl_str(&init_doc, &["layout", "focus-ring", "off-color"])
        .unwrap_or_else(|| "#cccccc".to_string());

    let fw_adj = gtk4::Adjustment::new(init_fw, 0.0, 20.0, 1.0, 2.0, 0.0);
    let fw_row = adw::SpinRow::new(&fw_adj, 1.0, 0);
    fw_row.set_title("Width (px)");
    focus_row.add_row(&fw_row);

    let active_row = adw::EntryRow::new();
    active_row.set_title("Active Color");
    active_row.set_text(&init_active);
    active_row.set_tooltip_text(Some(
        "Hex color for focused window border (e.g. \"#5294e2\")",
    ));
    focus_row.add_row(&active_row);

    let off_row = adw::EntryRow::new();
    off_row.set_title("Inactive Color");
    off_row.set_text(&init_off);
    off_row.set_tooltip_text(Some(
        "Hex color for unfocused window border (e.g. \"#cccccc\")",
    ));
    focus_row.add_row(&off_row);

    group.add(&focus_row);

    // --- Border sub-section ---
    let border_row = adw::ExpanderRow::new();
    border_row.set_title("Border");

    let init_bw = get_kdl_f64(&init_doc, &["layout", "border", "width"]).unwrap_or(1.0);
    let init_b_active = get_kdl_str(&init_doc, &["layout", "border", "active-color"])
        .unwrap_or_else(|| "#5294e2".to_string());
    let init_b_inactive = get_kdl_str(&init_doc, &["layout", "border", "inactive-color"])
        .unwrap_or_else(|| "#555555".to_string());

    let bw_adj = gtk4::Adjustment::new(init_bw, 0.0, 20.0, 1.0, 2.0, 0.0);
    let bw_row = adw::SpinRow::new(&bw_adj, 1.0, 0);
    bw_row.set_title("Width (px)");
    border_row.add_row(&bw_row);

    let ba_row = adw::EntryRow::new();
    ba_row.set_title("Active Color");
    ba_row.set_text(&init_b_active);
    border_row.add_row(&ba_row);

    let bi_row = adw::EntryRow::new();
    bi_row.set_title("Inactive Color");
    bi_row.set_text(&init_b_inactive);
    border_row.add_row(&bi_row);

    group.add(&border_row);

    // --- Wire change handlers ---
    let buf = text_buffer.clone();

    let b = buf.clone();
    gap_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["layout", "gap"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    fw_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["layout", "focus-ring", "width"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    active_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_value(
                &mut doc,
                &["layout", "focus-ring", "active-color"],
                &KdlValue::String(row.text()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    off_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_value(
                &mut doc,
                &["layout", "focus-ring", "off-color"],
                &KdlValue::String(row.text()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    bw_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_f64(&mut doc, &["layout", "border", "width"], row.value());
            b.set_text(&doc.to_string());
        }
    });

    let b = buf.clone();
    ba_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_value(
                &mut doc,
                &["layout", "border", "active-color"],
                &KdlValue::String(row.text()),
            );
            b.set_text(&doc.to_string());
        }
    });

    let b = buf;
    bi_row.connect_changed(move |row| {
        let text = get_buffer_text(&b);
        if let Ok(mut doc) = KdlDocument::from_str(&text) {
            set_kdl_value(
                &mut doc,
                &["layout", "border", "inactive-color"],
                &KdlValue::String(row.text()),
            );
            b.set_text(&doc.to_string());
        }
    });

    group.upcast()
}
