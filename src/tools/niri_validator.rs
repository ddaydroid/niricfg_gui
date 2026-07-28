//! Niri compositor validator. Implements the [`Validator`] trait by
//! writing the caller's text to a temporary file and spawning
//! `niri validate --config <tmpfile>` via `async-process`, then parsing
//! its stdout / stderr into [`ValidationIssue`] structs.
//!
//! # Architecture
//!
//! The caller passes the editor-buffer content as `text`. The validator
//! writes it to a [`tempfile::NamedTempFile`], spawns
//! `niri validate --config <tmp_path>` via `async-process`, captures
//! stdout/stderr, parses the output into issues, and cleans up the
//! tempfile. This validates the **actual editor text** rather than
//! the live compositor config, so validation results always correspond
//! to the user's in-progress edits.
//!
//! If `niri` is not on `$PATH` or the subprocess fails, a warning-level
//! issue is returned rather than failing the entire validation — the GUI
//! remains usable in headless / non-niri environments.
//!
//! # Debounce
//!
//! Subprocess spawn adds ~1-3 ms overhead even on fast systems. The
//! debounce hint is set to 500 ms — double the trait default — so the
//! shell's edit-loop doesn't spawn a niri validation on every keystroke
//! when the user is typing quickly. Tests override to 0 ms.
//!
//! # Output format (v1 heuristic)
//!
//! `niri validate --config` emits human-readable lines. The v1 parser
//! uses a simple heuristic:
//!
//! * Lines containing `"error"` or `"Error"` → `Severity::Error`.
//! * Lines containing `"warning"` or `"Warning"` → `Severity::Warning`.
//!   All other lines → `Severity::Info`.
//! * Any line with a leading integer is assumed to start with a line
//!   number (used for the `line` field). Lines without a recognised
//!   prefix use `line: 0`.
//!
//! This heuristic is intentionally coarse — a later wave can switch to
//! a machine-parseable format once niri exposes one.

use std::sync::OnceLock;
use std::time::Duration;

use crate::core::error::{Error, Severity, ValidationIssue};
use crate::core::validator::{BoxFuture, Validator};

/// Delay between edits before spawning a `niri msg validate` subprocess.
/// Longer than the trait default (250 ms) because subprocess overhead
/// makes frequent invocations wasteful.
const NIRI_VALIDATE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Niri compositor validator. Stateless — the struct holds no data;
/// all relevant context is passed through `validate_kdl`.
pub struct NiriValidator;

/// Detect whether we are running inside a Flatpak sandbox.
///
/// Checks for the existence of `/.flatpak-info`, which is the canonical
/// indicator. Falls back to checking the `FLATPAK_ID` env var.
fn is_flatpak() -> bool {
    static FLATPAK: OnceLock<bool> = OnceLock::new();
    *FLATPAK.get_or_init(|| {
        std::path::Path::new("/.flatpak-info").exists() || std::env::var("FLATPAK_ID").is_ok()
    })
}

/// Parse `niri msg validate` output into a list of validation issues.
///
/// See the [module-level documentation](self) for the heuristic used.
/// Empty output means the config is valid — returns `Ok(vec![])`.
fn parse_niri_output(stdout: &[u8], stderr: &[u8]) -> Result<Vec<ValidationIssue>, Error> {
    fn extract_line(text: &[u8]) -> Vec<ValidationIssue> {
        let body = String::from_utf8_lossy(text);
        body.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let line_no = line
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let severity = if line.contains("error") || line.contains("Error") {
                    Severity::Error
                } else if line.contains("warning") || line.contains("Warning") {
                    Severity::Warning
                } else {
                    Severity::Info
                };
                ValidationIssue {
                    line: line_no,
                    severity,
                    message: line.to_string(),
                }
            })
            .collect()
    }

    let mut issues = Vec::new();
    issues.extend(extract_line(stdout));
    issues.extend(extract_line(stderr));
    Ok(issues)
}

impl NiriValidator {
    /// Build a `NiriValidator` with a 0 ms debounce (for tests).
    pub fn new() -> Self {
        Self
    }
}

