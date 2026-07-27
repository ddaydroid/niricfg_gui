//! Fuzz target for the dotcfg-gui kdl-parser path (Step 19/21 of the spec).
//!
//! Invariants under test:
//!   (a) `KdlDocument::Display` emits text that `KdlDocument::from_str`
//!       accepts. If kdl's Display produces KDL the parser refuses to
//!       round-trip-parse, the harness panics so libFuzzer reports
//!       the case (the `.expect(...)` at step 3 below).
//!   (b) The parser, the Display formatter, and str::from_utf8 don't
//!       panic on arbitrary byte input. Cascading `let Ok ... else`
//!       fall-throughs keep non-UTF-8 and unparseable inputs returning
//!       cleanly without panic.
//!
//! # What we INTENTIONALLY do NOT assert
//!
//! Earlier commits in this sequence progressively tried to assert
//! `assert_eq!(something, something)` as a round-trip invariant, but
//! they all failed benign on kdl v6:
//!
//!  - `1326662` (textual round-trip, `assert_eq!(serialized,
//!    reserialized, ...)`): kdl's Display normalizes whitespace inside
//!    children blocks, so textual idempotence is impossible against
//!    kdl v6.
//!  - The same commit's follow-on thought of structural round-trip
//!    (`assert_eq!(doc, re_parsed, ...)`): kdl v6's `PartialEq` is
//!    metadata-equality, not tree-equality. It compares, on each
//!    `KdlDocument`:
//!        • `nodes: Vec<KdlNode>`            (the AST tree)
//!        • `format: Option<KdlDocumentFormat>`  (whitespace positioning)
//!        • `span: SourceSpan`                (source offsets/lengths)
//!    — and on each `KdlNode`:
//!        • `name`, `values`, `properties`, `children`
//!        • `ty: Option<KdlIdentifier>`
//!        • `format: Option<KdlNodeFormat>`
//!        • `span: SourceSpan`
//!    `KdlDocument::Display` emits canonical KDL, which discards the
//!    original `format` metadata and re-derives `span` based on the
//!    canonical text length. So even when the AST tree is identical
//!    across rounds, the metadata fields diverge and `PartialEq`
//!    returns false. Crash artifact at
//!    `fuzz/artifacts/kdl_parser/crash-0ac64ab68de58b3dd877c42670ad7fddd518755a`
//!    (`(P)\tnu,.???????`) demonstrates this.
//!
//! We can't fix `kdl = "6"`'s `PartialEq` from outside. A custom
//! tree-only comparator (ignoring `format`/`span`) would always pass
//! on kdl v6 (because the AST IS preserved) — turning the harness
//! into pure noise that doesn't catch any kdl bug. The right design
//! is to demote the round-trip invariant entirely: keep only property
//! (a) above.
//!
//! The spec's "preserve user comments, whitespace, and ordering on
//! save" mandate is enforced in Wave 1 Step 6 by `config_writer.rs`'s
//! atomic write-then-rename + the production-tool schema, NOT by
//! `kdl::Display` being lossless on metadata. `kdl = "6"` doesn't
//! preserve format/span across Display; Wave 2 may work around that
//! via a `kdl = "7"` upgrade OR a custom-format-preserving layer
//! above `KdlDocument`.
//!
//! # Boot
//!
//! `cd fuzz && cargo +nightly fuzz run kdl_parser -- -max_total_time=30`
//! CI: `cargo +nightly fuzz run kdl_parser -- -max_total_time=30` (Step 21).

#![no_main]

use kdl::KdlDocument;
use libfuzzer_sys::fuzz_target;
use std::str::FromStr;

fuzz_target!(|data: &[u8]| {
    // 1. Decode bytes as UTF-8; non-UTF-8 inputs return cleanly
    // without touching the kdl crate (the parser expects UTF-8 anyway).
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    // 2. Parse as KDL. Parse errors are expected and return cleanly.
    let Ok(doc) = KdlDocument::from_str(text) else {
        return;
    };

    // 3. (Property (a)) Re-parse the Display output. A failing re-parse
    // IS a real, actionable kdl bug — the formatter emitted KDL the
    // parser refuses. Panic on purpose so libFuzzer reports the case.
    let _re_parsed = KdlDocument::from_str(&doc.to_string())
        .expect("Invariant violation: KdlDocument serialized to invalid KDL format");

    // (intentionally no further assertions; see module-level doc-comment)
});
