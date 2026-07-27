//! Validators. Container for plugin-agnostic format validation rules
//! (the niri `Validator` impl will spawn `niri msg validate` per Step 9;
//! future sway/hyprland impls may shell out to their respective
//! validators).
//!
//! # Architecture
//!
//! Each `ToolPlugin` (e.g. NiriTool in Wave 2) owns a
//! `Box<dyn Validator>`; the shell calls `validator.validate_kdl(&text)`
//! shortly after each edit (250ms debounce per spec). Errors are
//! surfaced as `Error::Kdl|Io|Plugin(_)` and bubbled up to the shell's
//! `Adw.Banner` rendering path.
//!
//! # Object safety
//!
//! `Validator::validate_kdl` returns `BoxFuture<'a, …>` rather than
//! `async fn` because native async-fn-in-trait (Rust 1.75+) is NOT
//! object-safe (`impl Future<…>` in trait return position cannot be
//! used behind `dyn Trait`). Manual `BoxFuture` enables
//! `Box<dyn Validator>` storage in the shell — the canonical pattern
//! before the `dyn async fn` stabilization lands (target: 2026+, no
//! timeline yet).
//!
//! # Debounce
//!
//! The default debounce hint is 250ms (spec Wave 2 Step 9). Validators
//! with expensive validation (subprocess spawns) can override to e.g.
//! 500ms; tests typically set 0ms to skip debounce entirely.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::core::error::{Error, ValidationIssue};

/// Type alias for a boxed, Send-able, `'a`-bounded future. Manual
/// definition (vs. the `async-trait` crate) keeps `dyn Validator`
/// object-safe without per-call boxing overhead. The caller is not
/// expected to construct this directly — implement
/// [`Validator::validate_kdl`] via `Box::pin(async move { … })`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A plugin-agnostic validator. Held inside each `ToolPlugin` as
/// `Box<dyn Validator>` (set in Wave 2's NiriTool constructor).
/// Future shells call `validator.validate_kdl(&text)` on each edit
/// (250ms debounce per spec).
pub trait Validator: Send + Sync {
    /// Validator's display name (`niri`, `sway`, `noop`, etc.) for
    /// logging and UI rendering.
    fn name(&self) -> &'static str;

    /// Validate the given KDL text. Returns a (possibly empty) list
    /// of issues; an empty list means the doc is valid.
    ///
    /// Errors:
    /// - `Error::Kdl(_)` if the validator's defensive pre-parse layer
    ///   rejected the input.
    /// - `Error::Io(_)` for filesystem-related failures (subprocess
    ///   spawn, logfile read, etc.).
    /// - `Error::Plugin(_)` for validator-specific failures.
    fn validate_kdl<'a>(
        &'a self,
        text: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>>;

    /// Validator-specific debounce hint. Defaults to 250ms per spec.
    /// Override to extend (e.g. 500ms for `niri msg validate`'s
    /// subprocess spawn overhead) or nullify (0ms in `NoopValidator`
    /// so tests don't wait).
    fn debounce_hint(&self) -> Duration {
        Duration::from_millis(250)
    }
}

/// A no-op validator. Returns `Ok(vec![])` for any input — no issues
/// of any severity. Useful for headless tests and pure-file plugins
/// (TOML/YAML/INI for sway/hyprland/waybar) that don't require
/// compositor-side validation.
pub struct NoopValidator;

impl Validator for NoopValidator {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn validate_kdl<'a>(
        &'a self,
        _text: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>> {
        Box::pin(async move { Ok(vec![]) })
    }

    fn debounce_hint(&self) -> Duration {
        Duration::from_millis(0)
    }
}

/// A canned-issue validator. Returns its preset `Vec<ValidationIssue>`
/// regardless of the input text. Replaces the spec's planned
/// `tests/mocks/validate-fake.sh` for unit-level testing: tests that
/// need a "this validator always reports X" stub struct-construct a
/// `CannedValidator { name, issues }` and route through the same
/// `Box<dyn Validator>` plumbing the real NiriValidator will use.
pub struct CannedValidator {
    /// Display name surfaced to UI / logging.
    pub name: &'static str,
    /// Issues returned for any input.
    pub issues: Vec<ValidationIssue>,
}

impl Validator for CannedValidator {
    fn name(&self) -> &'static str {
        self.name
    }

    fn validate_kdl<'a>(
        &'a self,
        _text: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ValidationIssue>, Error>> {
        // Clone the preset issues into the future-own state so that
        // callers see a freshly-owned Vec (matches the production-path
        // contract where each call yields a fresh result).
        let issues = self.issues.clone();
        Box::pin(async move { Ok(issues) })
    }

    fn debounce_hint(&self) -> Duration {
        Duration::from_millis(0)
    }
}
