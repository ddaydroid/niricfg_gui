//! Integration test for the core `UndoStack<T>`.
//!
//! Runs under BOTH `cargo nextest run --no-default-features` AND
//! `cargo nextest run --features gtk`, so this file must not pull in any
//! glib / gtk4 / libadwaita type. We exercise the stack with two
//! `UndoCommand` implementations that share state through a
//! `static AtomicU32` — real `Send + Sync`, no `unsafe impl Send` to fake it.

use std::sync::atomic::{AtomicU32, Ordering};

use dotcfg_gui::{UndoCommand, UndoStack};

// ---- shared state ----
// Process-wide atomic so multiple commands observe the same value without
// needing Rc/RefCell tricks (which would not be Send+Sync).
static VALUE: AtomicU32 = AtomicU32::new(0);

fn reset() {
    VALUE.store(0, Ordering::SeqCst);
}

// ---- Counter command (increment by 1) ----
struct Counter {
    label_storage: String,
}

impl Counter {
    fn new() -> Self {
        Self {
            label_storage: "increment by 1".to_string(),
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
        &self.label_storage
    }
}

// ---- Set command (replace the value, capturing the previous one for inverse) ----
struct Set {
    new_val: u32,
    captured_old: Option<u32>,
    label_storage: String,
}

impl Set {
    fn new(new_val: u32) -> Self {
        Self {
            new_val,
            captured_old: None,
            label_storage: format!("set VALUE to {}", new_val),
        }
    }
}

impl UndoCommand for Set {
    fn apply(&mut self) {
        // Capture before overwriting so `undo` can restore it.
        self.captured_old = Some(VALUE.load(Ordering::SeqCst));
        VALUE.store(self.new_val, Ordering::SeqCst);
    }
    fn undo(&mut self) {
        if let Some(old) = self.captured_old {
            VALUE.store(old, Ordering::SeqCst);
        }
    }
    fn label(&self) -> &str {
        &self.label_storage
    }
}

// ---- tests ----

#[test]
fn dirty_flag_starts_false_on_fresh_stack() {
    reset();
    let stack: UndoStack<Counter> = UndoStack::new();
    assert!(!stack.is_dirty());
    assert!(!stack.can_undo());
    assert!(!stack.can_redo());
    assert_eq!(stack.history_len(), 0);
}

#[test]
fn push_then_undo_restores_zero() {
    reset();
    let mut stack: UndoStack<Counter> = UndoStack::new();

    for _ in 0..3 {
        stack.push(Counter::new());
    }
    assert_eq!(
        VALUE.load(Ordering::SeqCst),
        3,
        "after 3 pushes, VALUE should be 3"
    );

    for _ in 0..3 {
        stack.undo();
    }
    assert_eq!(
        VALUE.load(Ordering::SeqCst),
        0,
        "after 3 undos, VALUE back to 0"
    );
    assert!(stack.can_redo(), "after undo, redo should be available");
    assert_eq!(stack.history_len(), 0, "all 3 undos consumed");
}

#[test]
fn redo_replays_undo_buffer() {
    reset();
    let mut stack: UndoStack<Counter> = UndoStack::new();

    stack.push(Counter::new());
    assert_eq!(VALUE.load(Ordering::SeqCst), 1);
    stack.undo();
    assert_eq!(VALUE.load(Ordering::SeqCst), 0);

    stack.redo();
    assert_eq!(
        VALUE.load(Ordering::SeqCst),
        1,
        "redo should re-apply the undo'd command"
    );
    assert!(!stack.can_redo());
    assert!(stack.can_undo());
}

#[test]
fn new_push_clears_redo_buffer() {
    reset();
    let mut stack: UndoStack<Counter> = UndoStack::new();

    stack.push(Counter::new()); // VALUE = 1
    stack.undo(); // VALUE = 0, redo_buf has 1 cmd
    assert!(stack.can_redo());

    stack.push(Counter::new()); // should clear redo_buf
    assert!(
        !stack.can_redo(),
        "pushing a new command should clear the redo buffer"
    );
    assert_eq!(VALUE.load(Ordering::SeqCst), 1);
}

#[test]
fn dirty_flag_survives_undo_redo() {
    reset();
    let mut stack: UndoStack<Counter> = UndoStack::new();

    stack.push(Counter::new());
    assert!(stack.is_dirty());
    stack.mark_clean();
    assert!(!stack.is_dirty());

    stack.push(Counter::new());
    assert!(stack.is_dirty(), "new push re-raises dirty");

    stack.undo();
    assert!(stack.is_dirty(), "undo does not clean (it modified state)");

    stack.redo();
    assert!(stack.is_dirty(), "redo does not clean (it modified state)");
}

#[test]
fn cap_truncates_oldest_history() {
    reset();
    let mut stack: UndoStack<Counter> = UndoStack::with_cap(3);

    for _ in 0..5 {
        stack.push(Counter::new());
    }
    assert_eq!(stack.history_len(), 3, "only the most recent 3 are kept");
    assert_eq!(VALUE.load(Ordering::SeqCst), 5, "all 5 apply's happened");

    stack.undo();
    assert_eq!(stack.history_len(), 2);
    assert!(stack.can_redo());
}

#[test]
fn undo_of_set_command_restores_previous_value() {
    reset();
    let mut stack: UndoStack<Set> = UndoStack::new();

    stack.push(Set::new(7));
    assert_eq!(VALUE.load(Ordering::SeqCst), 7);
    stack.undo();
    assert_eq!(
        VALUE.load(Ordering::SeqCst),
        0,
        "undo Set(7) restores pre-push value"
    );

    stack.push(Set::new(42));
    assert_eq!(VALUE.load(Ordering::SeqCst), 42);
    stack.push(Set::new(100));
    assert_eq!(VALUE.load(Ordering::SeqCst), 100);

    // Undo brings us back to Set(42)'s pre-push capture (= 42).
    stack.undo();
    assert_eq!(VALUE.load(Ordering::SeqCst), 42);
    // Undo brings us back to 0 (the pre-Set(42) capture).
    stack.undo();
    assert_eq!(VALUE.load(Ordering::SeqCst), 0);
}
