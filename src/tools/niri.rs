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
pub struct NiriTool {
    config_dir: PathBuf,
    state: Mutex<NiriToolState>,
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

    /// Construct a new `NiriTool` with default config directory resolution
    /// and an empty initial state.
    pub fn new() -> Self {
        Self {
            config_dir: Self::default_config_dir(),
            state: Mutex::new(NiriToolState {
                active_path: None,
                doc: None,
                index: None,
            }),
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
        // Wave 2 Step 9 will spawn `niri msg validate` here. For now,
        // return an empty list so the shell's validator-loop has nothing
        // to display.
        Ok(Vec::new())
    }

    fn apply_saved(&self) -> Result<(), Error> {
        // Future: spawn `niri msg validate` via async-process and surface
        // any validation issues to the user. For now, silently succeed.
        Ok(())
    }

    fn on_external_change(&self) -> ExternalChangeAction {
        // Niri's config is reloaded at runtime by the compositor — the file
        // may change from outside the GUI. Default to Reload (the editor
        // silently re-reads the file) unless the user has unsaved edits
        // (that conflict is surfaced at the shell level).
        ExternalChangeAction::Reload
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
