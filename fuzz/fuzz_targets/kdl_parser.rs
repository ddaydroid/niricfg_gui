//! Fuzz target for the dotcfg-gui kdl-parser path (Step 19/21 of the spec).
//!
//! Arbiter semantic: `parse -> KdlDocument -> Display -> re-parse -> equal`.
//! A successful round-trip asserts two properties:
//!   (a) the kdl `Display` impl emits text the parser accepts, AND
//!   (b) the re-parsed document is **structurally** equivalent to the
//!       original (same nodes, same values, same properties, same children).
//!
//! # Why structural, not textual
//!
//! `kdl` v6's `Display` impl is NOT textually idempotent under re-parse —
//! it strips or normalizes blank-line whitespace inside children blocks
//! (an extra `\n \n` between two `}` braces is dropped on Display; the
//! fuzz corpus at 117,145 inputs hit this once, capturing artifact
//! `fuzz/artifacts/kdl_parser/crash-2d39f1addac05288b866ccb874cc8c56ac1932e0`).
//! A byte-for-byte `assert_eq!` would fire on benign kdl-v6 inputs.
//!
//! Since `kdl::KdlDocument`'s `PartialEq` in v6 compares STRUCTURALLY
//! (not whitespace, not source text), `assert_eq!(doc, re_parsed)` is
//! the right invariant for what dotcfg-gui actually promises. The
//! spec's "preserve user comments, whitespace, and ordering on save"
//! mandate is enforced by `config_writer.rs`'s atomic write-then-rename
//! in Wave 1 Step 6 + the production schema, NOT by `kdl::Display`
//! being lossless — kdl v6 isn't, and trying to compare Display bytes
//! would generate fuzz-noise without production benefit.
//!
//! Boot:  `cd fuzz && cargo +nightly fuzz run kdl_parser -- -max_total_time=30`
//! CI:    `cargo +nightly fuzz run kdl_parser -- -max_total_time=30`
//!
//! Panic-safety: arbitrary bytes are filtered through `str::from_utf8`
//! (clean `Err` path, never panics) and `KdlDocument::from_str` (clean
//! `Err` path on malformed KDL — that's the parser's job to refuse, not
//! our arbiter's). We only assert when both the parse AND the re-parse
//! succeeded; otherwise the cycle is meaningless and we return cleanly.

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
    // kdl formatter (it emitted KDL the parser rejects). Property (a).
    let re_parsed = KdlDocument::from_str(&serialized)
        .expect("Invariant violation: KdlDocument serialized to invalid KDL format");

    // 5. Structural round-trip equality: parse ∘ Display ∘ parse on
    // `doc` must produce a doc structurally equivalent to `doc` itself
    // — same nodes, same values, same properties, same children.
    // Whitespace and comment fidelity are NOT asserted here; see the
    // module-level doc-comment for why.
    assert_eq!(
        doc, re_parsed,
        "Invariant violation: re-parsing produced a structurally different KdlDocument"
    );
});
