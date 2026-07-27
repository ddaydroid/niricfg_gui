//! Integration test target for `src/core/config_loader.rs`.
//!
//! Three test cases per Wave 1 Step 2 spec:
//!   1. Well-formed multi-section config — fakes a small `[system] / [display]
//!      / [hotkeys]` tower-of-hanoi-style config that any future
//!      `ToolPlugin::apply_saved` could read.
//!   2. Malformed config — unbalanced brace; assert `Err(Error::Kdl(_))`.
//!   3. Empty document edge case — `""` is valid KDL and maps to an empty
//!      `ConfigDoc`, NOT to a parse error.

use dotcfg_gui::{load_config, ConfigDoc, Error};

#[test]
fn well_formed_multi_section_config_loads_with_three_top_level_nodes() {
    let text = r#"
system {
    hostname "my-linux-box"
}
display {
    resolution 1920 1080
    refresh 60
}
hotkeys {
    close-window "Mod+Q"
}
"#;
    let doc: ConfigDoc = load_config(text).expect("well-formed KDL text must parse cleanly");
    let nodes = doc.nodes();
    assert_eq!(
        nodes.len(),
        3,
        "expected 3 top-level sections (system, display, hotkeys); got {}",
        nodes.len()
    );
}

#[test]
fn malformed_config_returns_error_kdl_not_panic() {
    // Missing the closing brace on `system`, intentional syntax error.
    let text = r#"
system {
    hostname "my-linux-box"
"#;
    let result = load_config(text);
    assert!(
        matches!(result, Err(Error::Kdl(_))),
        "expected Err(Error::Kdl(_)) for malformed input; got {:?}",
        result
    );
}

#[test]
fn empty_document_is_accepted_as_an_empty_configdoc() {
    let text = "";
    let doc = load_config(text).expect("empty string is valid KDL in v6");
    assert_eq!(
        doc.nodes().len(),
        0,
        "empty document must map to a ConfigDoc with zero nodes"
    );
}
