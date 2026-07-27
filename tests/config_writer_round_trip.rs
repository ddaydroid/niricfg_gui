//! Integration test target for `src/core/config_writer.rs`.
//!
//! Three test cases per Wave 1 Step 6 spec:
//!   1. **Round-trip on a 3-node config** — parse a small multi-section
//!      KDL, save to a tempdir target, read back, structurally compare.
//!   2. **Read-only parent directory** — chmod the tempdir itself to
//!      mode `0o555` (no write permission). `save_config` must fail
//!      with `Err(Error::Io(_))` because `NamedTempFile::new_in` and
//!      the persist step both need write permission on `parent`.
//!   3. **Nonexistent parent directory** — target a path under a
//!      not-yet-created subdirectory of the tempdir. `save_config`
//!      must fail with `Err(Error::Io(_))`; `NamedTempFile::new_in`
//!      will propagate ENOENT from the OS.
//!
//! Round-trip comparison deliberately avoids full `assert_eq!` on
//! `ConfigDoc`: per the fuzz-demote commit `d25b77b`,
//! `kdl::KdlDocument`'s `PartialEq` includes metadata (format, span)
//! that Display round-trips re-derive, so byte-equality is unsound.
//! Comparing `nodes().len()` + per-node `name().value()` is the
//! stable, kdl-v6-friendly structural check.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;

use dotcfg_gui::{load_config, save_config, ConfigDoc, Error};

#[test]
fn save_then_load_three_node_config_structurally_matches() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let target = tmp.path().join("three-nodes.kdl");

    let text = r#"
system {
    hostname "my-linux-box"
}
display {
    resolution 1920 1080
}
hotkeys {
    close-window "Mod+Q"
}
"#;
    let original = load_config(text).expect("well-formed KDL parses");

    save_config(&original, &target).expect("save_config into writable tempdir");

    let on_disk = std::fs::read_to_string(&target).expect("read back from disk");
    assert!(
        !on_disk.is_empty(),
        "save_config must persist the serialized doc, not write 0 bytes"
    );

    let reloaded = load_config(&on_disk).expect("re-loaded doc parses");

    // Stable structural comparison (NOT full PartialEq — see module doc).
    assert_eq!(
        reloaded.nodes().len(),
        original.nodes().len(),
        "node count survives round-trip"
    );
    for (i, (a, b)) in reloaded
        .nodes()
        .iter()
        .zip(original.nodes().iter())
        .enumerate()
    {
        assert_eq!(
            a.name().value(),
            b.name().value(),
            "top-level node #{i} name survives round-trip"
        );
    }
}

#[test]
fn save_into_readonly_parent_directory_returns_io_error() {
    let tmp = tempfile::tempdir().expect("create tempdir");

    // Pre-create the target file inside the tempdir so the read-only
    // chmod happens AFTER a known-good state. Drop-ordering (chmod back
    // before TempDir::Drop) lets the cleanup succeed.
    let target = tmp.path().join("readonly-target.kdl");
    std::fs::write(&target, "").expect("pre-create target file");

    // Strip write permission from the tempdir itself (0o555 = r-x for all).
    std::fs::set_permissions(tmp.path(), Permissions::from_mode(0o555))
        .expect("chmod tempdir to read-only");

    // Any io::Error from here (open(O_TMPFILE), write, fsync, rename)
    // propagates through `?` and `Error::Io(#[from] _)`. A broad
    // assertion is correct here: we don't want to couple the test to
    // *which* internal step fails on a given glibc/kernel combo.
    let doc = ConfigDoc::default();
    let result = save_config(&doc, &target);

    // Restore write permission BEFORE asserting so a test failure on
    // the assertion line doesn't leak a read-only tempdir.
    std::fs::set_permissions(tmp.path(), Permissions::from_mode(0o777))
        .expect("chmod tempdir back to writable for cleanup");

    assert!(
        matches!(result, Err(Error::Io(_))),
        "expected Err(Error::Io(_)) for read-only parent dir; got {result:?}"
    );
}

#[test]
fn save_into_nonexistent_parent_directory_returns_io_error() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    // The subdirectory `does-not-exist-yet` is intentionally absent.
    let target = tmp.path().join("does-not-exist-yet/config.kdl");

    let doc = ConfigDoc::default();
    let result = save_config(&doc, &target);

    assert!(
        matches!(result, Err(Error::Io(_))),
        "expected Err(Error::Io(_)) for missing parent dir; got {result:?}"
    );
}
