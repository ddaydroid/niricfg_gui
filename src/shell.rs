//! GUI shell: GTK4 + libadwaita `Application` / `ApplicationWindow`. The
//! entire file is `#[cfg(feature = "gtk")]` so it contributes zero to a
//! no-default-features build (which is what the unit-test CI job runs).
//!
//! # Layout
//!
//! ```text
//! Adw.ApplicationWindow
//! └── Adw.NavigationSplitView
//!     ├── Sidebar: Adw.ToolbarView
//!     │   ├── HeaderBar ("Plugins" + [Compare] toggle)
//!     │   └── GtkListBox (one row per plugin)
//!     └── Content: GtkBox (vertical)
//!         ├── Adw.TabBar
//!         └── Adw.TabView (one tab per plugin)
//!             └── each tab: GtkBox (vertical)
//!                 ├── Adw.Banner (validation results)
//!                 └── GtkStack
//!                     ├── ["editor"] GtkScrolledWindow
//!                     │   └── GtkTextView (monospace, highlighted)
//!                     └── ["diff"]   GtkPaned (horizontal)
//!                         ├── line-numbered original (read-only)
//!                         └── line-numbered modified (read-only)
//! ```
//!
//! # Validation loop (Wave 2 Step 10)
//!
//! Each tab's `GtkTextBuffer::changed` signal drives a debounced async
//! validation loop that calls `tool.validator().validate_kdl(&text).await`
//! and updates the tab's `Adw.Banner`.
//!
//! # Syntax highlighting (Wave 3)
//!
//! KDL keywords, strings, numbers, comments, and punctuation are coloured
//! via `GtkTextTag` applied in a `::changed` handler. The tokeniser is a
//! simple char-by-char scanner in `core::kdl_highlighter`.
//!
//! # Side-by-side diff view (Wave 3)
//!
//! Each tab carries a `GtkStack` that toggles between `"editor"` (the
//! live GtkTextView with validation) and `"diff"` (a horizontal
//! `GtkPaned` showing the original saved content versus the current
//! editor text, with colour-coded line backgrounds for added/removed
//! lines and line-number gutters on both sides). A global `Compare`
//! toggle in the sidebar's HeaderBar switches all tabs between editor
//! and diff mode simultaneously. The diff is computed eagerly for every
//! tab when the toggle is activated, and stored per-tab so it persists
//! across tab switches without recomputation.

#![cfg(feature = "gtk")]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

// Re-export libadwaita under the shorter `adw` name used throughout
// this module (the underlying crate is `libadwaita` in 0.7.x).
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::core::diff::line_diff;
use crate::core::error::Error;
use crate::core::file_watcher::FileWatcher;
use crate::core::kdl_highlighter;
use crate::core::shell_state::ShellState;
use crate::core::state_persistence::{load_shell_window_state, save_shell_window_state};
use crate::DynTool;

#[cfg(feature = "gtk")]
use crate::tools::niri_shell::build_niri_sections;

/// Character displayed in the gutter for each diff status.
const GUTTER_SAME: char = ' ';
const GUTTER_ADDED: char = '+';
const GUTTER_REMOVED: char = '-';
const GUTTER_MODIFIED: char = '~';

