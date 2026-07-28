//! GObject subclass that tracks the shell's mutable application state:
//! dirty flag, active tool ID, and last validation outcome.
//!
//! Any widget that holds a reference to `ShellState` can:
//! - Query / set the dirty flag (→ binds Save button sensitivity).
//! - Watch the `"dirty-changed"` signal to toggle save sensitivity.
//! - Watch the `"tool-changed"` signal to refresh section widgets.
//! - Read the last validation result for banner display.
//!
//! # Type hierarchy
//!
//! ```text
//! glib::Object
//!  └── ShellState  (our subclass)
//!      ├── property "is-dirty"        (bool, read-write)
//!      ├── property "current-tool-id" (string, read-write)
//!      ├── property "last-validation" (string, read-write)
//!      ├── signal "dirty-changed"     ()
//!      └── signal "tool-changed"     ()
//! ```
//!
//! # Thread safety
//!
//! GObject methods take `&self` — all interior state uses `Cell` / `RefCell`.
//! `ShellState` itself is `!Send` / `!Sync` like all GTK objects, so it must
//! live on the GTK main thread (which is the only thread that touches it).

use std::cell::{Cell, RefCell};

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::ParamSpec;
use once_cell::sync::Lazy;

// ---------------------------------------------------------------------------
// Wrapper type (public API)
// ---------------------------------------------------------------------------

glib::wrapper! {
    /// Application state shared across the shell's widget tree.
    ///
    /// Construct via `ShellState::default()` (it implements `Default`).
    pub struct ShellState(ObjectSubclass<imp::ShellState>);

    // Match the parent type chain that glib::Object uses in gtk4-rs 0.9.x.
    // `ObjectSubclass` requires the chain: ObjectSubclass → ObjectImpl
    // which is satisfied by the impl block below.
}

impl Default for ShellState {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ShellState {
    /// Whether the editor has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.imp().is_dirty.get()
    }

    /// Mark the editor dirty or clean.
    ///
    /// Emits `"dirty-changed"` only when the value actually flips.
    pub fn set_dirty(&self, dirty: bool) {
        let imp = self.imp();
        if imp.is_dirty.replace(dirty) != dirty {
            self.emit_by_name::<()>("dirty-changed", &[]);
        }
    }

    /// The ID of the currently-active tool plugin (e.g. `"niri"`).
    pub fn current_tool_id(&self) -> Option<String> {
        self.imp().current_tool_id.borrow().clone()
    }

    /// Set the active tool ID.
    ///
    /// Emits `"tool-changed"` only when the value actually changes.
    pub fn set_current_tool_id(&self, id: Option<&str>) {
        let imp = self.imp();
        let old = imp.current_tool_id.replace(id.map(String::from));
        if old.as_deref() != id {
            self.emit_by_name::<()>("tool-changed", &[]);
        }
    }

    /// The last validation result string (e.g. `"3 issues found"` or empty).
    pub fn last_validation(&self) -> Option<String> {
        self.imp().last_validation.borrow().clone()
    }

    /// Store the latest validation outcome.
    pub fn set_last_validation(&self, result: Option<&str>) {
        self.imp().last_validation.replace(result.map(String::from));
    }
}

// ---------------------------------------------------------------------------
// Implementation struct (private)
// ---------------------------------------------------------------------------

mod imp {
    use super::*;

    /// Interior state for the ShellState GObject subclass.
    ///
    /// All fields use interior-mutability cells so that `ObjectImpl`'s
    /// `&self` methods can read / write them.
    #[derive(Default)]
    pub struct ShellState {
        /// `true` when any tab has unsaved edits.
        pub is_dirty: Cell<bool>,
        /// The plugin ID of the active tool (e.g. `Some("niri")`).
        pub current_tool_id: RefCell<Option<String>>,
        /// Human-readable summary of the last validation run.
        pub last_validation: RefCell<Option<String>>,
    }

    /// SAFETY: ShellState has no finalize / dispose logic beyond what
    /// glib::Object provides — all fields are Rust-managed.
    #[glib::object_subclass]
    impl ObjectSubclass for ShellState {
        const NAME: &'static str = "DotcfgShellState";
        type Type = super::ShellState;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for ShellState {
        /// Define the GObject properties exposed on this type.
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecBoolean::builder("is-dirty")
                        .nick("Dirty Flag")
                        .blurb("Whether the editor has unsaved changes")
                        .default_value(false)
                        .read_only() // mutated only via methods, not from CSS/bindings
                        .build(),
                    glib::ParamSpecString::builder("current-tool-id")
                        .nick("Current Tool ID")
                        .blurb("The ID of the active tool plugin")
                        .default_value(None::<&str>)
                        .build(),
                    glib::ParamSpecString::builder("last-validation")
                        .nick("Last Validation")
                        .blurb("Summary of the most recent validation result")
                        .default_value(None::<&str>)
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &ParamSpec) {
            match pspec.name() {
                "is-dirty" => {
                    self.is_dirty.set(value.get().unwrap_or(false));
                }
                "current-tool-id" => {
                    self.current_tool_id.replace(value.get().ok().flatten());
                }
                "last-validation" => {
                    self.last_validation.replace(value.get().ok().flatten());
                }
                _ => unimplemented!("unknown property: {}", pspec.name()),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> glib::Value {
            match pspec.name() {
                "is-dirty" => self.is_dirty.get().to_value(),
                "current-tool-id" => self.current_tool_id.borrow().to_value(),
                "last-validation" => self.last_validation.borrow().to_value(),
                _ => unimplemented!("unknown property: {}", pspec.name()),
            }
        }

        /// Define custom signals emitted by this type.
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: Lazy<Vec<glib::subclass::Signal>> = Lazy::new(|| {
                vec![
                    glib::subclass::Signal::builder("dirty-changed").build(),
                    glib::subclass::Signal::builder("tool-changed").build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }
}
