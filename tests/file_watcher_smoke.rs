//! Smoke test for the `FileWatcher` (notify v7 + async-std channel bridge).
//!
//! Verifies that writing to a watched path yields a path-change event
//! within a 2 s timeout. Runs alongside the other `--no-default-features`
//! tests in this crate (no GTK required).
//!
//! Test contract per `.specs/tasks/todo/implement-dotcfg-gui.feature.md`
//! Step 5:
//! 1. Create a `NamedTempFile`.
//! 2. Start watching its path (`FileWatcher::watch`).
//! 3. Write to the file from a separate async task.
//! 4. `await next_event()` with a 2 s timeout.
//! 5. Assert the reported `PathBuf` matches the watched path.

use std::time::Duration;

use dotcfg_gui::FileWatcher;
use tempfile::NamedTempFile;

#[test]
fn file_watcher_observes_write_to_watched_path() {
    async_std::task::block_on(async {
        let temp = NamedTempFile::new().expect("create NamedTempFile");
        let watched_path = temp.path().to_path_buf();

        let watcher = FileWatcher::watch(vec![watched_path.clone()])
            .await
            .expect("FileWatcher::watch");

        // Give the inotify hook a beat to install before we trigger an
        // event; otherwise write can race the install and the test would
        // accidentally miss the first event. A brief sleep on the green
        // thread here is fine — the test bench tolerates it and an
        // install race would be a real flake masking a notify bug.
        async_std::task::sleep(Duration::from_millis(50)).await;

        // Spawn the writer on a separate async task so production-side
        // and watcher-side truly run concurrently.
        let writer_path = watched_path.clone();
        let writer_task = async_std::task::spawn(async move {
            std::fs::write(&writer_path, b"hello world\n").expect("std::fs::write");
        });

        // Await the watched event with a 2 s timeout per spec.
        let event = async_std::future::timeout(Duration::from_secs(2), watcher.next_event())
            .await
            .expect("event arrived within 2 s timeout (no Elapsed)")
            .expect("event was Some (channel not closed)");

        // Wait for writer task to complete before the temp file is
        // dropped (NamedTempFile's Drop deletes the on-disk file; we want
        // the write to land first or we race the deletion).
        writer_task.await;

        assert_eq!(
            event, watched_path,
            "watched path matches the reported event path"
        );
    });
}
