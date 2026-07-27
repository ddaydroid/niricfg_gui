//! Integration test target for Wave 1 Step 4 `ToolRegistry`.

use std::path::{Path, PathBuf};

use dotcfg_gui::{DynTool, Error, ExternalChangeAction, ToolPlugin, ToolRegistry, ValidationIssue};
use proptest::prelude::*;

/// Minimal mock plugin. The `id()` leak pattern (`Box::leak(String → &'static str)`)
/// is acceptable for tests: each call to `id()` leaks one `String`, so total
/// leakage is bounded by test run length × N plugins × `id()` calls. For
/// production, plugins should use `&'static str` constants directly.
#[derive(Clone, Debug)]
struct TestPlugin {
    id_owned: String,
    api_version: u32,
}

impl TestPlugin {
    fn new(id: &str, api_version: u32) -> Self {
        Self {
            id_owned: id.to_owned(),
            api_version,
        }
    }
}

impl ToolPlugin for TestPlugin {
    fn id(&self) -> &'static str {
        // MEMORY: Box::leak intentionally leaks one String per id() call so
        // the dyn trait can return &'static str from owned state. Leak is
        // bounded by test run length × plugin count × id() calls; production
        // plugins should use &'static str constants directly.
        Box::leak(self.id_owned.clone().into_boxed_str())
    }
    fn display_name(&self) -> &'static str {
        "Mock"
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

fn mk_tool(id: &str, api_version: u32) -> DynTool {
    Box::new(TestPlugin::new(id, api_version))
}

#[test]
fn register_then_iter_sees_all_three_tools_in_insertion_order() {
    let mut registry = ToolRegistry::new();

    registry
        .register(mk_tool("alpha", 1))
        .expect("first register ok");
    registry
        .register(mk_tool("beta", 1))
        .expect("distinct id register ok");
    registry
        .register(mk_tool("gamma", 1))
        .expect("distinct id register ok");

    let observed: Vec<&'static str> = registry.as_slice().iter().map(|t| t.id()).collect();
    assert_eq!(
        observed,
        vec!["alpha", "beta", "gamma"],
        "iter() must preserve insertion order"
    );
}

#[test]
fn register_rejects_duplicate_id_with_error_plugin() {
    let mut registry = ToolRegistry::new();
    registry
        .register(mk_tool("dup", 1))
        .expect("first register ok");

    let result = registry.register(mk_tool("dup", 1));
    assert!(
        matches!(result, Err(Error::Plugin(_))),
        "second register of 'dup' must return Err(Error::Plugin(_)); got {:?}",
        result
    );
    // Verify the registry did NOT silently overwrite the original:
    let observed: Vec<&'static str> = registry.as_slice().iter().map(|t| t.id()).collect();
    assert_eq!(
        observed,
        vec!["dup"],
        "duplicate register must NOT overwrite the original tool"
    );
}

#[test]
fn unregister_returns_none_for_missing_id_and_some_for_present() {
    let mut registry = ToolRegistry::new();
    registry
        .register(mk_tool("a", 1))
        .expect("first register ok");

    assert!(registry.unregister("missing").is_none());
    let removed = registry.unregister("a");
    assert!(removed.is_some(), "unregister returns Some for present id");
    assert!(
        registry.as_slice().is_empty(),
        "registry is empty after the only tool is unregistered"
    );
    // Re-registering after unregister is allowed (id no longer in registry):
    registry
        .register(mk_tool("a", 1))
        .expect("re-register after unregister is allowed");
}

#[test]
fn is_plugin_compatible_returns_true_for_newer_or_equal_false_for_older_or_missing() {
    let mut registry = ToolRegistry::new();
    registry
        .register(mk_tool("legacy", 1))
        .expect("first register ok");
    registry
        .register(mk_tool("modern", 5))
        .expect("second register ok");

    // Forward-compatibility semantics: api_version(actual) >= api_version(required).
    assert!(
        registry.is_plugin_compatible("modern", 2),
        "tool with api_version=5 must satisfy a request for api_version=2"
    );
    assert!(
        registry.is_plugin_compatible("legacy", 1),
        "exact-match (5 >= 5) is also OK; tool with api_version=1 must satisfy a request for api_version=1"
    );
    assert!(
        !registry.is_plugin_compatible("legacy", 2),
        "tool with api_version=1 must fail a request for api_version=2"
    );
    assert!(
        !registry.is_plugin_compatible("missing", 1),
        "missing tool must fail-safe to false"
    );
}

proptest! {
    /// Fuzz-free property test: register → unregister → register should be
    /// idempotent w.r.t. the registry's shape. Asserts:
    ///   (i) duplicate-id register returns `Err(Error::Plugin(_))` until unregister,
    ///  (ii) unregister returns `Some(tool)` iff the id was registered,
    /// (iii) post-unregister re-register succeeds (slot is reclaimed).
    #[test]
    fn register_unregister_register_is_idempotent(id in "[a-z]{1,5}") {
        let mut registry = ToolRegistry::new();
        let tool = mk_tool(&id, 1);

        // (i) First register succeeds; second register of the same id fails.
        registry.register(tool).expect("first register");
        prop_assert!(matches!(
            registry.register(mk_tool(&id, 1)),
            Err(Error::Plugin(_))
        ));

        // (ii) Unregister returns Some for the present id; registry empty afterward.
        let removed = registry.unregister(&id);
        prop_assert!(removed.is_some());
        prop_assert_eq!(registry.as_slice().len(), 0);

        // (iii) Re-register succeeds (slot freed).
        prop_assert!(registry.register(mk_tool(&id, 1)).is_ok());
        prop_assert_eq!(registry.as_slice().len(), 1);
    }
}
