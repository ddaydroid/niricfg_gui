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
//!                 └── GtkScrolledWindow
//!                     └── GtkTextView (monospace editor)
//! ```
//!
//! # Validation loop (Wave 2 Step 10)
//!
//! Each tab's `GtkTextBuffer::changed` signal drives a debounced async
//! validation loop that calls `tool.validator().validate_kdl(&text).await`
//! and updates the tab's `Adw.Banner`.

#![cfg(feature = "gtk")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::core::error::Error;
use crate::DynTool;

/// Build an editor tab page (banner + text view) and wire its validation
/// loop. Returns the container widget, the banner (for external status
/// updates), and the text view (for content loading).
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

    // --- Layout: banner on top, scrolled editor below ---
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.set_vexpand(true);
    vbox.append(&banner);
    vbox.append(&scrolled);

    // --- Debounce state: optional SourceId for the pending timer ---
    let debounce_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    // --- Connect the text buffer's changed signal to drive validation ---
    let buffer = text_view.buffer();
    let tools_clone = tools;
    let banner_clone = banner.clone();
    let debounce_clone = debounce_source.clone();

    buffer.connect_changed(move |buf| {
        // Cancel the pending debounce timer (if any).
        if let Some(source_id) = debounce_clone.borrow_mut().take() {
            source_id.remove();
        }

        // Determine the debounce interval from this tool's validator.
        let debounce_ms = tools_clone
            .get(tool_index)
            .and_then(|tool| tool.validator())
            .map(|v| v.debounce_hint())
            .unwrap_or(Duration::from_millis(250));

        // Snapshot the current buffer text.
        let start = buf.start_iter();
        let end = buf.end_iter();
        let text: String = buf
            .text(&start, &end, false)
            .unwrap_or_default()
            .to_string();

        let timer_tools = tools_clone.clone();
        let timer_banner = banner_clone.clone();

        // Start a new debounce timer (single-shot via Break).
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
}/// Launch the dotcfg-gui GUI shell.
///
/// `plugins` is the list of `Box<dyn ToolPlugin>` built by `main`. The
/// shell wraps them into a sidebar (plugin list) + tab view (per-plugin
/// editor), each tab carrying its own debounced async validator loop.
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
            // Sidebar row
            let row = gtk4::ListBoxRow::new();
            let label = gtk4::Label::new(Some(tool.display_name()));
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(6);
            label.set_margin_bottom(6);
            label.set_halign(gtk4::Align::Start);
            row.set_child(Some(&label));
            plugin_list.append(&row);

            // Editor tab
            let (editor_widget, _banner, _text_view) =
                build_editor_page(i, tools.clone());
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
