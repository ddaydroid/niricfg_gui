//! Undo / redo stack. The stack is generic over a user-defined `UndoCommand`
//! type; the editor's per-tool fields are concrete `UndoCommand` impls.

use std::collections::VecDeque;

/// A reversible action recorded by the editor. Implementors hold the inverse
/// state they need so that `apply -> undo -> apply` is a clean round-trip.
pub trait UndoCommand: Send + Sync {
    fn apply(&mut self);
    fn undo(&mut self);
    fn label(&self) -> &str;
}

/// Bounded undo history (FIFO drop on overflow) with a paired redo buffer
/// that is cleared by any new `push`.
pub struct UndoStack<T: UndoCommand> {
    undo_buf: VecDeque<T>,
    redo_buf: VecDeque<T>,
    dirty: bool,
    cap: usize,
}

impl<T: UndoCommand> Default for UndoStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: UndoCommand> UndoStack<T> {
    /// New stack with the default cap of 256 commands.
    pub fn new() -> Self {
        Self::with_cap(256)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self {
            undo_buf: VecDeque::with_capacity(cap.min(1024)),
            redo_buf: VecDeque::new(),
            dirty: false,
            cap,
        }
    }

    /// Apply immediately and record the command for future undo. Any pending
    /// redo buffer is cleared (typical editor semantics: branching the
    /// history invalidates the redo path).
    pub fn push(&mut self, mut cmd: T) {
        cmd.apply();
        if self.cap > 0 {
            while self.undo_buf.len() >= self.cap {
                self.undo_buf.pop_front();
            }
            self.undo_buf.push_back(cmd);
        }
        self.redo_buf.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> Option<&T> {
        let mut cmd = self.undo_buf.pop_back()?;
        cmd.undo();
        self.redo_buf.push_back(cmd);
        self.dirty = true;
        self.redo_buf.back()
    }

    pub fn redo(&mut self) -> Option<&T> {
        let mut cmd = self.redo_buf.pop_back()?;
        cmd.apply();
        self.undo_buf.push_back(cmd);
        self.dirty = true;
        self.undo_buf.back()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_buf.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_buf.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Caller invokes after a successful `save` to clear the dirty flag.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn history_len(&self) -> usize {
        self.undo_buf.len()
    }

    /// Newest-first iterator over the undo history (for UI menu listings).
    pub fn iter_undo(&self) -> impl Iterator<Item = &T> {
        self.undo_buf.iter().rev()
    }
}
