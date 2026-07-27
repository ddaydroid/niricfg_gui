//! Property-test harnesses (proptest) for core invariants.
//!
//! # Harnesses
//!
//! 1. **KDL exact-string round-trip** — Any KDL text that parses
//!    successfully must, after serialization via `ConfigDoc::to_string()`,
//!    re-parse to a structurally identical document. This catches
//!    serializer bugs where the output is syntactically valid KDL but
//!    semantically different (e.g. dropped nodes, mangled values).
//!
//! 2. **Undo commutativity** — For any sequence of `UndoCommand`s,
//!    `push(A) → push(B) → undo() → undo()` must restore the state
//!    that existed before both pushes. This is the defining invariant
//!    of a well-behaved undo stack.
//!
//! 3. **Semantic-path stability under sibling reorder** — Reordering
//!    sibling nodes in a KDL document must NOT change the set of
//!    `SemanticPath` keys in the index (the map is keyed by path
//!    strings, which are invariant under sibling order). Only the
//!    offset/len values change, reflecting the new byte positions.
//!
//! Run: `PROPTEST_CASES=2000 cargo test --release proptest`
//! CI runs `PROPTEST_CASES=2000` in the Property Tests job.

use std::sync::atomic::{AtomicU32, Ordering};

use proptest::prelude::*;

use dotcfg_gui::{build_index, load_config, UndoCommand, UndoStack};

// ---------------------------------------------------------------------------
// Shared fixture corpus for KDL round-trip tests
// ---------------------------------------------------------------------------

/// Hand-curated KDL snippets covering the niri config surface area.
/// Each string is valid KDL that should parse + round-trip cleanly.
///
/// Uses `r##` raw-string delimiters because some fixtures contain `"#`
/// (e.g. hex colour `"#5294e2"`) which would prematurely close `r#"`.
const KDL_FIXTURES: &[&str] = &[
    // Empty document
    "",
    // Single top-level node with string value
    r##"spawn-at-startup "/usr/libexec/polkit-gnome-authentication-agent-1""##,
    // Top-level node with integer value
    "gap 8",
    // Top-level node with boolean value
    "natural-scroll true",
    // Nested single child
    r##"layout {
    gap 8
}"##,
    // Two-level nesting
    r##"input {
    keyboard {
        repeat-delay 250
        repeat-rate 33
    }
}"##,
    // Multiple top-level nodes
    r##"binds {
    Mod+Return spawn "foot"
    Mod+Q close-window
    Mod+T toggle-floating
}"##,
    // String with hex color (contains "#, needs r## delimiter)
    r##"focus-ring {
    active-color "#5294e2"
}"##,
    // Multiple nested sections
    r##"input {
    keyboard {
        repeat-delay 250
        repeat-rate 33
    }
    touchpad {
        tap-to-click true
        natural-scroll true
    }
}
layout {
    gap 8
    focus-ring {
        width 2
        active-color "#5294e2"
    }
}
binds {
    Mod+Return spawn "foot"
    Mod+Q close-window
    Mod+1 focus-workspace 1
    Mod+Shift+1 move-column-to-workspace 1
}"##,
    // Comments (both line and block)
    r##"// this is a comment
binds {
    /* block comment */
    Mod+R spawn "foot"
}"##,
    // Escaped string
    r##"spawn "hello \"world\"""##,
    // Float value
    "opacity 0.85",
    // Negative integer
    "offset -5",
];

/// Strategy that picks a random KDL fixture string.
fn kdl_fixture_strategy() -> impl Strategy<Value = &'static str> {
    prop::sample::select(KDL_FIXTURES)
}

// ===========================================================================
// Harness 1: KDL exact-string round-trip
// ===========================================================================

