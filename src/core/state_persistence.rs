//! State persistence via `glib::KeyFile` stored at
//! `~/.config/dotcfg-gui/state.ini`. Saves and restores window geometry
//! and the last-active tool across restarts.
//!
//! Entire module is gated behind `#[cfg(feature = "gtk")]` because
//! `glib::KeyFile` is a GTK/GLib type not available in
//! `--no-default-features` builds.

#![cfg(feature = "gtk")]

use glib::KeyFile;
use std::path::PathBuf;

/// Subdirectory under `$XDG_CONFIG_HOME` (or `~/.config`) for state files.
const STATE_DIR: &str = "dotcfg-gui";
/// KeyFile filename that stores window geometry + last tool.
const STATE_FILE: &str = "state.ini";

/// Serializable subset of the shell's window state persisted between runs.
pub struct ShellWindowState {
    pub width: i32,
    pub height: i32,
    pub last_tool: Option<String>,
}

impl Default for ShellWindowState {
    fn default() -> Self {
        Self {
            width: 1000,
            height: 700,
            last_tool: None,
        }
    }
}

/// Resolve the state file path under the user's config directory.
///
/// Follows the XDG Base Directory Specification: uses
/// `$XDG_CONFIG_HOME/dotcfg-gui/state.ini` when the env var is set,
/// otherwise `~/.config/dotcfg-gui/state.ini`. Falls back to
/// `/tmp` when neither `$XDG_CONFIG_HOME` nor `$HOME` is set
/// (unusual — prevents a crash in headless test / CI contexts).
fn state_file_path() -> PathBuf {
    let config = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config.join(STATE_DIR).join(STATE_FILE)
}

/// Load the persisted shell state from `~/.config/dotcfg-gui/state.ini`.
///
/// Returns sensible defaults when:
/// - The state file does not exist (first run).
/// - The state file is corrupt or unparseable (manual edit, downgrade).
/// - Any individual key is missing (partial upgrade).
pub fn load_shell_window_state() -> ShellWindowState {
    let path = state_file_path();
    if !path.exists() {
        return ShellWindowState::default();
    }

    let kf = KeyFile::new();
    if kf
        .load_from_file(&path, glib::KeyFileFlags::empty())
        .is_err()
    {
        return ShellWindowState::default();
    }

    ShellWindowState {
        width: kf.integer("Window", "width").unwrap_or(1000),
        height: kf.integer("Window", "height").unwrap_or(700),
        last_tool: kf
            .string("Session", "last_tool")
            .ok()
            .map(|s| s.to_string()),
    }
}

/// Persist the current window geometry and active tool to disk.
///
/// Creates `~/.config/dotcfg-gui/` if it does not exist. Silently
/// ignores write errors (permission denied, read-only filesystem).
/// The state is advisory — losing it on a crash is acceptable.
pub fn save_shell_window_state(width: i32, height: i32, last_tool: Option<&str>) {
    let kf = KeyFile::new();
    kf.set_integer("Window", "width", width);
    kf.set_integer("Window", "height", height);
    if let Some(tool) = last_tool {
        kf.set_string("Session", "last_tool", tool);
    }

    let path = state_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Silently ignore write errors (state is advisory).
    let _ = kf.save_to_file(&path);
}