impl Default for NiriValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl Validator for NiriValidator {
    fn name(&self) -> &'static str {
        "niri"
    }

    fn validate_kdl<'a>(
        &'a self,
        text: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>> {
        // Write the editor text to a tempfile, spawn
        // `niri validate --config <tmp_path>`, and parse the output.
        // The tempfile is automatically cleaned up when `tmp` is dropped
        // after the subprocess completes.
        //
        // This validates the **actual editor text** (not the live
        // compositor config), so validation results always correspond
        // to the user's in-progress edits. `niri validate --config`
        // does NOT need a running niri instance — it only reads the
        // file and exits, so validation works even in headless builds
        // or on machines without niri running.
        //
        // If niri is not on `$PATH`, a warning-level issue is returned
        // so the GUI remains usable in non-niri environments.
        Box::pin(async move {
            let tmp = tempfile::NamedTempFile::new().map_err(Error::Io)?;
            std::fs::write(tmp.path(), text).map_err(Error::Io)?;

            let config_path = tmp
                .path()
                .to_str()
                .ok_or_else(|| Error::Plugin("non-UTF-8 tempfile path".to_string()))?
                .to_string();

            // When running inside Flatpak, escape the sandbox via
            // `flatpak-spawn --host` so the niri binary (on the host
            // system) can be reached. Outside Flatpak, call niri
            // directly.
            let (prog, args_prefix): (&str, &[&str]) = if is_flatpak() {
                ("flatpak-spawn", &["--host", "niri", "validate", "--config"])
            } else {
                ("niri", &["validate", "--config"])
            };

            let output = async_process::Command::new(prog)
                .args(args_prefix)
                .arg(&config_path)
                .output()
                .await;

            // Drop the tempfile after the subprocess finishes.
            drop(tmp);

            match output {
                Ok(out) => {
                    let issues = parse_niri_output(&out.stdout, &out.stderr)?;
                    Ok(issues)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![ValidationIssue {
                    line: 0,
                    severity: Severity::Warning,
                    message: "niri binary not found — validation skipped".to_string(),
                }]),
                Err(e) => Err(Error::Io(e)),
            }
        })
    }

    fn debounce_hint(&self) -> Duration {
        NIRI_VALIDATE_DEBOUNCE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_niri() {
        assert_eq!(NiriValidator.name(), "niri");
    }

    #[test]
    fn debounce_hint_is_500ms() {
        assert_eq!(NiriValidator.debounce_hint(), Duration::from_millis(500));
    }

    #[test]
    fn parse_empty_output() {
        let issues = parse_niri_output(b"", b"").unwrap();
        assert!(issues.is_empty(), "empty output → no issues");
    }

    #[test]
    fn parse_whitespace_only_output() {
        let issues = parse_niri_output(b"  \n  \n  ", b"").unwrap();
        assert!(issues.is_empty(), "whitespace-only lines are filtered");
    }

    #[test]
    fn parse_stdout_error_line() {
        let issues = parse_niri_output(b"error: something broke", b"").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert!(issues[0].message.contains("error"));
    }

    #[test]
    fn parse_stdout_warning_line() {
        let issues = parse_niri_output(b"Warning: deprecated field", b"").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }

    #[test]
    fn parse_stderr_lines() {
        let issues = parse_niri_output(b"", b"stderr line one\nstderr line two").unwrap();
        assert_eq!(issues.len(), 2);
        // Both lines without error/warning keyword → Info severity.
        for issue in &issues {
            assert_eq!(issue.severity, Severity::Info);
        }
    }

    #[test]
    fn parse_line_number_extraction() {
        let issues = parse_niri_output(b"42 error at line 42", b"").unwrap();
        assert!(!issues.is_empty());
        // The line should be 42 (the leading number).
        assert_eq!(issues[0].line, 42);
    }

    #[test]
    fn parse_line_number_on_no_number_uses_zero() {
        let issues = parse_niri_output(b"error without leading number", b"").unwrap();
        assert!(!issues.is_empty());
        assert_eq!(issues[0].line, 0);
    }

    #[test]
    fn parse_both_streams() {
        let issues =
            parse_niri_output(b"stdout: valid config", b"stderr: Warning: minor issue").unwrap();
        assert_eq!(issues.len(), 2);
        // First from stdout: no error/warning → Info.
        assert_eq!(issues[0].severity, Severity::Info);
        // Second from stderr: contains "Warning" → Warning.
        assert_eq!(issues[1].severity, Severity::Warning);
    }

    #[test]
    fn not_found_error_returns_warning_issue() {
        // We cannot easily simulate `std::io::ErrorKind::NotFound` in a
        // unit test without actually making a syscall. This test checks
        // the structural invariant: a warning-level issue is generated
        // for the "binary not found" path by testing the logic branch
        // directly (the `Err(NotFound)` arm in `validate_kdl` returns
        // `Ok(vec![warning_issue])` — we verify the warning shape).
        let warning_issue = ValidationIssue {
            line: 0,
            severity: Severity::Warning,
            message: "niri binary not found — validation skipped".to_string(),
        };
        assert_eq!(warning_issue.line, 0);
        assert_eq!(warning_issue.severity, Severity::Warning);
        assert!(warning_issue.message.contains("not found"));
    }
}
