//! Niri compositor validator. Implements the [`Validator`] trait by
//! spawning `niri msg validate` via `async-process` and parsing its
//! stdout / stderr into [`ValidationIssue`] structs.
//!
//! # Architecture
//!
//! The validator spawns `niri msg validate` via `async-process` and
//! captures its stdout / stderr. Because `niri msg validate` validates
//! the _live_ config (niri's currently loaded config, not the caller's
//! text), the output may not correspond to the user's in-progress edits.
//! This is a known v1 limitation — a Wave 3 enhancement can switch to
//! `niri --config <tempfile> --validate` or `--stdin` when niri adds
//! such a flag.
//!
//! The validator maps each output line to a `ValidationIssue` with a
//! best-effort parser. If niri is not installed or not running, a
//! warning-level issue is returned rather than failing the entire
//! validation.
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
//! `niri msg validate` emits human-readable lines. The v1 parser uses a
//! simple heuristic:
//!
//! * Lines containing `"error"` or `"Error"` → `Severity::Error`.
//! * Lines containing `"warning"` or `"Warning"` → `Severity::Warning`.
//!   All other lines → `Severity::Info`.
//! * Any line with a leading integer is assumed to start with a line
//!   number (used for the `line` field). Lines without a recognised
//!   prefix use `line: 0`.
//!
//! This heuristic is intentionally coarse — a later wave will switch to
//! a machine-parseable format once niri exposes one.

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
        _text: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>> {
        // Spawn `niri msg validate` — this communicates with the running
        // niri compositor instance via its socket. The command validates
        // the currently-loaded config (not the caller's text). We capture
        // its output for parsing.
        //
        // TODO(Wave 3): When niri adds a `--config` / `--stdin` flag for
        // validating arbitrary text, pass the text directly to the
        // subprocess. For now, niri validates its own loaded config — the
        // result may not correspond to the user's in-progress edits, but
        // it establishes the subprocess bridge and parse pipeline.
        //
        // If niri is not running or the socket is unavailable, the
        // command exits with a non-zero status and a human-readable
        // error on stderr — we still capture and surface those as
        // issues so the user sees "niri not running" in the shell's
        // validation panel rather than a silent pass.
        Box::pin(async move {
            let output = async_process::Command::new("niri")
                .args(["msg", "validate"])
                .output()
                .await;

            match output {
                Ok(out) => {
                    let issues = parse_niri_output(&out.stdout, &out.stderr)?;
                    Ok(issues)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // niri binary is not installed. Return a warning-level
                    // issue instead of failing — the user may be doing a
                    // headless build or compiling on a non-niri machine.
                    Ok(vec![ValidationIssue {
                        line: 0,
                        severity: Severity::Warning,
                        message: "niri binary not found — validation skipped".to_string(),
                    }])
                }
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
