//! Integration test target for the `KdlBackedTool` sub-trait wiring the
//! canonical `load_config` + `save_config` round-trip into a real
//! `ToolPlugin` impl.
//!
//! Three test cases:
//!   1. **`load_kdl_default_method_parses_three_node_config`** — a
//!      `KdlTestTool` reads a tempdir-resident KDL file via its inherited
//!      `load_kdl` default-method, asserting the parsed node count.
//!   2. **`save_kdl_default_method_round_trips_through_load_kdl`** — load
//!      via `load_kdl`, save to a different path via `save_kdl`, then
//!      reload from the new path and assert structural equality
//!      (nodes().len() + per-node name, NOT full PartialEq — see commit
//!      `d25b77b`'s kdl-v6 metadata-equality insight).
//!   3. **`load_kdl_default_method_returns_io_error_on_missing_path`** —
//!      `std::fs::read_to_string` on a non-existent path returns ENOENT,
//!      which propagates through `?` as `Error::Io(_)`.
//!
//! `KdlTestTool::new("...", api)` uses an explicit `&'static str` const for
//! `id()` (NOT `Box::leak`) — the test-pattern shift: when the id is known
//! at compile time, use a const; the `Box::leak` pattern in
//! `tests/tool_registry_round_trip.rs` is reserved for proptest-generated
//! ids where the string isn't known until runtime.

use std::path::{Path, PathBuf};

use dotcfg_gui::{Error, ExternalChangeAction, KdlBackedTool, ToolPlugin, ValidationIssue};

/// Minimal KDL-backed test plugin. Opts in to `KdlBackedTool` with a blank
/// impl (`{}`) so it inherits the canonical load-via-`load_config` plus
/// save-via-`save_config` default-methods. The blank-impl idiom preserves
/// the trait-extensibility point: a real `NiriTool` would override
/// `load_kdl` to attach a Semantic-Path index.
struct KdlTestTool {
    id: &'static str,
    api_version: u32,
}

impl KdlTestTool {
    const fn new(id: &'static str, api_version: u32) -> Self {
        Self { id, api_version }
    }
}

impl ToolPlugin for KdlTestTool {
    fn id(&self) -> &'static str {
        self.id
    }
    fn display_name(&self) -> &'static str {
        "KdlTestTool"
    }
    fn config_paths(&self) -> Vec<PathBuf> {
        vec![]
    }
    fn detect(&self, _path: &Path) -> bool {
        false
    }
    fn load(&self, _path: &Path) -> Result<(), Error> {
        Ok(())
    }
    fn save(&self) -> Result<(), Error> {
        Ok(())
    }
    fn validate(&self) -> Result<Vec<ValidationIssue>, Error> {
        Ok(vec![])
    }
    fn apply_saved(&self) -> Result<(), Error> {
        Ok(())
    }
    fn on_external_change(&self) -> ExternalChangeAction {
        ExternalChangeAction::Ignore
    }
    fn api_version(&self) -> u32 {
        self.api_version
    }
}

impl KdlBackedTool for KdlTestTool {} // blank impl: uses both default-methods

#[test]
fn load_kdl_default_method_parses_three_node_config() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let src = tmp.path().join("three-nodes.kdl");
    std::fs::write(
        &src,
        "system { hostname \"box\" }\ndisplay { resolution 1920 1080 }\nhotkeys { close-window \"Mod+Q\" }\n",
    )
    .expect("write source KDL");

    let tool = KdlTestTool::new("kdl-loader-test", 1);
    let doc = tool
        .load_kdl(&src)
        .expect("default load_kdl reads + parses without error");

    let names: Vec<&str> = doc.nodes().iter().map(|n| n.name().value()).collect();
    assert_eq!(
        names,
        vec!["system", "display", "hotkeys"],
        "kdl v6.7.1 preserves node-name ordering on parse"
    );
}

#[test]
fn save_kdl_default_method_round_trips_through_load_kdl() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let src = tmp.path().join("source.kdl");
    let dst = tmp.path().join("round-trip-dst.kdl");
    std::fs::write(
        &src,
        "system { hostname \"my-linux-box\" }\ndisplay { resolution 1920 1080 }\nhotkeys { close-window \"Mod+Q\" }\n",
    )
    .expect("write source KDL");

    let tool = KdlTestTool::new("kdl-round-trip-test", 1);
    let original = tool.load_kdl(&src).expect("load source KDL");

    tool.save_kdl(&original, &dst)
        .expect("save_kdl writes atomically without error");

    let reloaded = tool
        .load_kdl(&dst)
        .expect("reload survives a fresh load_kdl");

    // Stable structural comparison (NOT full PartialEq — see module doc).
    assert_eq!(
        reloaded.nodes().len(),
        original.nodes().len(),
        "node count survives the KdlBackedTool round-trip"
    );
    let original_names: Vec<&str> = original.nodes().iter().map(|n| n.name().value()).collect();
    let reloaded_names: Vec<&str> = reloaded.nodes().iter().map(|n| n.name().value()).collect();
    assert_eq!(
        original_names, reloaded_names,
        "node ordering survives the KdlBackedTool round-trip"
    );
}

#[test]
fn load_kdl_default_method_returns_io_error_on_missing_path() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let missing = tmp.path().join("does-not-exist.kdl");

    let tool = KdlTestTool::new("kdl-missing-test", 1);
    let result = tool.load_kdl(&missing);

    assert!(
        matches!(result, Err(Error::Io(_))),
        "expected Err(Error::Io(_)) for missing path; got {result:?}"
    );
}
