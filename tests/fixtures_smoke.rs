//! Wave 6 Step 20: Fixture & Mock Integration.
//!
//! Each fixture file in `tests/fixtures/` is loaded via `load_config`,
//! verified to parse (or fail as expected), then exercised through the
//! semantic-path index and (for valid files) a save→load round-trip.
//! Also runs the mock `validate-fake.sh` shell script against each fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

use dotcfg_gui::{build_index, load_config, save_config};

// ---------------------------------------------------------------------------
// Fixture discovery
// ---------------------------------------------------------------------------

/// Returns the path to `tests/fixtures/` relative to the crate root.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Returns the path to `tests/mocks/validate-fake.sh`.
fn mock_validator() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("mocks")
        .join("validate-fake.sh")
}

/// Known-valid fixtures that must parse without error.
const VALID_FIXTURES: &[&str] = &[
    "minimal.kdl",
    "typical.kdl",
    "comments-rich.kdl",
    "binds-heavy.kdl",
    "nested.kdl",
    "empty.kdl",
];

/// Fixtures that should produce a parse error.
const INVALID_FIXTURES: &[&str] = &["invalid-syntax.kdl"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_fixture(name: &str) -> String {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn valid_fixtures_parse_successfully() {
    for name in VALID_FIXTURES {
        let text = read_fixture(name);
        let result = load_config(&text);
        assert!(
            result.is_ok(),
            "fixture {name} should parse OK but got error: {:?}",
            result.err(),
        );
    }
}

#[test]
fn invalid_fixture_returns_kdl_error() {
    for name in INVALID_FIXTURES {
        let text = read_fixture(name);
        let result = load_config(&text);
        assert!(
            result.is_err(),
            "fixture {name} should produce an error but parsed OK",
        );
        // The error must come from KDL parsing, not IO or plugin code.
        match result {
            Err(dotcfg_gui::Error::Kdl(_)) => {} // expected
            Err(other) => panic!("fixture {name}: expected Error::Kdl, got {other:?}"),
            Ok(_) => unreachable!(),
        }
    }
}

#[test]
fn valid_fixtures_build_semantic_index() {
    for name in VALID_FIXTURES {
        let text = read_fixture(name);
        let doc = load_config(&text).expect("valid fixture should parse");
        let index = build_index(&doc);

        if name == &"empty.kdl" {
            assert!(
                index.entries.is_empty(),
                "empty.kdl should produce an empty index"
            );
        } else {
            assert!(
                !index.entries.is_empty(),
                "{name}: expected at least one semantic entry"
            );
        }

        // Every entry's offset+len must be within the source text bounds.
        for &(offset, len) in index.entries.values() {
            let end = offset + len;
            assert!(
                end <= text.len(),
                "{name}: offset={offset} + len={len} exceeds text length {}",
                text.len(),
            );
        }
    }
}

#[test]
fn typical_round_trips_through_save_and_load() {
    let text = read_fixture("typical.kdl");
    let doc = load_config(&text).expect("typical.kdl should parse");

    // Save to a temp file, re-load, verify structural equivalence.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
    let target = tmp.path().to_path_buf();
    save_config(&doc, &target).expect("save should succeed");

    let reloaded_text = std::fs::read_to_string(&target).expect("read saved file");
    let reloaded_doc = load_config(&reloaded_text).expect("re-load should succeed");

    assert_eq!(doc.nodes().len(), reloaded_doc.nodes().len());
    // KdlDocument has no .entries() — only KdlNode has entries.
    // Structural comparison is done via node count + node names below.

    for (i, orig) in doc.nodes().iter().enumerate() {
        let reloaded = &reloaded_doc.nodes()[i];
        assert_eq!(orig.name().value(), reloaded.name().value());
    }
}

#[test]
fn mock_validator_accepts_valid_fixtures() {
    let validator = mock_validator();
    assert!(
        validator.exists(),
        "mock validator not found at {:?}",
        validator
    );

    for name in VALID_FIXTURES {
        let fixture = fixtures_dir().join(name);
        let output = Command::new(&validator)
            .arg(&fixture)
            .output()
            .unwrap_or_else(|e| panic!("failed to run validator on {name}: {e}"));

        assert!(
            output.status.success(),
            "validator rejected valid fixture {name}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn mock_validator_rejects_invalid_fixtures() {
    let validator = mock_validator();
    for name in INVALID_FIXTURES {
        let fixture = fixtures_dir().join(name);
        let output = Command::new(&validator)
            .arg(&fixture)
            .output()
            .unwrap_or_else(|e| panic!("failed to run validator on {name}: {e}"));

        assert!(
            !output.status.success(),
            "validator should reject invalid fixture {name}",
        );
    }
}
