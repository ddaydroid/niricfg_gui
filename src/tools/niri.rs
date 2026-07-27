//! Niri compositor tool plugin. Owns `~/.config/niri/config.kdl`, loads it
//! as KDL, and pre-computes the semantic-path index at load time for O(1)
//! edit-time lookups.
//!
//! # Architecture
//!
//! `NiriTool` stores a resolved `config_dir` (`~/.config/niri`) and an
//! interior-mutable `Mutex<NiriToolState>` holding the active path, parsed
//! KDL document, and the precomputed semantic-path index.
//!
//! Mutex interior is necessary because `ToolPlugin` takes `&self` on all
//! methods (the trait is `Send + Sync` bound), so state mutation requires
//! interior mutability. A single-threaded `RefCell` would suffice for the
//! GTK-thread use case, but the trait contract is `Sync`, so `Mutex` is
//! chosen to keep the implementation honest.
//!
//! # Wave 2 Step 7 (spec path)
//!
//! This module implements spec Step 7 (NiriTool skeleton) with Step 8's
//! Semantic-Path indexing integrated directly into `load_kdl` — because
//! building the index at load time (rather than deferring to a separate
//! Step-8-only commit) means every `load` call atomically produces both
//! the parsed doc AND the index, preventing a window where the shell has
//! a `ConfigDoc` but no index.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::config_loader::{load_config, ConfigDoc};
use crate::core::error::{Error, ExternalChangeAction, ValidationIssue};
use crate::core::semantic_path::{build_index, SemanticIndex};
use crate::core::tool_plugin::{KdlBackedTool, ToolPlugin};
use crate::core::validator::Validator;
use crate::NiriValidator;

/// Display name shown in the sidebar / tab bar.
const NIRI_DISPLAY_NAME: &str = "Niri";

/// Stable plugin identifier used in the `ToolRegistry` and for persistence
/// of sidebar state across restarts.
const NIRI_ID: &str = "niri";

/// Niri compositor tool plugin.
///
/// # Fields
///
/// * `config_dir` — Resolved to `~/.config/niri` (or `$XDG_CONFIG_HOME/niri`)
///   at construction. Used by [`config_paths`](ToolPlugin::config_paths) and
///   [`detect`](ToolPlugin::detect) to locate the niri config file.
/// * `state` — Interior-mutable state protected by `Mutex`. Holds the
///   last-loaded `active_path`, the parsed `ConfigDoc`, and the
///   precomputed `SemanticIndex` (Wave 2 Step 8).
/// * `validator` — Plugin-specific async validator. Defaults to
///   [`NiriValidator`] which spawns `niri msg validate` via
///   `async-process`. The shell's async loop calls this after each
///   edit (with the configured debounce).
pub struct NiriTool {
    config_dir: PathBuf,
    state: Mutex<NiriToolState>,
    validator: Box<dyn Validator>,
}

/// Interior state for [`NiriTool`], guarded by `Mutex`.
struct NiriToolState {
    /// The file path most recently passed to [`load`](ToolPlugin::load).
    active_path: Option<PathBuf>,
    /// Parsed KDL document, populated after a successful `load_kdl`.
    doc: Option<ConfigDoc>,
    /// Precomputed path--span index, rebuilt on every `load_kdl` call.
    index: Option<SemanticIndex>,
}

impl NiriTool {
    /// Resolve the niri config directory from `$XDG_CONFIG_HOME` or `$HOME`.
    ///
    /// Fallback to `/tmp` when neither variable is set (unusual but
    /// prevents a panic in headless test / CI environments).
    fn default_config_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("niri")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config").join("niri")
        }
    }

    /// Construct a new `NiriTool` with default config directory resolution,
    /// an empty initial state, and a [`NiriValidator`] wired as the
    /// async validator.
    pub fn new() -> Self {
        Self {
            config_dir: Self::default_config_dir(),
            state: Mutex::new(NiriToolState {
                active_path: None,
                doc: None,
                index: None,
            }),
            validator: Box::new(NiriValidator::new()),
        }
    }

    /// Construct a `NiriTool` with a custom validator (for tests that
    /// need a [`CannedValidator`] or other non-default impl).
    pub fn with_validator(validator: Box<dyn Validator>) -> Self {
        Self {
            config_dir: Self::default_config_dir(),
            state: Mutex::new(NiriToolState {
                active_path: None,
                doc: None,
                index: None,
            }),
            validator,
        }
    }

    /// Access the precomputed `SemanticIndex`, if loaded.
    ///
    /// Returns `None` if no file has been loaded yet (freshly constructed
    /// tool, or a `load` call returned `Err`).
    pub fn index(&self) -> Option<SemanticIndex> {
        self.state.lock().unwrap().index.clone()
    }

    /// Access the currently-loaded `ConfigDoc`, if any.
    pub fn doc(&self) -> Option<ConfigDoc> {
        self.state.lock().unwrap().doc.clone()
    }

    /// Access the async validator used for KDL validation.
    ///
    /// The shell's async edit-loop uses this directly:
    /// `tool.validator().validate_kdl(&text).await`.
    pub fn validator(&self) -> &dyn Validator {
        &*self.validator
    }

    /// Write a minimal but functional niri config to the default path
    /// (`~/.config/niri/config.kdl`) and return the path of the written
    /// file.
    ///
    /// Creates the `config_dir` parent directory if it does not exist.
    /// Errors are `Error::Io(_)` — permission denied, read-only
    /// filesystem, disk full, etc.
    pub fn generate_default_config(&self) -> Result<PathBuf, Error> {
        let path = self.config_dir.join("config.kdl");
        let content = r##"input {
    keyboard {
        repeat-delay 250
        repeat-rate 33
    }
    touchpad {
        tap-to-click true
        natural-scroll true
    }
}

