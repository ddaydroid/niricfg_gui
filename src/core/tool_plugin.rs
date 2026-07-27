//! Plugin-trait hierarchy. `ToolPlugin` carries no GTK types so any dyn
//! value is usable from headless code (tests, CLI tools, validators). The
//! GUI-only `create_shell_page` lives on a separate `ToolPluginUi` trait,
//! gated behind `#[cfg(feature = "gtk")]`.

use std::path::{Path, PathBuf};

use crate::core::error::{Error, ExternalChangeAction, ValidationIssue};

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
}

/// Convenience alias for the registry's element type.
pub type DynTool = Box<dyn ToolPlugin>;

/// GTK-specific extension: build the libadwaita widget tree for this plugin's
/// main pane. Only available with `--features gtk`.
#[cfg(feature = "gtk")]
pub trait ToolPluginUi: ToolPlugin {
    fn create_shell_page(&self) -> gtk4::Widget;
}
