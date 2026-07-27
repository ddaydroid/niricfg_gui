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
//!     │   ├── HeaderBar ("Plugins")
//!     │   └── GtkListBox (one row per plugin)
//!     └── Content: GtkBox (vertical)
//!         ├── Adw.TabBar
//!         └── Adw.TabView (one tab per plugin)
//!             └── each tab: GtkBox (vertical)
//!                 ├── Adw.Banner (validation results)
//!                 ├── GtkBox (toolbar with diff toggle)
//!                 └── GtkStack
//!                     ├── ["editor"] GtkScrolledWindow
//!                     │   └── GtkTextView (monospace, highlighted)
//!                     └── ["diff"]   GtkPaned (horizontal)
//!                         ├── original (read-only, highlighted)
//!                         └── modified (read-only, highlighted)
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
//! lines). A `ToggleButton` in the tab's toolbar switches the stack.
//! The diff is computed lazily each time the user activates the toggle,
//! comparing the current editor buffer against the snapshot taken on
//! first edit.

#![cfg(feature = "gtk")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::core::diff::line_diff;
use crate::core::error::Error;
use crate::core::kdl_highlighter;
use crate::DynTool;

/// Character displayed in the gutter for each diff status.
const GUTTER_SAME: char = ' ';
const GUTTER_ADDED: char = '+';
const GUTTER_REMOVED: char = '-';
const GUTTER_MODIFIED: char = '~';

/// Build the side-by-side diff widget.
///
/// Returns the paned, the left (original) buffer, and the right (modified)
/// buffer so callers can re-populate on demand.
fn build_diff_widget() -> (gtk4::Paned, gtk4::TextBuffer, gtk4::TextBuffer) {
    let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    paned.set_wide_handle(true);
    paned.set_position(500);

    // --- Left: original (read-only) ---
    let left_sw = gtk4::ScrolledWindow::new();
    left_sw.set_vexpand(true);
    left_sw.set_hexpand(true);

    let left_view = gtk4::TextView::new();
    left_view.set_monospace(true);
    left_view.set_editable(false);
    left_view.set_cursor_visible(false);
    left_view.set_wrap_mode(gtk4::WrapMode::None);
    left_view.set_margin_start(6);
    left_view.set_margin_end(6);
    left_view.set_margin_top(4);
    left_view.set_margin_bottom(4);
    left_sw.set_child(Some(&left_view));

    // --- Right: modified (read-only) ---
    let right_sw = gtk4::ScrolledWindow::new();
    right_sw.set_vexpand(true);
    right_sw.set_hexpand(true);

    let right_view = gtk4::TextView::new();
    right_view.set_monospace(true);
    right_view.set_editable(false);
    right_view.set_cursor_visible(false);
    right_view.set_wrap_mode(gtk4::WrapMode::None);
    right_view.set_margin_start(6);
    right_view.set_margin_end(6);
    right_view.set_margin_top(4);
    right_view.set_margin_bottom(4);
    right_sw.set_child(Some(&right_view));

    // --- Colour tags ---
    fn make_diff_tag(buf: &gtk4::TextBuffer, name: &str, bg: &str, fg: &str) -> gtk4::TextTag {
        let tag = buf.create_tag(Some(name));
        tag.set_background(Some(bg));
        tag.set_foreground(Some(fg));
        tag
    }

    let left_buf = left_view.buffer();
    let right_buf = right_view.buffer();

    make_diff_tag(&right_buf, "diff_added", "#1b4a1b", "#a3be8c");
    make_diff_tag(&left_buf, "diff_removed", "#4a1b1b", "#bf616a");
    make_diff_tag(&left_buf, "diff_mod_left", "#3d3520", "#d08770");
    make_diff_tag(&right_buf, "diff_mod_right", "#3d3520", "#d08770");

    // --- Sync vertical scrolling ---
    let left_vadj = left_sw.vadjustment();
    let right_vadj = right_sw.vadjustment();
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

    paned.set_start_child(Some(&left_sw));
    paned.set_end_child(Some(&right_sw));
    (paned, left_buf, right_buf)
}

