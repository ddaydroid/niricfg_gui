//! Core error type plus the small enums shared between ToolPlugin
//! implementations and the shell. Uses `thiserror::Error` (no `anyhow`) so
//! downstream callers can pattern-match on the variants.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("kdl error: {0}")]
    Kdl(#[from] kdl::KdlError),

    #[error("plugin error: {0}")]
    Plugin(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalChangeAction {
    /// The file changed outside the editor and we are not dirty — silently
    /// reload the editor's view.
    Reload,
    /// The file changed outside the editor and we are dirty — keep our edits,
    /// ignore the external change.
    Ignore,
    /// The file changed outside the editor and we are dirty — surface the
    /// conflict to the user (`Adw.Banner` Reload / Ignore).
    AskUser,
}
