//! GUI shell: GTK4 + libadwaita `Application` / `ApplicationWindow`. The
//! entire file is `#[cfg(feature = "gtk")]` so it contributes zero to a
//! no-default-features build (which is what the unit-test CI job runs).
//!
//! # Validation loop (Wave 2 Step 10)
//!
//! The shell connects to the `GtkTextBuffer::changed` signal and
//! implements a debounced async validation loop:
//!
//! 1. On each keystroke, cancel the previous debounce timer.
//! 2. Start a new timer with the first tool's `validator.debounce_hint()`
//!    (default 500 ms for `niri`).
//! 3. When the timer fires, spawn a `glib::MainContext::spawn_local`
//!    future that calls `validator.validate_kdl(&text).await`.
//! 4. Update an `Adw.Banner` at the top of the window: hidden when no
//!    issues exist, shown with a message otherwise.

#![cfg(feature = "gtk")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::core::error::Error;
use crate::DynTool;

/// Launch the dotcfg-gui GUI shell.
///
/// `plugins` is the registry of `Box<dyn ToolPlugin>` built by `main`.
/// The first plugin's `Validator` (if any) drives the edit-time
/// validation loop.
pub fn run_shell(plugins: Vec<DynTool>) -> Result<(), Error> {
    let app = adw::Application::new(Some("com.d3t0x.niricfg"), Default::default());
    let tools = Rc::new(plugins);

    app.connect_activate(move |app| {
        let window = adw::ApplicationWindow::new(app);
        window.set_default_size(900, 700);
        window.set_title(Some("dotcfg-gui — niri config editor"));

        // --- Validation result banner ---
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
        // Some padding so the text doesn't touch the scrolled-window edge.
        text_view.set_margin_start(8);
        text_view.set_margin_end(8);
        text_view.set_margin_top(4);
        text_view.set_margin_bottom(4);
        scrolled.set_child(Some(&text_view));

        // --- Layout: banner on top, editor below ---
        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        vbox.append(&banner);
        vbox.append(&scrolled);
        window.set_content(Some(&vbox));

        // --- Debounce state: optional SourceId for the pending timer ---
        let debounce_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

        // --- Connect the text buffer's changed signal to drive validation ---
        let buffer = text_view.buffer();

        let tools_clone = tools.clone();
        let banner_clone = banner.clone();
        let debounce_clone = debounce_source.clone();

        buffer.connect_changed(move |buf| {
            // Cancel the pending debounce timer (if any).
            if let Some(source_id) = debounce_clone.borrow_mut().take() {
                source_id.remove();
            }

            // Determine the debounce interval from the first tool's
            // validator, falling back to 250 ms if no validator is present.
            let debounce_ms = tools_clone
                .first()
                .and_then(|tool| tool.validator())
                .map(|v| v.debounce_hint())
                .unwrap_or(Duration::from_millis(250));

            // Snapshot the current buffer text for the validator.
            let start = buf.start_iter();
            let end = buf.end_iter();
            let text: String = buf
                .text(&start, &end, false)
                .unwrap_or_default()
                .to_string();

            let timer_tools = tools_clone.clone();
            let timer_banner = banner_clone.clone();

            // Start a new debounce timer. When it fires, spawn an async
            // future on the glib main context to run the validator and
            // update the banner.  The timer is single-shot (Break).
            let id = glib::timeout_add_local(debounce_ms, move || {
                let validate_tools = timer_tools.clone();
                let validate_banner = timer_banner.clone();
                let validate_text = text.clone();

                glib::MainContext::default().spawn_local(async move {
                    if let Some(tool) = validate_tools.first() {
                        if let Some(validator) = tool.validator() {
                            match validator.validate_kdl(&validate_text).await {
                                Ok(issues) => {
                                    if issues.is_empty() {
                                        validate_banner.set_revealed(false);
                                    } else {
                                        let count = issues.len();
                                        let title = if count == 1 {
                                            format!("{}: {}", validator.name(), issues[0].message,)
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
                                    validate_banner.set_title(&format!("Validation error: {e}",));
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

        window.present();
    });

    app.run();
    Ok(())
}
