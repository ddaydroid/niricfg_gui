//! Integration test target for the Semantic-Path Indexing foundation.
//!
//! Three test cases (kdl v6 grammar aware):
//!   1. **`index_flat_config_indexes_parent_plus_nested_key_nodes`** —
//!      `system { hostname "box" } display { width 1920 } hotkeys {
//!      close "Mod+Q" }` produces 6 entries: each top-level parent
//!      PLUS the nested key-name nodes. Under kdl v6's grammar,
//!      `system { hostname "box" }` is parsed as parent `system` plus
//!      child node `hostname("box")` — `key value` form creates a
//!      child node, while `key=value` would be a property entry.
//!   2. **`index_nested_braces_include_grandchildren`** —
//!      `binds { Mod+Return { spawn "alacritty" } Mod+Q { spawn
//!      "kill-window" } }` produces 5 entries (1 parent + 2 children
//!      + 2 grandchildren: `binds`, `binds/Mod+Return`,
//!      `binds/Mod+Return/spawn`, `binds/Mod+Q`, `binds/Mod+Q/spawn`).
//!   3. **`index_lookup_api_returns_span_for_indexed_path`** —
//!      `SemanticIndex::lookup(["system"])` returns the source
//!      (`offset`, `len`) byte-range; lookup of a genuinely
//!      non-existent path returns `None`.
//!
//! Test 3 additionally exercises the lookup API with non-existent
//! paths and partial paths to confirm the failure mode never
//! panics — important because the shell's bind-row lookup uses this
//! method heavily at edit time and a typo must NOT crash.

use std::str::FromStr;

use dotcfg_gui::{build_index, SemanticPath};
use kdl::KdlDocument;

#[test]
fn index_flat_config_indexes_parent_plus_nested_key_nodes() {
    // Under kdl v6's grammar, `system { hostname "box" }` parses as
    // parent node `system` PLUS a child node `hostname("box")` — the
    // `key value` form creates a child node, NOT a property entry
    // (the equals form `key=value` would be a property entry). The
    // walker correctly indexes BOTH the parent name AND the nested
    // key-node name.
    let text = "system { hostname \"box\" }\ndisplay { width 1920 }\nhotkeys { close \"Mod+Q\" }\n";
    let doc = KdlDocument::from_str(text).expect("flat KDL parses");
    let index = build_index(&doc);

    assert_eq!(
        index.len(),
        6,
        "3 parent nodes + 3 nested key-name child nodes = 6 entries (kdl v6 grammar)"
    );

    let paths: Vec<String> = index
        .entries
        .keys()
        .map(SemanticPath::to_display_string)
        .collect();

    assert!(paths.contains(&"system".to_string()), "system indexed");
    assert!(paths.contains(&"display".to_string()), "display indexed");
    assert!(paths.contains(&"hotkeys".to_string()), "hotkeys indexed");
    // Nested key-name nodes ARE indexed under kdl v6's grammar.
    assert!(
        paths.contains(&"system/hostname".to_string()),
        "system/hostname indexed (kdl v6 grammar: hostname inside braces is a child node)"
    );
    assert!(
        paths.contains(&"display/width".to_string()),
        "display/width indexed"
    );
    assert!(
        paths.contains(&"hotkeys/close".to_string()),
        "hotkeys/close indexed"
    );
}

#[test]
fn index_nested_braces_include_grandchildren() {
    // `binds { Mod+Return { spawn "alacritty" } Mod+Q { spawn
    // "kill-window" } }` produces 5 entries: parent `binds`, plus 2
    // child chord nodes, plus 2 grandchild `spawn` nodes (because
    // `spawn "alacritty"` is again a child node under Mod+Return,
    // not a property entry).
    let text = "binds {\n    Mod+Return {\n        spawn \"alacritty\"\n    }\n    Mod+Q {\n        spawn \"kill-window\"\n    }\n}\n";
    let doc = KdlDocument::from_str(text).expect("nested KDL parses");
    let index = build_index(&doc);

    assert_eq!(
        index.len(),
        5,
        "1 parent + 2 children + 2 grandchildren = 5 entries (full kdl v6 grammar)"
    );

    let display_paths: Vec<String> = index
        .entries
        .keys()
        .map(SemanticPath::to_display_string)
        .collect();

    assert!(
        display_paths.contains(&"binds".to_string()),
        "parent path 'binds' indexed"
    );
    assert!(
        display_paths.contains(&"binds/Mod+Return".to_string()),
        "child chord 'binds/Mod+Return' indexed with full path"
    );
    assert!(
        display_paths.contains(&"binds/Mod+Q".to_string()),
        "child chord 'binds/Mod+Q' indexed with full path"
    );
    assert!(
        display_paths.contains(&"binds/Mod+Return/spawn".to_string()),
        "grandchild 'binds/Mod+Return/spawn' indexed"
    );
    assert!(
        display_paths.contains(&"binds/Mod+Q/spawn".to_string()),
        "grandchild 'binds/Mod+Q/spawn' indexed"
    );

    // Partial-path lookup MUST still return None: paths with missing
    // segments do not match any indexed full-path entry.
    assert_eq!(
        index.lookup(&["Mod+Return".to_string()]).map(|_| true),
        None,
        "lookup with only the leaf segment (missing parent) returns None -- partial paths are NOT supported"
    );
    assert_eq!(
        index
            .lookup(&["binds".to_string(), "ghost".to_string()])
            .map(|_| true),
        None,
        "lookup with valid parent + bogus child returns None"
    );
}

#[test]
fn index_lookup_api_returns_span_for_indexed_path() {
    // `system { hostname "box" }` produces 2 entries under kdl v6's
    // grammar (the parent AND the nested key-node).
    let text = "system { hostname \"box\" }\n";
    let doc = KdlDocument::from_str(text).expect("parse");
    let index = build_index(&doc);

    assert_eq!(
        index.len(),
        2,
        "1 parent + 1 nested key-node = 2 entries (full kdl v6 grammar)"
    );

    let (offset, len) = index
        .lookup(&["system".to_string()])
        .copied()
        .expect("'system' is indexed and present");
    assert_eq!(
        offset, 0,
        "first node 'system' starts at source-text offset 0"
    );
    assert!(len > 0, "span has positive length");

    // The nested key-node IS indexed under kdl v6 grammar -- this
    // affirms the contract, not a regression.
    let (host_offset, host_len) = index
        .lookup(&["system".to_string(), "hostname".to_string()])
        .copied()
        .expect("'system/hostname' is indexed under kdl v6 grammar");
    assert!(
        host_offset > offset,
        "system/hostname offset ({host_offset}) is AFTER the parent system offset ({offset})"
    );
    assert!(host_len > 0, "host span has positive length");

    // Genuinely non-existent paths return None (never panic).
    assert_eq!(
        index.lookup(&["nonexistent".to_string()]),
        None,
        "lookup of a non-indexed path returns None"
    );
    assert_eq!(
        index
            .lookup(&["system".to_string(), "ghost".to_string()])
            .map(|_| true),
        None,
        "lookup of a valid parent + bogus child returns None"
    );
}