/// Populate the two diff buffers from the original and modified text.
fn populate_diff_view(
    left_buf: &gtk4::TextBuffer,
    right_buf: &gtk4::TextBuffer,
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

    // Apply colour tags.
    let left_tt = left_buf.tag_table();
    let right_tt = right_buf.tag_table();

    for (i, state) in left_states.iter().enumerate() {
        // Each line contributes: gutter(1) + space(1) + content + newline(1).
        // sum(l.len() + 3) over preceding lines → start of current line.
        // +2 skips the gutter and space to reach the content body.
        let line_start = left_lines[..i].iter().map(|l| l.len() + 3).sum::<usize>() + 2;
        let line_end = line_start + left_lines[i].len();

        match *state {
            "removed" => {
                if let Some(tag) = left_tt.lookup("diff_removed") {
                    let s = left_buf.iter_at_offset(line_start as i32);
                    let e = left_buf.iter_at_offset(line_end as i32);
                    left_buf.apply_tag(&tag, &s, &e);
                }
            }
            "modified" => {
                if let Some(tag) = left_tt.lookup("diff_mod_left") {
                    let s = left_buf.iter_at_offset(line_start as i32);
                    let e = left_buf.iter_at_offset(line_end as i32);
                    left_buf.apply_tag(&tag, &s, &e);
                }
            }
            _ => {}
        }
    }

    for (i, state) in right_states.iter().enumerate() {
        // sum(l.len() + 3) over preceding lines → start of current line.
        let line_start = right_lines[..i].iter().map(|l| l.len() + 3).sum::<usize>() + 2;
        let line_end = line_start + right_lines[i].len();

        match *state {
            "added" => {
                if let Some(tag) = right_tt.lookup("diff_added") {
                    let s = right_buf.iter_at_offset(line_start as i32);
                    let e = right_buf.iter_at_offset(line_end as i32);
                    right_buf.apply_tag(&tag, &s, &e);
                }
            }
            "modified" => {
                if let Some(tag) = right_tt.lookup("diff_mod_right") {
                    let s = right_buf.iter_at_offset(line_start as i32);
                    let e = right_buf.iter_at_offset(line_end as i32);
                    right_buf.apply_tag(&tag, &s, &e);
                }
            }
            _ => {}
        }
    }
}

/// Build one editor tab page: banner + toolbar (diff toggle) + stack
/// (editor ↔ diff view), with KDL highlighting and debounced async
/// validation loop.
///
/// Returns the container widget, the banner, and the text view (so
/// callers can load content or attach external state).
fn build_editor_page(
    tool_index: usize,
    tools: Rc<Vec<DynTool>>,
) -> (gtk4::Box, adw::Banner, gtk4::TextView) {
    // --- Banner (validation results) ---
    let banner = adw::Banner::new();
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

    // Apply KDL syntax highlighting.
    kdl_highlighter::apply_highlighting(&text_view.buffer());

    // --- Original text snapshot (used for diff) ---
    let original_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // --- Diff view (lazily built; store buffers for later refresh) ---
    let (diff_paned, left_buf, right_buf) = build_diff_widget();
    let diff_bufs: Rc<RefCell<(gtk4::TextBuffer, gtk4::TextBuffer)>> =
        Rc::new(RefCell::new((left_buf, right_buf)));

    // --- Stack: editor ↔ diff ---
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    // Page 0: editor.
    stack.add_titled(&scrolled, Some("editor"), "Editor");
    // Page 1: diff view.
    stack.add_titled(&diff_paned, Some("diff"), "Diff");

    // --- Toolbar row (diff toggle) ---
    let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    toolbar.set_margin_top(4);
    toolbar.set_margin_bottom(4);
    toolbar.set_margin_start(8);
    toolbar.set_margin_end(8);
    toolbar.set_spacing(8);

    let diff_toggle = gtk4::ToggleButton::with_label("Compare");
    diff_toggle.set_tooltip_text(Some("Show side-by-side diff against saved version"));

    // Spacer to push toggle to the right.
    let spacer = gtk4::Label::new(None);
    toolbar.set_halign(gtk4::Align::Fill);
    toolbar.append(&spacer);
    toolbar.append(&diff_toggle);

    // Wire the diff toggle: on activation, refresh the diff and switch pages.
    let stack_clone = stack.clone();
    let tv_buf = text_view.buffer();
    let tv_orig = original_text.clone();
    let diff_bufs_c = diff_bufs.clone();

    diff_toggle.connect_toggled(move |btn| {
        if btn.is_active() {
            // Snapshot modified text and refresh diff.
            let modified = tv_buf
                .text(&tv_buf.start_iter(), &tv_buf.end_iter(), false)
                .unwrap_or_default()
                .to_string();
            let original = tv_orig.borrow().clone();

            let (ref left, ref right) = *diff_bufs_c.borrow();
            populate_diff_view(left, right, &original, &modified);

            stack_clone.set_visible_child_name("diff");
        } else {
            stack_clone.set_visible_child_name("editor");
        }
    });

    // Connect buffer changed: on first edit, snapshot original text.
    let orig_set: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let orig_set_c = orig_set.clone();
    let tv_orig2 = original_text.clone();
    text_view.buffer().connect_changed(move |buf| {
        if !*orig_set_c.borrow() {
            let current = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .unwrap_or_default()
                .to_string();
            *tv_orig2.borrow_mut() = current;
            *orig_set_c.borrow_mut() = true;
        }
    });

    // --- Layout: banner + toolbar + stack ---
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.set_vexpand(true);
    vbox.append(&banner);
    vbox.append(&toolbar);
    vbox.append(&stack);

    // --- Debounce state (validation loop) ---
    let debounce_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let tools_clone = tools;
    let banner_clone = banner.clone();
    let debounce_clone = debounce_source.clone();
    let buffer = text_view.buffer();

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
        let text: String = buf
            .text(&start, &end, false)
            .unwrap_or_default()
            .to_string();

        let timer_tools = tools_clone.clone();
        let timer_banner = banner_clone.clone();

        let id = glib::timeout_add_local(debounce_ms, move || {
            let validate_tools = timer_tools.clone();
            let validate_banner = timer_banner.clone();
            let validate_text = text.clone();

            glib::MainContext::default().spawn_local(async move {
                if let Some(tool) = validate_tools.get(tool_index) {
                    if let Some(validator) = tool.validator() {
                        match validator.validate_kdl(&validate_text).await {
                            Ok(issues) => {
                                if issues.is_empty() {
                                    validate_banner.set_revealed(false);
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
                                }
                            }
                            Err(e) => {
                                validate_banner.set_title(&format!("Validation error: {e}"));
                                validate_banner.set_revealed(true);
                            }
                        }
                    }
                }
            });

            glib::ControlFlow::Break
        });

        *debounce_clone.borrow_mut() = Some(id);
    });

    (vbox, banner, text_view)
}

