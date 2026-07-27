//! Regression-pinning smoke test for the lib-root re-exports declared in
//! `src/lib.rs`.
//!
//! The CI `Lint & Format` job runs `cargo clippy --all-targets --features gtk
//! -- -D warnings`, which compiles `src/shell.rs` and therefore requires the
//! re-exports `pub use crate::core::tool_plugin::DynTool` and `pub use
//! crate::core::tool_plugin::ToolPlugin` to resolve at `crate::DynTool` /
//! `crate::ToolPlugin` respectively (not e.g. `crate::core::DynTool`). A
//! future careless rename of any re-export in `src/lib.rs` would break that
//! compile. This test pins the lib-root symbols locally so we catch the
//! regression before pushing.
//!
//! `tests/undo_stack_round_trip.rs` is the substantive integration test for
//! `UndoStack<T>`; this file is purely a type-resolution smoke test on the
//! public re-exports.

#[test]
fn lib_root_reexports_resolve() {
    use dotcfg_gui::{
        DynTool, Error, ExternalChangeAction, Severity, ToolPlugin, UndoCommand, ValidationIssue,
    };

    // The `Option<...>::None` literal never constructs a value, so we don't
    // need a concrete `ToolPlugin` / `UndoCommand` impl in this test binary —
    // the type-check itself is the assertion.
    let _: Option<DynTool> = None;
    let _: Option<Box<dyn UndoCommand>> = None;
    let _: Option<Error> = None;
    let _: Option<ExternalChangeAction> = None;
    let _: Option<Severity> = None;
    let _: Option<ValidationIssue> = None;
}
