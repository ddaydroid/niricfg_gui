//! Plugin-trait hierarchy. `ToolPlugin` carries no GTK types so any dyn
//! value is usable from headless code (tests, CLI tools, validators). The
//! GUI-only `create_shell_page` lives on a separate `ToolPluginUi` trait,
//! gated behind `#[cfg(feature = "gtk")]`.
//!
//! The KDL-flavored `KdlBackedTool` sub-trait provides the canonical
//! load-via-`load_config` / save-via-`save_config` round-trip with
//! overridable default-methods. Plugins like `NiriTool` (Wave 2) opt in
//! by `impl KdlBackedTool for NiriTool {}` and may override either
//! method (e.g. NiriTool Step 8 will override `load_kdl` to also build
//! the Semantic-Path index after parsing). Format-agnostic plugins
//! (TOML/YAML/INI for sway/hyprland/waybar etc.) implement just
//! `ToolPlugin` and never inherit the KDL-specific helpers — keeping
//! the parent trait free of format-coupled surface area.

use std::any::Any;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use crate::core::config_loader::{load_config, ConfigDoc};
use crate::core::config_writer::save_config;
use crate::core::error::{Error, ExternalChangeAction, ValidationIssue};
use crate::core::validator::Validator;

/// The platform-agnostic plugin contract.
pub trait ToolPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;

    /// Candidate config paths this plugin owns. Used by the shell's
    /// `discover_by_path` to decide which plugin handles a file the user
    /// drops on the sidebar.
    fn config_paths(&self) -> Vec<PathBuf>;

    /// Cheap path-matcher: does this plugin claim ownership of this path?
    fn detect(&self, path: &Path) -> bool;

    fn load(&self, path: &Path) -> Result<(), Error>;

    /// Atomic-temp-file + rename; writes to the path passed to the most recent
    /// successful `load`. Plugin's responsibility.
    fn save(&self) -> Result<(), Error>;

    fn validate(&self) -> Result<Vec<ValidationIssue>, Error>;

    /// After `save` succeeds, ask the live compositor / tool to refresh.
    /// Pure-file plugins return `Ok(())`.
    fn apply_saved(&self) -> Result<(), Error>;

    /// Called by the inotify watcher when an external change is detected on
    /// a config path this plugin owns.
    fn on_external_change(&self) -> ExternalChangeAction;

    /// Plugin's API version. The registry's `is_plugin_compatible` uses this
    /// for forward-compatibility gating. Default impl returns 1 so existing
    /// `ToolPlugin` impls continue compiling unchanged after this method is
    /// added; explicit impls should bump on breaking-trait changes.
    fn api_version(&self) -> u32 {
        1
    }

    /// Access the async [`Validator`](Validator) for edit-time validation.
    ///
    /// The shell's debounce loop calls this after each edit (with the
    /// validator's `debounce_hint` delay). Returns `None` for plugins
    /// that don't perform async validation (pure-file plugins for
    /// TOML/YAML/INI formats, headless tests, etc.).
    fn validator(&self) -> Option<&dyn Validator> {
        None
    }

    /// Generate a default config file at the first config path and return
    /// the written path.
    ///
    /// The default impl returns `Error::Plugin(…)` — plugins that support
    /// auto-generated baseline configs (like the first-run UX) override
    /// this to write a sensible default and return `Ok(path)`.
    fn generate_default_config(&self) -> Result<PathBuf, Error> {
        Err(Error::Plugin(format!(
            "{}: auto-generation not supported",
            self.display_name()
        )))
    }

    /// Allow downcasting to concrete types for shell-level extension.
    /// Implement as `fn as_any(&self) -> &dyn Any { self }`.
    fn as_any(&self) -> &dyn Any;
}

/// Convenience alias for the registry's element type.
pub type DynTool = Box<dyn ToolPlugin>;

/// GTK-specific extension: build the libadwaita widget tree for this plugin's
/// main pane. The shell passes the shared text buffer so section widgets can
/// write their changes into the same buffer that drives the validation loop
/// and diff view.
///
/// Only available with `--features gtk`.
#[cfg(feature = "gtk")]
pub trait ToolPluginUi: ToolPlugin {
    fn create_shell_page(&self, text_buffer: &gtk4::TextBuffer) -> gtk4::Widget;
}

/// Sub-trait for tools whose state IS a KDL [`ConfigDoc`]. Mirrors the
/// `ToolPluginUi` pattern: a focused extension on top of the
/// format-agnostic `ToolPlugin`. Concrete plugins opt in with:
///
/// ```ignore
/// impl KdlBackedTool for NiriTool {}
/// ```
///
/// and inherit the canonical load-via-`load_config` +
/// save-via-`save_config` round-trip. Default-methods may be overridden:
///
/// - **Wave 2's `NiriTool`** overrides `load_kdl` to also build the
///   Semantic-Path index (Step 8) after parsing.
/// - **Wave 2's `apply_saved`** (in `ToolPlugin`) calls `save_kdl` after
///   the user hits Ctrl+S.
///
/// Like `ToolPluginUi`, this trait adds KDL-specific surface on top of a
/// format-agnostic parent so future TOML/YAML/INI plugins can implement
/// `ToolPlugin` without inheriting KDL-specific helpers.
///
/// Errors propagate `Error::Io(_)` for filesystem failures
/// (`fs::read_to_string`, `save_config` parent missing / read-only /
/// rename) and `Error::Kdl(_)` for syntactic parse failures inside the
/// default `load_kdl` body. Overrides should preserve the same dual
/// mapping so `dyn KdlBackedTool` callers get a consistent error
/// vocabulary.
pub trait KdlBackedTool: ToolPlugin {
    /// Read `path` as UTF-8 text and parse via [`load_config`].
    ///
    /// Default-method — concrete tools may override to attach
    /// domain-specific post-parse setup (Semantic-Path indexing, drift
    /// detection, plugin-specific augmentation). Overrides should
    /// preserve the dual error mapping (`Error::Io` for I/O,
    /// `Error::Kdl` for parse).
    fn load_kdl(&self, path: &Path) -> Result<ConfigDoc, Error> {
        let text = read_to_string(path)?;
        load_config(&text)
    }

    /// Serialize `doc` to `target` atomically via [`save_config`]
    /// (tempfile-in-target-dir + sync_all + persist rename).
    ///
    /// Default-method — concrete tools may override for custom
    /// pre-serialization transformations (header injection, section
    /// reordering, plugin-specific normalization). Overrides should
    /// preserve the atomic-write property so a crashed mid-save leaves
    /// either the old or new file on disk, never a hybrid.
    fn save_kdl(&self, doc: &ConfigDoc, target: &Path) -> Result<(), Error> {
        save_config(doc, target)
    }
}
