//! Atomic write-then-rename config writer.
//!
//! Wave 1 Step 6 of `.specs/tasks/todo/implement-dotcfg-gui.feature.md`
//! (out-of-spec numbering — user-defined). This module is the spec's
//! home for the "preserve user comments, whitespace, and ordering on
//! save" mandate: by writing the `ConfigDoc` via a tempfile in the
//! same directory as `target` and then atomically renaming, we
//! guarantee that any concurrent reader (the running niri compositor,
//! the inotify watcher, a save-during-editing scenario) sees either the
//! old file or the new file — never a half-written one.
//!
//! Algorithm: NameTempFile in `target.parent()` → write → fsync →
//! `persist(target)`. Each step's failure rolls back cleanly:
//! `NamedTempFile::Drop` unlinks the temp file automatically on any
//! error path before `persist`.
//!
//! Atomicity trade-off: `sync_all` fsyncs the file but NOT the parent
//! directory. On POSIX this is sufficient to survive process crashes,
//! but a kernel-level power loss between the rename and the dir-journal
//! write could still surface the rename to the journal after a restart
//! and lose the contents under a kernel-specific edge case. Dotfile
//! configs are small, frequently re-saved, and low-value at risk — the
//! industry-standard "fsync the file" trade-off is the right call here.
//! Promoting to `fsync(dir)` is a one-line fix deferred to Wave 6 if
//! any user reports a real crash-loss.

use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use crate::core::config_loader::ConfigDoc;
use crate::core::error::Error;

/// Atomically write a [`ConfigDoc`] to `target`.
///
/// The write is implemented as the textbook tempfile-in-parent-dir +
/// fsync + rename recipe so a crashed mid-save leaves either the old
/// contents or the new contents on disk, never a hybrid.
///
/// Errors:
/// - `Error::Io(InvalidInput)` if `target` has no parent directory
///   (e.g. exactly `"/"` or `""`).
/// - `Error::Io(_)` if the parent directory is missing, is not writable,
///   the temp-file create fails, the write fails, the fsync fails, or
///   the rename fails. All branch on `std::io::Error` and auto-map
///   through `Error::Io(#[from] …)`.
///
/// Drop-semantics: on any `Err` return, the `NamedTempFile` is dropped
/// without `persist`, which unlinks the on-disk temp file. The
/// caller's `target` path is never partially overwritten.
pub fn save_config(doc: &ConfigDoc, target: &Path) -> Result<(), Error> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "save_config target has no parent directory",
        )
    })?;

    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(doc.to_string().as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(target).map_err(|e| e.error)?;
    Ok(())
}