/// Launch the dotcfg-gui GUI shell.
///
/// `plugins` is the list of `Box<dyn ToolPlugin>` built by `main`. The
/// shell wraps them into a sidebar (plugin list) + tab view (per-plugin
/// editor), each tab carrying its own debounced async validator loop,
/// KDL syntax highlighting, and a side-by-side diff view.
///
/// A `ToolRegistry` is not used here because `Box<dyn ToolPlugin>` does
/// not implement `Clone` (dyn traits don't carry Clone), so the plugins
/// are stored in an `Rc<Vec<DynTool>>` and iterated by reference — the
/// Vec itself serves the registry role for the shell's lifetime.
pub fn run_shell(plugins: Vec<DynTool>) -> Result<(), Error> {
    let app = adw::Application::new(Some("com.d3t0x.niricfg"), Default::default());
    let tools = Rc::new(plugins);

    app.connect_activate(move |app| {
        let window = adw::ApplicationWindow::new(app);
        window.set_default_size(1000, 700);
        window.set_title(Some("dotcfg-gui — niri config editor"));

        // --- Navigation split view: sidebar | content ---
        let split_view = adw::NavigationSplitView::new();
        split_view.set_sidebar_width_fraction(0.25);
        split_view.set_min_sidebar_width(180);
        split_view.set_max_sidebar_width(350);

        // --- Sidebar: plugin list in a ToolbarView ---
        let sidebar = adw::ToolbarView::new();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&gtk4::Label::new(Some("Plugins"))));
        sidebar.add_top_bar(&sidebar_header);

        let plugin_list = gtk4::ListBox::new();
        plugin_list.set_selection_mode(gtk4::SelectionMode::Single);
        sidebar.set_content(Some(&plugin_list));

        split_view.set_sidebar(Some(&sidebar));

        // --- Content: tab bar + tab view ---
        let tab_bar = adw::TabBar::new();
        let tab_view = adw::TabView::new();
        tab_bar.set_view(Some(&tab_view));

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.set_vexpand(true);
        content_box.append(&tab_bar);
        content_box.append(&tab_view);

        split_view.set_content(Some(&content_box));
        window.set_content(Some(&split_view));

        // --- Populate sidebar rows + editor tabs ---
        for (i, tool) in tools.as_slice().iter().enumerate() {
            // Sidebar row.
            let row = gtk4::ListBoxRow::new();
            let label = gtk4::Label::new(Some(tool.display_name()));
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(6);
            label.set_margin_bottom(6);
            label.set_halign(gtk4::Align::Start);
            row.set_child(Some(&label));
            plugin_list.append(&row);

            // Editor tab.
            let (editor_widget, _banner, _text_view) = build_editor_page(i, tools.clone());
            tab_view.append(&editor_widget, tool.display_name());
        }

        // Wire sidebar row activation -> tab selection.
        let tab_view_clone = tab_view.clone();
        plugin_list.connect_row_activated(move |_list, row| {
            let idx = row.index();
            if idx >= 0 {
                if let Some(page) = tab_view_clone.nth_page(idx as u32) {
                    tab_view_clone.set_selected_page(&page);
                }
            }
        });

        // Select the first tab by default.
        if let Some(page) = tab_view.nth_page(0) {
            tab_view.set_selected_page(&page);
        }

        window.present();
    });

    app.run();
    Ok(())
}