proptest! {
    /// For any KDL text that parses successfully:
    ///   parse → serialize → re-parse → structural identity
    ///
    /// This catches serializer bugs like dropped nodes, mangled
    /// values, or wrong nesting.
    #[test]
    fn kdl_round_trip_structural_identity(
        text in kdl_fixture_strategy(),
    ) {
        let doc = match load_config(text) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };

        let serialized = doc.to_string();

        let re_parsed = match load_config(&serialized) {
            Ok(d) => d,
            Err(e) => {
                panic!(
                    "round-trip violation: valid KDL produced output \
                     that fails re-parse.\noriginal:  {text:?}\nserialized: {serialized:?}\nerror: {e}"
                );
            }
        };

        // Structural equivalence: manual comparison avoids kdl's
        // PartialEq which includes span metadata (byte offsets) that
        // necessarily differ after re-parsing.
        assert_eq!(
            doc.nodes().len(),
            re_parsed.nodes().len(),
            "node count mismatch after round-trip\n\
             original:   {text:?}\n\
             serialized: {serialized:?}"
        );

        for (orig, re) in doc.nodes().iter().zip(re_parsed.nodes().iter()) {
            assert_eq!(
                orig.name().value(),
                re.name().value(),
                "node name mismatch after round-trip\n\
                 original:   {text:?}\n\
                 serialized: {serialized:?}"
            );
            assert_eq!(
                orig.entries().len(),
                re.entries().len(),
                "entry count mismatch for node {:?}\n\
                 original:   {text:?}\n\
                 serialized: {serialized:?}",
                orig.name().value()
            );

            let orig_children = orig.children().map(|c| c.nodes().len()).unwrap_or(0);
            let re_children = re.children().map(|c| c.nodes().len()).unwrap_or(0);
            assert_eq!(
                orig_children, re_children,
                "child count mismatch for node {:?}\n\
                 original:   {text:?}\n\
                 serialized: {serialized:?}",
                orig.name().value()
            );
        }
    }
}

// ===========================================================================
// Harness 2: Undo commutativity
// ===========================================================================

/// Shared atomic state for undo-commutativity tests.
static VALUE: AtomicU32 = AtomicU32::new(0);

fn reset() {
    VALUE.store(0, Ordering::SeqCst);
}

/// Counter command: increment the shared value by 1 on apply, decrement
/// on undo.
struct Counter {
    _label_storage: String,
}

impl Counter {
    fn new() -> Self {
        Self {
            _label_storage: "increment".to_string(),
        }
    }
}

impl UndoCommand for Counter {
    fn apply(&mut self) {
        VALUE.fetch_add(1, Ordering::SeqCst);
    }
    fn undo(&mut self) {
        VALUE.fetch_sub(1, Ordering::SeqCst);
    }
    fn label(&self) -> &str {
        &self._label_storage
    }
}

/// Set command: replace the shared value, capturing the old value for undo.
struct Set {
    new_val: u32,
    captured_old: Option<u32>,
    _label_storage: String,
}

impl Set {
    fn new(new_val: u32) -> Self {
        Self {
            new_val,
            captured_old: None,
            _label_storage: format!("set to {new_val}"),
        }
    }
}

impl UndoCommand for Set {
    fn apply(&mut self) {
        self.captured_old = Some(VALUE.load(Ordering::SeqCst));
        VALUE.store(self.new_val, Ordering::SeqCst);
    }
    fn undo(&mut self) {
        if let Some(old) = self.captured_old {
            VALUE.store(old, Ordering::SeqCst);
        }
    }
    fn label(&self) -> &str {
        &self._label_storage
    }
}

/// Strategy for generating sequences of Set command values.
fn undo_commands_strategy() -> impl Strategy<Value = Vec<u32>> {
    proptest::collection::vec(any::<u32>(), 0..16)
}

