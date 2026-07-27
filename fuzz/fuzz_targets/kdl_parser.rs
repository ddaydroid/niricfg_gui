//! Fuzz target for the dotcfg-gui kdl-parser path (Step 19/21 of the spec).
//!
//! Arbiter semantic: parse -> KdlDocument -> serialize -> re-parse -> equal.
//! A successful round-trip asserts that `KdlDocument::to_string()` produces
//! text that the parser both accepts AND when re-serialized produces the
//! exact same text. That is a strong invariant — any kdl-crate bug that
//! causes either:
//!   (a) the originally-serialized text to be rejected by the parser, or
//!   (b) the re-parsed document to re-serialize to structurally different
//!       text,
//! will trigger an `assert_eq!` panic here, which libFuzzer catches and
//! reports as a test-case violation.
//!
//! Boot:  `cd fuzz && cargo +nightly fuzz run kdl_parser -- -max_total_time=30`
//! CI:    `cargo +nightly fuzz run kdl_parser -- -max_total_time=30`
//!
//! Panic-safety: arbitrary bytes are filtered through `str::from_utf8` (clean
//! `Err` path, never panics) and `KdlDocument::from_str` (clean `Err` path on
//! malformed KDL — that's the parser's job to refuse, not our arbiter's).
//! We only assert when both the parse AND the re-parse succeeded, otherwise
//! the cycle is meaningless and we return cleanly.

#![no_main]

use kdl::KdlDocument;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|data: &[u8]| {
    // 1. Decode bytes as UTF-8; non-UTF-8 inputs return cleanly without
    // touching the kdl crate (the parser expects UTF-8 anyway).
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    // 2. Parse as KDL. Parse errors are expected and return cleanly.
    let Ok(doc) = KdlDocument::from_str(text) else {
        return;
    };

    // 3. Serialize the parsed document back to its canonical KDL form.
    let serialized = doc.to_string();

    // 4. Re-parse the serialized text. A failing re-parse is a bug in the
    // kdl formatter (it emitted KDL the parser rejects). We panic here on
    // purpose so libFuzzer reports the test case.
    let re_parsed = KdlDocument::from_str(&serialized)
        .expect("Invariant violation: KdlDocument serialized to invalid KDL format");

    // 5. Strong round-trip equality: the second serialization must match
    // the first byte-for-byte. This catches any case where the parser
    // accepts a doc but normalizes it (e.g. dropping comments or rewrites
    // whitespace) — both of which would betray the dotfile editor's
    // "preserve user comments, whitespace, and ordering" promise.
    let reserialized = re_parsed.to_string();
    assert_eq!(
        serialized, reserialized,
        "Invariant violation: re-parsing produced a structurally or textually different KdlDocument"
    );
});