/// Per-tab state needed by the global diff toggle and external-change
/// handler.
struct TabDiffState {
    stack: gtk4::Stack,
    editor_buf: gtk4::TextBuffer,
    original_text: Rc<RefCell<String>>,
    left_buf: gtk4::TextBuffer,
    right_buf: gtk4::TextBuffer,
    left_line_label: gtk4::Label,
    right_line_label: gtk4::Label,
    /// Weak reference to the parent window so we can present dialogs.
    window: glib::WeakRef<adw::ApplicationWindow>,
    /// The tool's config path on disk (for reloading content).
    config_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Line number gutter
// ---------------------------------------------------------------------------

/// Build one side of the diff view: a line-number gutter + a read-only
/// monospace text view, scroll-synced.
///
/// Returns `(outer_box, text_view, line_label, left_buf, right_buf)` where
/// `outer_box` goes into the GtkPaned, `text_view` is the editor,
/// `line_label` is the line-number widget (for later updates), and the
/// buffer belongs to the text view.
fn build_diff_editor_side() -> (gtk4::Box, gtk4::TextView, gtk4::Label) {
    // --- Line number gutter label ---
    let line_label = gtk4::Label::new(None);
    // set_monospace / set_font_desc are unavailable in gtk4 0.9.x, so
    // use CSS to set monospace font for the line-number gutter.
    // style_context() is deprecated since GTK 4.10 but still works;
    // the replacement (gtk::StyleManager) requires ≥4.12.
    #[allow(deprecated)]
    {
        let css_provider = gtk4::CssProvider::new();
        css_provider.load_from_string("* { font-family: monospace; }");
        line_label
            .style_context()
            .add_provider(&css_provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    line_label.set_xalign(1.0);
    line_label.set_valign(gtk4::Align::Start);
    line_label.set_margin_start(4);
    line_label.set_margin_end(4);
    line_label.set_width_chars(4);
    line_label.set_text("1");

    // Gutter sits in its own mini scrolled window (no scrollbars, no
    // user-scrollable) so it can track the text view's vadjustment.
    let gutter_sw = gtk4::ScrolledWindow::new();
    gutter_sw.set_hexpand(false);
    gutter_sw.set_vexpand(true);
    gutter_sw.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::External);
    gutter_sw.set_child(Some(&line_label));

    // --- Text view (read-only, no wrap) ---
    let text_view = gtk4::TextView::new();
    text_view.set_monospace(true);
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_wrap_mode(gtk4::WrapMode::None);
    text_view.set_margin_start(2);
    text_view.set_margin_end(6);
    text_view.set_margin_top(4);
    text_view.set_margin_bottom(4);

    // Editor sits in a normal scrollable window.
    let editor_sw = gtk4::ScrolledWindow::new();
    editor_sw.set_vexpand(true);
    editor_sw.set_hexpand(true);
    editor_sw.set_child(Some(&text_view));

    // --- Sync: gutter follows editor scroll ---
    let gutter_vadj = gutter_sw.vadjustment();
    let editor_vadj = editor_sw.vadjustment();
    editor_vadj.connect_value_changed(move |adj| {
        gutter_vadj.set_value(adj.value());
    });

    // --- Combine into a horizontal box ---
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    hbox.set_vexpand(true);
    hbox.set_hexpand(true);
    hbox.append(&gutter_sw);
    hbox.append(&editor_sw);

    (hbox, text_view, line_label)
}

/// Update the line-number label to show 1..N for N lines of buffer content.
fn update_line_numbers(line_label: &gtk4::Label, buf: &gtk4::TextBuffer) {
    let line_count = buf.line_count();
    let nums: String = (1..=line_count)
        .map(|n| format!("{n:>3}"))
        .collect::<Vec<_>>()
        .join("\n");
    line_label.set_text(&nums);
}

// ---------------------------------------------------------------------------
// Diff widget construction
// ---------------------------------------------------------------------------

/// Build the side-by-side diff widget with line numbers on both sides.
///
/// Returns the paned, the left (original) buffer, the right (modified)
/// buffer, and both line labels (for later updates).
struct DiffWidget {
    paned: gtk4::Paned,
    left_buf: gtk4::TextBuffer,
    right_buf: gtk4::TextBuffer,
    left_line_label: gtk4::Label,
    right_line_label: gtk4::Label,
}

fn build_diff_widget() -> DiffWidget {
    let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    paned.set_wide_handle(true);
    paned.set_position(500);

    // Left original side.
    let (left_box, left_view, left_line_label) = build_diff_editor_side();
    let left_buf = left_view.buffer();

    // Right modified side.
    let (right_box, right_view, right_line_label) = build_diff_editor_side();
    let right_buf = right_view.buffer();

    // --- Colour tags ---
    fn make_diff_tag(buf: &gtk4::TextBuffer, name: &str, bg: &str, fg: &str) -> gtk4::TextTag {
        let tag = buf.create_tag(Some(name), &[]).expect("create_tag failed");
        tag.set_background(Some(bg));
        tag.set_foreground(Some(fg));
        tag
    }

    make_diff_tag(&right_buf, "diff_added", "#1b4a1b", "#a3be8c");
    make_diff_tag(&left_buf, "diff_removed", "#4a1b1b", "#bf616a");
    make_diff_tag(&left_buf, "diff_mod_left", "#3d3520", "#d08770");
    make_diff_tag(&right_buf, "diff_mod_right", "#3d3520", "#d08770");

    // --- Sync vertical scrolling between left and right sides ---
    // The editor_sw is the second child of each hbox. We need to reach
    // into the hbox children to get the editor scrolled windows.
    let left_editor_sw = find_editor_sw_in_side(&left_box);
    let right_editor_sw = find_editor_sw_in_side(&right_box);

    let left_vadj = left_editor_sw.vadjustment();
    let right_vadj = right_editor_sw.vadjustment();
    let syncing = Rc::new(RefCell::new(false));

    let syncing_l = syncing.clone();
    let right_adj = right_vadj.clone();
    left_vadj.connect_value_changed(move |adj| {
        if !*syncing_l.borrow() {
            *syncing_l.borrow_mut() = true;
            right_adj.set_value(adj.value());
            *syncing_l.borrow_mut() = false;
        }
    });

    let syncing_r = syncing.clone();
    let left_adj = left_vadj.clone();
    right_vadj.connect_value_changed(move |adj| {
        if !*syncing_r.borrow() {
            *syncing_r.borrow_mut() = true;
            left_adj.set_value(adj.value());
            *syncing_r.borrow_mut() = false;
        }
    });

    paned.set_start_child(Some(&left_box));
    paned.set_end_child(Some(&right_box));

    DiffWidget {
        paned,
        left_buf,
        right_buf,
        left_line_label,
        right_line_label,
    }
}

/// Walk the children of a side's hbox to find the editor ScrolledWindow
/// (second child). The first child is the gutter mini-scrolled-window.
fn find_editor_sw_in_side(hbox: &gtk4::Box) -> gtk4::ScrolledWindow {
    // We know children[0] = gutter_sw, children[1] = editor_sw.
    // In GTK4, we can use observe_children() or first_child / next_sibling.
    let child = hbox
        .first_child()
        .and_then(|gutter| gutter.next_sibling())
        .expect("diff side hbox must have exactly 2 children");
    child
        .downcast::<gtk4::ScrolledWindow>()
        .expect("second child must be ScrolledWindow")
}

// ---------------------------------------------------------------------------
// Diff population
// ---------------------------------------------------------------------------

/// Populate the two diff buffers from the original and modified text, and
/// update both line-number labels.
fn populate_diff_view(
    left_buf: &gtk4::TextBuffer,
    right_buf: &gtk4::TextBuffer,
    left_line_label: &gtk4::Label,
    right_line_label: &gtk4::Label,
    original: &str,
    modified: &str,
) {
    left_buf.set_text("");
    right_buf.set_text("");

    let ops = line_diff(original, modified);

    // Build aligned line arrays.
    let mut left_lines: Vec<String> = Vec::new();
    let mut right_lines: Vec<String> = Vec::new();
    let mut left_states: Vec<&str> = Vec::new();
    let mut right_states: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < ops.len() {
        use crate::core::diff::DiffLine;
        match &ops[i] {
            DiffLine::Same(s) => {
                left_lines.push(s.clone());
                right_lines.push(s.clone());
                left_states.push("same");
                right_states.push("same");
                i += 1;
            }
            DiffLine::Removed(s) => {
                if i + 1 < ops.len() && matches!(ops[i + 1], DiffLine::Added(_)) {
                    if let DiffLine::Added(a) = &ops[i + 1] {
                        left_lines.push(s.clone());
                        right_lines.push(a.clone());
                        left_states.push("modified");
                        right_states.push("modified");
                        i += 2;
                    } else {
                        unreachable!()
                    }
                } else {
                    left_lines.push(s.clone());
                    right_lines.push(String::new());
                    left_states.push("removed");
                    right_states.push("empty");
                    i += 1;
                }
            }
            DiffLine::Added(s) => {
                left_lines.push(String::new());
                right_lines.push(s.clone());
                left_states.push("empty");
                right_states.push("added");
                i += 1;
            }
        }
    }

    // Build text with gutter markers.
    let left_full = left_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let gutter = match left_states[i] {
                "same" => GUTTER_SAME,
                "removed" => GUTTER_REMOVED,
                "modified" => GUTTER_MODIFIED,
                _ => ' ',
            };
            format!("{gutter} {l}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let right_full = right_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let gutter = match right_states[i] {
                "same" => GUTTER_SAME,
                "added" => GUTTER_ADDED,
                "modified" => GUTTER_MODIFIED,
                _ => ' ',
            };
            format!("{gutter} {l}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    left_buf.set_text(&left_full);
    right_buf.set_text(&right_full);

    // Update line numbers.
    update_line_numbers(left_line_label, left_buf);
    update_line_numbers(right_line_label, right_buf);

    // Apply colour tags to left side.
    let left_tt = left_buf.tag_table();
    for (i, state) in left_states.iter().enumerate() {
        let line_start = left_lines[..i].iter().map(|l| l.len() + 3).sum::<usize>() + 2;
        let line_end = line_start + left_lines[i].len();

        let tag_name = match *state {
            "removed" => Some("diff_removed"),
            "modified" => Some("diff_mod_left"),
            _ => None,
        };
        if let Some(name) = tag_name {
            if let Some(tag) = left_tt.lookup(name) {
                let s = left_buf.iter_at_offset(line_start as i32);
                let e = left_buf.iter_at_offset(line_end as i32);
                left_buf.apply_tag(&tag, &s, &e);
            }
        }
    }

    // Apply colour tags to right side.
    let right_tt = right_buf.tag_table();
    for (i, state) in right_states.iter().enumerate() {
        let line_start = right_lines[..i].iter().map(|l| l.len() + 3).sum::<usize>() + 2;
        let line_end = line_start + right_lines[i].len();

        let tag_name = match *state {
            "added" => Some("diff_added"),
            "modified" => Some("diff_mod_right"),
            _ => None,
        };
        if let Some(name) = tag_name {
            if let Some(tag) = right_tt.lookup(name) {
                let s = right_buf.iter_at_offset(line_start as i32);
                let e = right_buf.iter_at_offset(line_end as i32);
                right_buf.apply_tag(&tag, &s, &e);
            }
        }
    }
}

/// Refresh a single tab's diff buffers from its current editor text.
fn refresh_tab_diff(state: &TabDiffState) {
    let modified = state
        .editor_buf
        .text(
            &state.editor_buf.start_iter(),
            &state.editor_buf.end_iter(),
            false,
        )
        .to_string();
    let original = state.original_text.borrow().clone();
    populate_diff_view(
        &state.left_buf,
        &state.right_buf,
        &state.left_line_label,
        &state.right_line_label,
        &original,
        &modified,
    );
}

/// Switch a tab's stack to "editor" or "diff" based on the global toggle.
fn set_tab_mode(state: &TabDiffState, show_diff: bool) {
    state
        .stack
        .set_visible_child_name(if show_diff { "diff" } else { "editor" });
}

// ---------------------------------------------------------------------------
// Editor tab construction
// ---------------------------------------------------------------------------

/// Build one editor tab page: banner + stack (editor ↔ diff view), with KDL
/// highlighting and debounced async validation loop.
///
/// `initial_text` is the on-disk content of the tool's config file — it is
/// loaded into the text view and also stored as the diff baseline (original
/// text) so the "Compare" toggle shows meaningful differences against the
/// saved state rather than against an empty buffer.
///
/// Does NOT include a per-tab diff toggle — that lives in the sidebar's
/// HeaderBar as a global toggle.
///
/// Returns the container widget, the banner, the text view, and the diff
/// state struct so `run_shell` can collect it for the global toggle.
fn build_editor_page(
    tool_index: usize,
    tools: Rc<Vec<DynTool>>,
    tab_diff_state: &mut Vec<TabDiffState>,
    initial_text: &str,
    shell_state: ShellState,
) -> (gtk4::Box, adw::Banner, gtk4::TextView) {
    // --- Banner (validation results) ---
    let banner = adw::Banner::new("");
    banner.set_revealed(false);

    // --- Text editor ---
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_monospace(true);
    text_view.set_vexpand(true);
    text_view.set_hexpand(true);
    text_view.set_margin_start(8);
    text_view.set_margin_end(8);
    text_view.set_margin_top(4);
    text_view.set_margin_bottom(4);
    scrolled.set_child(Some(&text_view));

    // Load initial content into the text view.
    text_view.buffer().set_text(initial_text);

    // Apply KDL syntax highlighting.
    kdl_highlighter::apply_highlighting(&text_view.buffer());

    // --- Original text snapshot (used for diff) — initialised from file ---
    let original_text: Rc<RefCell<String>> = Rc::new(RefCell::new(initial_text.to_string()));

    // --- Diff view ---
    let diff_widget = build_diff_widget();

    // --- Stack: sections / editor / diff ---
    // For NiriTool (Wave 3 Step 10), add a "Sections" tab with structured
    // widget editors (SpinRow, SwitchRow, etc.) before the raw text editor.
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // Try to build section widgets for this tool (NiriTool v1).
    if let Some(sections_widget) = build_niri_sections(&*tools[tool_index], &text_view.buffer()) {
        stack.add_titled(&sections_widget, Some("sections"), "Sections");
    }

    stack.add_titled(&scrolled, Some("raw"), "Raw");
    stack.add_titled(&diff_widget.paned, Some("diff"), "Diff");

    // --- Parser-fallback view (Wave 4 Step 15) ---
    // When the buffer text fails to parse as KDL, the stack switches to this
    // view instead of the sections/raw editor. The user sees the raw text
    // with the error span highlighted in red, and a "Restore GUI" button
    // that re-parses and restores the structured section view on success.
    let fallback_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    fallback_box.set_vexpand(true);

    let parse_error_label = gtk4::Label::new(None::<&str>);
    parse_error_label.set_xalign(0.0);
    parse_error_label.set_margin_start(8);
    parse_error_label.set_margin_end(8);
    parse_error_label.set_margin_top(12);
    parse_error_label.add_css_class("error");
    parse_error_label.add_css_class("heading");
    parse_error_label.set_text("KDL Parse Error");
    fallback_box.append(&parse_error_label);

    let parse_error_desc = gtk4::Label::new(None::<&str>);
    parse_error_desc.set_xalign(0.0);
    parse_error_desc.set_wrap(true);
    parse_error_desc.set_margin_start(8);
    parse_error_desc.set_margin_end(8);
    parse_error_desc.set_margin_bottom(4);
    parse_error_desc.set_sensitive(false);
    fallback_box.append(&parse_error_desc);

    let fallback_scrolled = gtk4::ScrolledWindow::new();
    fallback_scrolled.set_vexpand(true);
    fallback_scrolled.set_hexpand(true);

    let fallback_view = gtk4::TextView::new();
    fallback_view.set_buffer(Some(&text_view.buffer())); // shared buffer
    fallback_view.set_monospace(true);
    fallback_view.set_vexpand(true);
    fallback_view.set_hexpand(true);
    fallback_view.set_wrap_mode(gtk4::WrapMode::Word);
    fallback_view.set_margin_start(8);
    fallback_view.set_margin_end(8);
    fallback_view.set_margin_top(4);
    fallback_view.set_margin_bottom(4);
    // KDL highlighting is already applied to the shared buffer, so the
    // fallback view inherits it automatically.
    fallback_scrolled.set_child(Some(&fallback_view));
    fallback_box.append(&fallback_scrolled);

    // Error span tag — red background to highlight the parse error location
    // in the fallback text view (offset from kdl::KdlDiagnostic::span).
    let parse_error_tag = text_view
        .buffer()
        .create_tag(Some("parse_error"), &[])
        .expect("create_tag failed");
    parse_error_tag.set_background(Some("#5a1b1b"));

    // Restore GUI button: re-parses the buffer and rebuilds sections on success.
    let restore_btn = gtk4::Button::with_label("Restore GUI");
    restore_btn.add_css_class("suggested-action");
    restore_btn.add_css_class("pill");
    restore_btn.set_halign(gtk4::Align::Center);
    restore_btn.set_margin_top(8);
    restore_btn.set_margin_bottom(8);
    fallback_box.append(&restore_btn);

    stack.add_titled(&fallback_box, Some("fallback"), "Fallback");

    // Push TabDiffState so the global toggle & external-change handler
    // can manage this tab. The window reference is set later by run_shell
    // (the window is not yet created at build_editor_page time).
    tab_diff_state.push(TabDiffState {
        stack: stack.clone(),
        editor_buf: text_view.buffer(),
        original_text: original_text.clone(),
        left_buf: diff_widget.left_buf,
        right_buf: diff_widget.right_buf,
        left_line_label: diff_widget.left_line_label,
        right_line_label: diff_widget.right_line_label,
        window: glib::WeakRef::new(),
        config_path: None,
    });

    // --- Layout: banner + stack ---
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.set_vexpand(true);
    vbox.append(&banner);
    vbox.append(&stack);

    // --- Debounce state (validation loop) ---
    let debounce_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    // --- Parser-fallback closures (Wave 4 Step 15) ---
    // Pre-clone widgets for connect_changed and restore button before
    // moving originals into the connect_changed closure.
    let parse_stack = stack.clone();
    let parse_error_label_c = parse_error_label.clone();
    let parse_error_desc_c = parse_error_desc.clone();
    let parse_error_tag_c = parse_error_tag.clone();

    let restore_tools = tools.clone();
    let restore_stack = stack.clone();
    let restore_buf = text_view.buffer();
    let restore_tool_index = tool_index;
    let restore_error_desc = parse_error_desc.clone();
    let restore_error_tag = parse_error_tag.clone();

    // Core captures for connect_changed (consumes originals via move).
    let tools_clone = tools;
    let banner_clone = banner.clone();
    let debounce_clone = debounce_source.clone();
    let buffer = text_view.buffer();
    let shell_state_c = shell_state.clone();
    let original_text_c = original_text.clone();

    buffer.connect_changed(move |buf| {
        // Cancel pending debounce timer.
        if let Some(source_id) = debounce_clone.borrow_mut().take() {
            source_id.remove();
        }

        // Debounce interval from validator.
        let debounce_ms = tools_clone
            .get(tool_index)
            .and_then(|tool| tool.validator())
            .map(|v| v.debounce_hint())
            .unwrap_or(Duration::from_millis(250));

        // Snapshot text.
        let start = buf.start_iter();
        let end = buf.end_iter();
        let text: String = buf.text(&start, &end, false).to_string();

        let timer_tools = tools_clone.clone();
        let timer_banner = banner_clone.clone();
        let timer_shell = shell_state_c.clone();

        let id = glib::timeout_add_local(debounce_ms, move || {
            let validate_tools = timer_tools.clone();
            let validate_banner = timer_banner.clone();
            let validate_text = text.clone();
            let validate_shell = timer_shell.clone();

            glib::MainContext::default().spawn_local(async move {
                if let Some(tool) = validate_tools.get(tool_index) {
                    if let Some(validator) = tool.validator() {
                        match validator.validate_kdl(&validate_text).await {
                            Ok(issues) => {
                                if issues.is_empty() {
                                    validate_banner.set_revealed(false);
                                    validate_shell.set_last_validation(None);
                                } else {
                                    let count = issues.len();
                                    let title = if count == 1 {
                                        format!("{}: {}", validator.name(), issues[0].message)
                                    } else {
                                        format!(
                                            "{} ({} issues): {}",
                                            validator.name(),
                                            count,
                                            issues[0].message,
                                        )
                                    };
                                    validate_banner.set_title(&title);
                                    validate_banner.set_revealed(true);
                                    validate_shell
                                        .set_last_validation(Some(&format!("{count} issue(s)")));
                                }
                            }
                            Err(e) => {
                                validate_banner.set_title(&format!("Validation error: {e}"));
                                validate_banner.set_revealed(true);
                                validate_shell
                                    .set_last_validation(Some(&format!("Validation error: {e}")));
                            }
                        }
                    }
                }
            });

            glib::ControlFlow::Break
        });

        *debounce_clone.borrow_mut() = Some(id);

        // --- Dirty flag tracking via ShellState (Wave 0 Step 3) ---
        let cur_text: String = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        let saved = original_text_c.borrow();
        shell_state_c.set_dirty(cur_text != *saved);
        drop(saved);

        // --- Parser-Fallback detection (Wave 4 Step 15) ---
        // Whenever the buffer changes, try to parse the text as KDL.
        // If parsing fails and the stack is in sections/raw mode, switch
        // to the fallback view with the error span highlighted.
        // If parsing succeeds and the stack is in fallback mode, restore
        // the sections view.
        let parse_text: String = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();

        match kdl::KdlDocument::from_str(&parse_text) {
            Ok(_) => {
                // Parse succeeded. If currently in fallback mode, restore sections.
                if parse_stack.visible_child_name().as_deref() == Some("fallback") {
                    // Clear the error highlighting from the buffer.
                    let clear_start = buf.start_iter();
                    let clear_end = buf.end_iter();
                    buf.remove_tag(&parse_error_tag_c, &clear_start, &clear_end);
                    // Switch back to sections (structured view) or raw as fallback.
                    if parse_stack.child_by_name("sections").is_some() {
                        parse_stack.set_visible_child_name("sections");
                    } else {
                        parse_stack.set_visible_child_name("raw");
                    }
                }
            }
            Err(err) => {
                // Parse failed. Switch to fallback if not already there
                // (and not in diff mode, which the user explicitly selected).
                let current = parse_stack.visible_child_name();
                if current.as_deref() != Some("fallback") && current.as_deref() != Some("diff") {
                    // Update the error banner with details from the first diagnostic.
                    if let Some(diag) = err.diagnostics.first() {
                        let msg = diag.message.clone().unwrap_or_default();
                        let help = diag.help.clone().unwrap_or_default();
                        let detail = if help.is_empty() {
                            msg
                        } else {
                            format!("{msg}\n\nSuggestion: {help}")
                        };
                        parse_error_desc_c.set_text(&detail);

                        // Highlight the offending span in red.
                        let offset = diag.span.offset();
                        let len = diag.span.len().max(1);
                        let hi_start = buf.iter_at_offset(offset as i32);
                        let hi_end = buf.iter_at_offset((offset + len) as i32);
                        buf.apply_tag(&parse_error_tag_c, &hi_start, &hi_end);
                    } else {
                        parse_error_desc_c.set_text("Unknown parsing error");
                    }

                    // Hide the validation banner when entering fallback — the
                    // KDL parse failure is the more actionable issue.
                    parse_error_label_c.set_text("KDL Parse Error");
                    banner_clone.set_revealed(false);
                    parse_stack.set_visible_child_name("fallback");
                }
            }
        }
    });

    // --- Wire the Restore GUI button ---
    // Re-parses the current buffer text. On success, rebuilds the sections
    // widget tree (because the old sections were built from stale state)
    // and switches the stack back to sections view. On failure, updates
    // the error display with the new diagnostic.
    restore_btn.connect_clicked(move |_| {
        let text: String = restore_buf
            .text(&restore_buf.start_iter(), &restore_buf.end_iter(), false)
            .to_string();

        match kdl::KdlDocument::from_str(&text) {
            Ok(_) => {
                // Clear parse error highlighting from the buffer.
                let clear_start = restore_buf.start_iter();
                let clear_end = restore_buf.end_iter();
                restore_buf.remove_tag(&restore_error_tag, &clear_start, &clear_end);

                // Parse succeeded — rebuild sections and restore GUI.
                if let Some(tool) = restore_tools.get(restore_tool_index) {
                    // Remove the old sections page from the stack.
                    if let Some(old_child) = restore_stack.child_by_name("sections") {
                        restore_stack.remove(&old_child);
                    }
                    // Build new sections widget from current tool state.
                    if let Some(new_sections) = build_niri_sections(&**tool, &restore_buf) {
                        restore_stack.add_titled(&new_sections, Some("sections"), "Sections");
                        restore_stack.set_visible_child_name("sections");
                    } else {
                        restore_stack.set_visible_child_name("raw");
                    }
                }
            }
            Err(err) => {
                // Still failing — update the error display in the fallback view.
                // Clear old highlight first.
                let clear_start = restore_buf.start_iter();
                let clear_end = restore_buf.end_iter();
                restore_buf.remove_tag(&restore_error_tag, &clear_start, &clear_end);

                if let Some(diag) = err.diagnostics.first() {
                    let msg = diag.message.clone().unwrap_or_default();
                    let help = diag.help.clone().unwrap_or_default();
                    let detail = if help.is_empty() {
                        msg
                    } else {
                        format!("{msg}\n\nSuggestion: {help}")
                    };
                    restore_error_desc.set_text(&detail);

                    // Highlight the new error span.
                    let offset = diag.span.offset();
                    let len = diag.span.len().max(1);
                    let hi_start = restore_buf.iter_at_offset(offset as i32);
                    let hi_end = restore_buf.iter_at_offset((offset + len) as i32);
                    restore_buf.apply_tag(&restore_error_tag, &hi_start, &hi_end);
                } else {
                    restore_error_desc.set_text("Unknown parsing error");
                }
                // Make sure we stay on the fallback view.
                restore_stack.set_visible_child_name("fallback");
            }
        }
    });

    (vbox, banner, text_view)
}

// ---------------------------------------------------------------------------
// External-change handling
// ---------------------------------------------------------------------------

/// Handle an external file change detected by the FileWatcher.
///
/// - If the editor is **clean** (no unsaved edits), the content is silently
///   reloaded and the original_text snapshot is updated.
/// - If the editor is **dirty**, an `adw::AlertDialog` is presented with
///   "Reload" (discard edits and load on-disk content) and "Ignore" (keep
///   editor state as-is).
fn handle_external_change(
    tab_states: &Rc<RefCell<Vec<TabDiffState>>>,
    changed_path: &std::path::Path,
    shell_state: &ShellState,
) {
    let states = tab_states.borrow();
    // Find the tab whose config_path matches the changed path.
    let tab_idx = states
        .iter()
        .position(|s| s.config_path.as_ref().is_some_and(|p| p == changed_path));

    let idx = match tab_idx {
        Some(i) => i,
        None => return, // no tab cares about this path
    };

    let state = &states[idx];

    // Read new file content.
    let new_text = match std::fs::read_to_string(changed_path) {
        Ok(t) => t,
        Err(_) => return, // file disappeared; silently ignore
    };

    // Check if dirty: compare current editor text against original snapshot.
    let current = state
        .editor_buf
        .text(
            &state.editor_buf.start_iter(),
            &state.editor_buf.end_iter(),
            false,
        )
        .to_string();
    let original = state.original_text.borrow();
    let is_dirty = current != *original;
    drop(original);

    if is_dirty {
        // --- Dirty → show conflict dialog ---
        let window_opt = state.window.upgrade();
        let Some(window) = window_opt else { return };

        let dialog = adw::AlertDialog::new(
            Some("External Change Detected"),
            Some(
                "The config file was modified by another application. \
                  Reloading will discard your unsaved changes.",
            ),
        );
        dialog.add_response("ignore", "_Ignore");
        dialog.add_response("reload", "_Reload");
        dialog.set_default_response(Some("ignore"));
        dialog.set_close_response("ignore");

        let st = tab_states.clone();
        let ss = shell_state.clone();
        dialog.connect_response(None, move |_dlg, response| {
            if response == "reload" {
                let states = st.borrow_mut();
                if let Some(s) = states.get(idx) {
                    let txt = new_text.clone();
                    s.editor_buf.set_text(&txt);
                    *s.original_text.borrow_mut() = txt;
                    ss.set_dirty(false);
                }
            }
            // "ignore": do nothing, keep editor state intact
        });

        dialog.present(Some(&window));
    } else {
        // --- Clean → silent reload ---
        state.editor_buf.set_text(&new_text);
        *state.original_text.borrow_mut() = new_text;
        shell_state.set_dirty(false);
    }
}

// ---------------------------------------------------------------------------
// Shell entrypoint
// ---------------------------------------------------------------------------

/// Launch the dotcfg-gui GUI shell.
///
/// `plugins` is the list of `Box<dyn ToolPlugin>` built by `main`. The
/// shell wraps them into a sidebar (plugin list) + tab view (per-plugin
/// editor), each tab carrying its own debounced async validator loop,
/// KDL syntax highlighting, and a side-by-side diff view with line
/// numbers. A global `Compare` toggle in the sidebar's HeaderBar
/// switches all tabs between editor and diff mode simultaneously.
///
/// A `ToolRegistry` is not used here because `Box<dyn ToolPlugin>` does
/// not implement `Clone` (dyn traits don't carry Clone), so the plugins
/// are stored in an `Rc<Vec<DynTool>>` and iterated by reference — the
/// Vec itself serves the registry role for the shell's lifetime.
pub fn run_shell(plugins: Vec<DynTool>) -> Result<(), Error> {
    let app = adw::Application::new(Some("com.d3t0x.niricfg"), Default::default());
    let tools = Rc::new(plugins);
    let tab_diff_states: Rc<RefCell<Vec<TabDiffState>>> = Rc::new(RefCell::new(Vec::new()));
    let shell_state = ShellState::default();

    app.connect_activate(move |app| {
        // --- Load persisted window state ---
        let saved = load_shell_window_state();

        let window = adw::ApplicationWindow::new(app);
        window.set_default_size(saved.width, saved.height);
        window.set_title(Some("dotcfg-gui — niri config editor"));

        // --- Main stack: editor view overlay with first-run status page ---
        let main_stack = gtk4::Stack::new();

        // --- Navigation split view: sidebar | content ---
        let split_view = adw::NavigationSplitView::new();
        split_view.set_sidebar_width_fraction(0.25);
        split_view.set_min_sidebar_width(180.0);
        split_view.set_max_sidebar_width(350.0);

        // --- Sidebar: plugin list in a ToolbarView ---
        let sidebar = adw::ToolbarView::new();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&gtk4::Label::new(Some("Plugins"))));

        // --- HeaderBar action buttons ---
        // Undo / Redo on the left side.
        let undo_btn = gtk4::Button::new();
        undo_btn.set_icon_name("edit-undo-symbolic");
        undo_btn.set_tooltip_text(Some("Undo last edit"));
        sidebar_header.pack_start(&undo_btn);

        let redo_btn = gtk4::Button::new();
        redo_btn.set_icon_name("edit-redo-symbolic");
        redo_btn.set_tooltip_text(Some("Redo last undone edit"));
        sidebar_header.pack_start(&redo_btn);

        // Save button on the right side (before Compare toggle).
        let save_btn = gtk4::Button::new();
        save_btn.set_icon_name("document-save-symbolic");
        save_btn.set_tooltip_text(Some("Save changes to disk"));
        save_btn.set_sensitive(false); // starts disabled (no edits yet)
        sidebar_header.pack_end(&save_btn);

        // Bind Save button sensitivity to ShellState's dirty flag.
        // When is-dirty is true → save button is enabled.
        // The `SYNC_CREATE` flag initialises the target property immediately.
        shell_state
            .bind_property("is-dirty", &save_btn, "sensitive")
            .flags(glib::BindingFlags::SYNC_CREATE)
            .build();

        // --- is_saving guard: suppresses file-watcher backfeed during save ---
        // Must be declared before the Save button and watcher closures.
        let is_saving: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

        // Wire Save button to persist all dirty tabs to disk.
        // Sets is_saving guard to suppress watcher backfeed for 250ms.
        let save_btn_states = tab_diff_states.clone();
        let save_btn_shell = shell_state.clone();
        let save_btn_saving = is_saving.clone();
        save_btn.connect_clicked(move |_| {
            save_btn_saving.store(true, std::sync::atomic::Ordering::Relaxed);
            let states = save_btn_states.borrow();
            let mut any_saved = false;
            for state in states.iter() {
                let current = state
                    .editor_buf
                    .text(
                        &state.editor_buf.start_iter(),
                        &state.editor_buf.end_iter(),
                        false,
                    )
                    .to_string();
                let original = state.original_text.borrow();
                if current != *original {
                    if let Some(ref path) = state.config_path {
                        if std::fs::write(path, &current).is_ok() {
                            *state.original_text.borrow_mut() = current;
                            any_saved = true;
                        }
                    }
                }
            }
            drop(states);
            if any_saved {
                save_btn_shell.set_dirty(false);
            }
            // Release the save guard after 250ms so the watcher resumes.
            let saving = save_btn_saving.clone();
            glib::timeout_add_local(Duration::from_millis(250), move || {
                saving.store(false, std::sync::atomic::Ordering::Relaxed);
                glib::ControlFlow::Break
            });
        });

        // Global diff toggle.
        let diff_toggle = gtk4::ToggleButton::with_label("Compare");
        diff_toggle.set_tooltip_text(Some(
            "Show side-by-side diff against saved version for all tabs",
        ));
        sidebar_header.pack_end(&diff_toggle);

        sidebar.add_top_bar(&sidebar_header);

        let plugin_list = gtk4::ListBox::new();
        plugin_list.set_selection_mode(gtk4::SelectionMode::Single);
        sidebar.set_content(Some(&plugin_list));

        // set_sidebar requires an IsA<NavigationPage> wrapper.
        let sidebar_page = adw::NavigationPage::new(&sidebar, "Plugins");
        split_view.set_sidebar(Some(&sidebar_page));

        // --- Content: tab bar + tab view ---
        let tab_bar = adw::TabBar::new();
        let tab_view = adw::TabView::new();
        tab_bar.set_view(Some(&tab_view));

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.set_vexpand(true);
        content_box.append(&tab_bar);
        content_box.append(&tab_view);

        // set_content also requires an IsA<NavigationPage> wrapper.
        let content_page = adw::NavigationPage::new(&content_box, "Editor");
        split_view.set_content(Some(&content_page));

        main_stack.add_titled(&split_view, Some("editor"), "Editor");

        // --- First-run status page ---
        let status_page = adw::StatusPage::new();
        status_page.set_title("Welcome to dotcfg-gui");
        status_page.set_description(Some(
            "No configuration file found. \
             Generate a default config to get started editing.",
        ));
        status_page.set_icon_name(Some("preferences-system"));

        let gen_button = gtk4::Button::with_label("Generate Default Config");
        gen_button.set_halign(gtk4::Align::Center);
        gen_button.set_valign(gtk4::Align::Start);
        gen_button.set_margin_top(24);
        gen_button.add_css_class("suggested-action");
        gen_button.add_css_class("pill");
        status_page.set_child(Some(&gen_button));

        main_stack.add_titled(&status_page, Some("first-run"), "First Run");

        window.set_content(Some(&main_stack));

        // Check whether any tool has an existing config, then show the
        // appropriate view.
        let has_any_config = tools
            .as_slice()
            .iter()
            .any(|t| t.config_paths().iter().any(|p| p.exists()));

        // --- Populate sidebar rows + editor tabs ---
        {
            let mut states = tab_diff_states.borrow_mut();
            for (i, tool) in tools.as_slice().iter().enumerate() {
                let row = gtk4::ListBoxRow::new();
                let label = gtk4::Label::new(Some(tool.display_name()));
                label.set_margin_start(12);
                label.set_margin_end(12);
                label.set_margin_top(6);
                label.set_margin_bottom(6);
                label.set_halign(gtk4::Align::Start);
                row.set_child(Some(&label));
                plugin_list.append(&row);

                // Read the tool's config file from disk to populate the editor.
                let config_path = tool.config_paths().iter().find(|p| p.exists()).cloned();
                let initial_text = config_path
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_default();

                // Call load() so the tool updates its internal state.
                if let Some(ref path) = config_path {
                    let _ = tool.load(path);
                }

                let (editor_widget, _banner, _text_view) = build_editor_page(
                    i,
                    tools.clone(),
                    &mut states,
                    &initial_text,
                    shell_state.clone(),
                );
                let page = tab_view.append(&editor_widget);
                page.set_title(tool.display_name());
            }
        }

        // --- Show editor or first-run depending on config presence ---
        if has_any_config {
            main_stack.set_visible_child_name("editor");
        } else {
            main_stack.set_visible_child_name("first-run");
        }

        // --- Start the async file watcher for external-edit detection ---
        // Using a channel-based approach: the watcher thread sends PathBuf
        // events through an mpsc channel; a glib timeout on the GTK main
        // thread polls the receiver and dispatches to handle_external_change.
        // This avoids sharing GTK (non-Send) state across thread boundaries.
        //
        // The closure below is reusable so it can be called both at initial
        // startup (when configs already exist) AND after the first-run
        // "Generate Default Config" button creates a file. It is defined
        // here before the button handler so it's in scope for the closure.
        //
        // `is_saving` is declared earlier (before the Save button) so both
        // the save handler and the watcher can access it.

        // Use Rc<ShellState> for shared ownership across the nested closure
        // chain. Each `move` closure captures the Rc by value (cheap clone of
        // the Rc pointer), avoiding move conflicts with other shell_state users.
        let watcher_shell: Rc<ShellState> = Rc::new(shell_state.clone());

        let start_watcher = {
            let tools = tools.clone();
            let tab_diff_states = tab_diff_states.clone();

            move || {
                let watch_paths: Vec<PathBuf> = tools
                    .as_slice()
                    .iter()
                    .flat_map(|t| t.config_paths())
                    .filter(|p| p.exists())
                    .collect();

                if watch_paths.is_empty() {
                    return;
                }

                let (tx, rx) = mpsc::channel::<PathBuf>();
                std::thread::spawn(move || {
                    async_std::task::block_on(async move {
                        let watcher = match FileWatcher::watch(watch_paths).await {
                            Ok(w) => w,
                            Err(e) => {
                                eprintln!("dotcfg-gui: file watcher start failed: {e}");
                                return;
                            }
                        };

                        loop {
                            if let Some(path) = watcher.next_event().await {
                                if tx.send(path).is_err() {
                                    return;
                                }
                            }
                        }
                    });
                });

                let watcher_debounce_src: Rc<RefCell<Option<glib::SourceId>>> =
                    Rc::new(RefCell::new(None));
                let watcher_last_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
                let poll_ds = watcher_debounce_src.clone();
                let poll_lp = watcher_last_path.clone();
                let poll_st = tab_diff_states.clone();
                let poll_shell = watcher_shell.clone();
                let poll_saving = is_saving.clone();

                glib::timeout_add_local(Duration::from_millis(100), move || {
                    // Suppress watcher events while a save is in progress.
                    if poll_saving.load(std::sync::atomic::Ordering::Relaxed) {
                        return glib::ControlFlow::Continue;
                    }

                    while let Ok(path) = rx.try_recv() {
                        if let Some(id) = poll_ds.borrow_mut().take() {
                            id.remove();
                        }
                        *poll_lp.borrow_mut() = Some(path);

                        let lp = poll_lp.clone();
                        let st = poll_st.clone();
                        let inner_shell = poll_shell.clone();
                        let src = glib::timeout_add_local(Duration::from_millis(500), move || {
                            let path_opt = lp.borrow().clone();
                            if let Some(ref p) = path_opt {
                                handle_external_change(&st, p, &inner_shell);
                            }
                            glib::ControlFlow::Break
                        });
                        *poll_ds.borrow_mut() = Some(src);
                    }
                    glib::ControlFlow::Continue
                });
            }
        };

        // Start the watcher if any config exists on startup.
        start_watcher();

        // --- Wire the Generate Default Config button ---
        let gen_tools = tools.clone();
        let gen_tab_view = tab_view.clone();
        let gen_tab_diff = tab_diff_states.clone();
        let gen_stack = main_stack.clone();
        let gen_win = window.downgrade();
        let gen_start_watcher = start_watcher.clone();
        let gen_shell = shell_state.clone();
        gen_button.connect_clicked(move |_btn| {
            let Some(tool) = gen_tools.as_slice().first() else {
                eprintln!("dotcfg-gui: no tools registered for config generation");
                return;
            };
            match tool.generate_default_config() {
                Ok(path) => {
                    // Load the generated config into the tool.
                    let _ = tool.load(&path);

                    let initial_text = std::fs::read_to_string(&path).unwrap_or_default();

                    let mut states = gen_tab_diff.borrow_mut();
                    states.clear();

                    let (editor_widget, _banner, _text_view) = build_editor_page(
                        0,
                        gen_tools.clone(),
                        &mut states,
                        &initial_text,
                        gen_shell.clone(),
                    );

                    // Close any existing tab pages before adding the new one.
                    if gen_tab_view.n_pages() > 0 {
                        let page = gen_tab_view.nth_page(0);
                        gen_tab_view.close_page(&page);
                    }
                    let page = gen_tab_view.append(&editor_widget);
                    page.set_title(tool.display_name());

                    // Update window refs + config path.
                    if let Some(win) = gen_win.upgrade() {
                        for state in states.iter_mut() {
                            state.window = win.downgrade();
                            state.config_path = Some(path.clone());
                        }
                    }

                    // Select the first tab and switch to editor view.
                    let page = gen_tab_view.nth_page(0);
                    gen_tab_view.set_selected_page(&page);
                    gen_stack.set_visible_child_name("editor");

                    // Start the file watcher for the newly-created config.
                    gen_start_watcher();
                }
                Err(e) => {
                    eprintln!("dotcfg-gui: default config generation failed: {e}");
                }
            }
        });

        // Wire sidebar row activation → tab selection.
        let tab_view_clone = tab_view.clone();
        plugin_list.connect_row_activated(move |_list, row| {
            let idx = row.index();
            if idx >= 0 {
                let page = tab_view_clone.nth_page(idx);
                tab_view_clone.set_selected_page(&page);
            }
        });

        // Select the first tab by default.
        let page = tab_view.nth_page(0);
        tab_view.set_selected_page(&page);

        // --- Wire the global diff toggle ---
        let states = tab_diff_states.clone();
        diff_toggle.connect_toggled(move |btn| {
            let show_diff = btn.is_active();
            let states = states.borrow();
            for state in states.iter() {
                if show_diff {
                    refresh_tab_diff(state);
                }
                set_tab_mode(state, show_diff);
            }
        });

        // --- Set window refs + config paths on each tab state ---
        // (window was created after build_editor_page, so we fill them now)
        {
            let mut states = tab_diff_states.borrow_mut();
            for (state, tool) in states.iter_mut().zip(tools.as_slice().iter()) {
                state.window = window.downgrade();
                state.config_path = tool.config_paths().iter().find(|p| p.exists()).cloned();
            }
        }

        // --- ShellState: set initial tool ID ---
        if let Some(first_tool) = tools.as_slice().first() {
            shell_state.set_current_tool_id(Some(first_tool.id()));
        }

        // --- Wire dirty shutdown intercept (Wave 5 Step 17) ---
        // Intercept close-request when any tab has unsaved edits.
        // Also persists window state on clean-dismissal.
        {
            let cr_states = tab_diff_states.clone();
            let cr_tools = tools.clone();
            let cr_shell = shell_state.clone();
            window.connect_close_request(move |win| {
                if !cr_shell.is_dirty() {
                    // Save window state before proceeding.
                    let (w, h) = win.default_size();
                    let last_tool = cr_tools.as_slice().first().map(|t| t.id());
                    save_shell_window_state(w.max(1), h.max(1), last_tool);
                    return glib::Propagation::Proceed;
                }

                let dialog = adw::AlertDialog::new(
                    Some("Unsaved Changes"),
                    Some("You have unsaved changes. What would you like to do?"),
                );
                dialog.add_response("cancel", "_Cancel");
                dialog.add_response("discard", "_Discard");
                dialog.add_response("save", "_Save");
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");

                let st = cr_states.clone();
                let win_weak = win.downgrade();
                let save_tools = cr_tools.clone();
                dialog.connect_response(None, move |_dlg, response| {
                    if response == "save" {
                        // Write each dirty tab's content to its config path.
                        let states = st.borrow();
                        for state in states.iter() {
                            let current = state
                                .editor_buf
                                .text(
                                    &state.editor_buf.start_iter(),
                                    &state.editor_buf.end_iter(),
                                    false,
                                )
                                .to_string();
                            let original = state.original_text.borrow();
                            if current != *original {
                                if let Some(ref path) = state.config_path {
                                    let _ = std::fs::write(path, &current);
                                    *state.original_text.borrow_mut() = current;
                                }
                            }
                        }
                    }
                    if response != "cancel" {
                        if let Some(win) = win_weak.upgrade() {
                            let (w, h) = win.default_size();
                            let last_tool = save_tools.as_slice().first().map(|t| t.id());
                            save_shell_window_state(w.max(1), h.max(1), last_tool);
                            win.destroy();
                        }
                    }
                });

                dialog.present(Some(win));
                glib::Propagation::Stop
            });
        }

        window.present();
    });

    app.run();
    Ok(())
}