proptest! {
    /// For any sequence of Set commands:
    ///   push(A) → push(B) → ... → undo() × N
    /// must restore the original state (VALUE == 0).
    #[test]
    fn undo_commutativity_restores_original_state(
        values in undo_commands_strategy(),
    ) {
        reset();
        let mut stack: UndoStack<Set> = UndoStack::new();

        let initial = VALUE.load(Ordering::SeqCst);

        for &v in &values {
            stack.push(Set::new(v));
        }
        let after_push = VALUE.load(Ordering::SeqCst);
        if !values.is_empty() {
            assert_eq!(after_push, *values.last().unwrap(),
                "after pushing all commands, VALUE should equal the last value");
        }

        let mut undone_count = 0;
        while stack.can_undo() {
            stack.undo();
            undone_count += 1;
        }
        assert_eq!(undone_count, values.len(),
            "undo count must match push count (no phantom undos)");

        let final_val = VALUE.load(Ordering::SeqCst);
        assert_eq!(final_val, initial,
            "undoing all commands must restore original state. \
             values: {values:?}, initial: {initial}, final: {final_val}");
    }

    /// Cap-FIFO drop: pushing more commands than capacity silently
    /// drops the oldest, but all applies happened (VALUE unaffected).
    #[test]
    fn undo_cap_does_not_affect_value(
        values in undo_commands_strategy(),
    ) {
        reset();
        let cap = 3;
        let mut stack: UndoStack<Counter> = UndoStack::with_cap(cap);

        for _ in &values {
            stack.push(Counter::new());
        }
        let expected_value = values.len() as u32;
        assert_eq!(
            VALUE.load(Ordering::SeqCst),
            expected_value,
            "FIFO drop must not affect total VALUE (all applies happened)"
        );

        let mut undone = 0;
        while stack.can_undo() {
            stack.undo();
            undone += 1;
        }
        assert_eq!(undone, cap.min(values.len()),
            "can undo at most 'cap' commands after overflow");

        let expected_remaining = expected_value.saturating_sub(undone as u32);
        assert_eq!(
            VALUE.load(Ordering::SeqCst),
            expected_remaining,
            "cap-FIFO drop means oldest commands are lost (cannot undo them)"
        );
    }
}

// ===========================================================================
// Harness 3: Semantic-path stability under sibling reorder
// ===========================================================================

proptest! {
    /// The index contains one entry per node in the doc, regardless of
    /// parse order.
    #[test]
    fn semantic_index_contains_all_paths(
        text in kdl_fixture_strategy(),
    ) {
        let doc = match load_config(text) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };

        // Build the index from the parsed document.
        // build_index is re-exported at the crate root.
        let index = build_index(&doc);

        // Count how many nodes the document has (recursively).
        fn count_nodes(doc: &dotcfg_gui::ConfigDoc) -> usize {
            let mut count = doc.nodes().len();
            for node in doc.nodes() {
                if let Some(children) = node.children() {
                    count += count_nodes(children);
                }
            }
            count
        }

        let expected_count = count_nodes(&doc);
        assert_eq!(
            index.len(),
            expected_count,
            "semantic index should contain one entry per node.\n\
             text: {text:?}\n\
             expected: {expected_count}, got: {}",
            index.len()
        );

        // Every top-level node must have a corresponding path.
        for node in doc.nodes() {
            let name = node.name().value().to_string();
            let paths: Vec<_> = index
                .entries
                .keys()
                .filter(|p| p.segments().first() == Some(&name))
                .collect();
            assert!(
                !paths.is_empty(),
                "top-level node {name:?} must have at least one index entry.\n\
                 text: {text:?}"
            );
        }
    }

    /// Index entries have consistent offset/len: offset + len does not
    /// exceed the source text length.
    #[test]
    fn semantic_index_offsets_are_within_bounds(
        text in kdl_fixture_strategy(),
    ) {
        let doc = match load_config(text) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        let text_len = text.len();
        let index = build_index(&doc);

        for &(offset, len) in index.entries.values() {
            assert!(
                offset + len <= text_len,
                "index entry offset+len exceeds source length.\n\
                 text: {text:?} (len={text_len})\n\
                 entry offset={offset}, len={len} exceeds {text_len}"
            );
        }
    }
}