layout {
    gap 8
    focus-ring {
        width 2
        active-color "#5294e2"
    }
}

spawn-at-startup "/usr/libexec/polkit-gnome-authentication-agent-1"

binds {
    Mod+Return spawn "foot"
    Mod+Q close-window
    Mod+T toggle-floating
    Mod+Shift+F switch-preset-workspace-forward
    Mod+Shift+B switch-preset-workspace-backward
    Mod+1 focus-workspace 1
    Mod+2 focus-workspace 2
    Mod+3 focus-workspace 3
    Mod+4 focus-workspace 4
    Mod+Shift+1 move-column-to-workspace 1
    Mod+Shift+2 move-column-to-workspace 2
    Mod+Shift+3 move-column-to-workspace 3
    Mod+Shift+4 move-column-to-workspace 4
    Mod+Shift+Left move-column-left
    Mod+Shift+Right move-column-right
}
"##;
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::write(&path, content)?;
        Ok(path)
    }
}

impl Default for NiriTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolPlugin for NiriTool {
    fn id(&self) -> &'static str {
        NIRI_ID
    }

    fn display_name(&self) -> &'static str {
        NIRI_DISPLAY_NAME
    }

    fn config_paths(&self) -> Vec<PathBuf> {
        vec![self.config_dir.join("config.kdl")]
    }

    fn detect(&self, path: &Path) -> bool {
        self.config_dir.join("config.kdl") == path
    }

    fn load(&self, path: &Path) -> Result<(), Error> {
        // Delegate to load_kdl which handles both parsing AND index
        // construction. The returned ConfigDoc is already stored in
        // `state.doc` by the override below — discard it.
        self.load_kdl(path)?;
        Ok(())
    }

    fn save(&self) -> Result<(), Error> {
        let state = self.state.lock().unwrap();
        let doc = state
            .doc
            .as_ref()
            .ok_or_else(|| Error::Plugin("NiriTool: nothing loaded to save".to_string()))?;
        let path = state
            .active_path
            .as_ref()
            .ok_or_else(|| Error::Plugin("NiriTool: no active path to save to".to_string()))?;
        // Delegate to the KdlBackedTool default-method (atomic write via
        // save_config / tempfile + rename).
        self.save_kdl(doc, path)
    }

    fn validate(&self) -> Result<Vec<ValidationIssue>, Error> {
        let state = self.state.lock().unwrap();
        let doc = state
            .doc
            .as_ref()
            .ok_or_else(|| Error::Plugin("NiriTool: nothing loaded to validate".to_string()))?;
        let text = doc.to_string();
        drop(state); // release mutex before the async block runs
        async_std::task::block_on(self.validator.validate_kdl(&text))
    }

    fn apply_saved(&self) -> Result<(), Error> {
        // After a successful save, run the validator against the saved
        // document so the shell can surface any post-save issues.
        let state = self.state.lock().unwrap();
        let doc = state
            .doc
            .as_ref()
            .ok_or_else(|| Error::Plugin("NiriTool: nothing saved to apply".to_string()))?;
        let text = doc.to_string();
        drop(state);
        let issues = async_std::task::block_on(self.validator.validate_kdl(&text))?;
        if issues.is_empty() {
            Ok(())
        } else {
            // Surface the first error (or the first issue) so the shell
            // can display a post-save banner. A full issue list is
            // available through the validator accessor.
            Err(Error::Plugin(format!(
                "{} issue(s) after save; first: {}",
                issues.len(),
                issues[0].message
            )))
        }
    }

    fn on_external_change(&self) -> ExternalChangeAction {
        // Niri's config is reloaded at runtime by the compositor — the file
        // may change from outside the GUI. Default to Reload (the editor
        // silently re-reads the file) unless the user has unsaved edits
        // (that conflict is surfaced at the shell level).
        ExternalChangeAction::Reload
    }

    fn validator(&self) -> Option<&dyn Validator> {
        Some(&*self.validator)
    }

    fn generate_default_config(&self) -> Result<PathBuf, Error> {
        NiriTool::generate_default_config(self)
    }
}

impl KdlBackedTool for NiriTool {
    /// Override the default `load_kdl` to also build the Semantic-Path index.
    ///
    /// Default behaviour (inherited from the trait) reads + parses the file;
    /// this override adds SemanticIndex construction and stores both the doc
    /// and index in `state` so that subsequent lookups (e.g. locating the
    /// source span for `["binds", "Mod+Return"]`) hit the precomputed
    /// HashMap instead of walking the tree O(N) on every keystroke.
    fn load_kdl(&self, path: &Path) -> Result<ConfigDoc, Error> {
        let text = std::fs::read_to_string(path)?;
        let doc = load_config(&text)?;
        let index = build_index(&doc);

        let mut state = self.state.lock().unwrap();
        state.active_path = Some(path.to_path_buf());
        state.doc = Some(doc.clone());
        state.index = Some(index);

        Ok(doc)
    }
}
