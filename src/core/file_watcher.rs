//! Async wrapper around `notify` v7's recommended file-system watcher.
//!
//! Wave 1 Step 5 of `.specs/tasks/todo/implement-dotcfg-gui.feature.md`.
//! Targets `cargo nextest run --no-default-features` (no GTK, async-std
//! runtime). Forwards `notify::Event`s into an
//! `async_std::channel::Receiver<PathBuf>` so callers can `await` path
//! changes without polling OS-level file APIs directly.
//!
//! # Architecture
//!
//! - The `notify::RecommendedWatcher` owns the OS inotify backend; we
//!   hold it inside `FileWatcher::_watcher` so its lifetime matches the
//!   receive loop. Dropping `FileWatcher` closes the watcher → the
//!   upstream `std::sync::mpsc::Receiver` disconnects → the bridge task
//!   detects the disconnect on next poll → exits → `next_event` then
//!   returns `None`.
//! - The bridge is a single async green-thread that polls
//!   `std::sync::mpsc::Receiver<notify::Result<notify::Event>>` via
//!   `try_recv()`, forwarding each path into the async-std channel with
//!   `tx.send().await`. Empty polls yield via `async_std::task::yield_now`
//!   (cheap — the scheduler relaxes for one tick).
//!
//! # Why poll-and-yield instead of spawn_blocking + blocking_send?
//!
//! async-std's `Sender<T>` only exposes async `send()` — there is **no**
//! `blocking_send` analogue. The viable alternatives:
//! a) `spawn_blocking` the recv loop and call
//!    `async_std::task::block_on(tx.send(p))` per send — wasteful per-call
//!    executor setup.
//! b) Hold a green-thread on `mpsc::recv()` directly — works in practice
//!    because async-std's lazy worker expansion handles it, but formally
//!    unsound (a held worker can starve sibling green-threads).
//! c) Poll with `try_recv` + `yield_now` (chosen here). Idle cost is one
//!    OS-poll per scheduler tick (~1 ms); event cost is one send per
//!    `notify::Event`. Bounded channel at 64 caps memory under storms.
//!
//! # Field drop order
//!
//! `rx` is declared first so when `FileWatcher` drops, `rx` releases the
//! only consumer first; the bridge's `tx.send().await` then returns
//! `Err(SendError)` and the bridge exits cleanly. `_watcher` then drops,
//! the notify handler closure is dropped, `raw_tx` is dropped, and the
//! bridge's `try_recv()` returns `Err(Disconnected)` — already-exited
//! task is a no-op. So either drop order is observably equivalent; we
//! keep `rx` first as future-maintainer documentation.

use std::path::PathBuf;

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};

use crate::core::error::Error;

/// Async file-system watcher. Drop to stop watching (the async-std
/// channel closes and `next_event` returns `None` on the next call).
pub struct FileWatcher {
    rx: async_std::channel::Receiver<PathBuf>,
    // Held to keep the OS inotify backend alive.
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Begin watching each entry in `paths` (non-recursive: hooks to the
    /// path itself, not its children). Returns a `FileWatcher` whose
    /// `next_event` yields one `PathBuf` per path per `notify::Event`.
    ///
    /// Runbook: paths must exist at the moment of `watch()`; absent
    /// paths return `Err(Error::FileWatcher(_))` from the underlying
    /// notify backend. Directories pass in non-recursive mode, so child
    /// file events are NOT reported.
    pub async fn watch(paths: Vec<PathBuf>) -> Result<Self, Error> {
        // Bounded async-std channel (cap 64): backpressure observable
        // without unbounded memory in pathological storms.
        let (tx, rx) = async_std::channel::bounded::<PathBuf>(64);

        // `notify`'s recommended watcher takes a synchronous handler; we
        // give it a closure that pushes events into a std mpsc channel
        // that the bridge below polls.
        let (raw_tx, raw_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();

        let mut watcher = recommended_watcher(move |res| {
            // Best-effort: if the bridge already dropped, the closed
            // upstream channel here is fine — notify will be dropped
            // shortly too.
            let _ = raw_tx.send(res);
        })
        .map_err(|e| Error::FileWatcher(e.to_string()))?;

        for path in &paths {
            watcher
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(|e| Error::FileWatcher(format!("watch({path:?}): {e}")))?;
        }

        // Bridge: poll `raw_rx` on a green-thread. Per-event cost is
        // one `tx.send().await`; idle cost is one `yield_now()` per
        // scheduler tick. The async-std channel's bounded `64` provides
        // backpressure if the consumer falls behind.
        async_std::task::spawn(async move {
            loop {
                match raw_rx.try_recv() {
                    Ok(Ok(event)) => {
                        for p in event.paths {
                            // Returns Err if the consumer (FileWatcher)
                            // has been dropped. Bridge exits cleanly.
                            if tx.send(p).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // Either notify reported a transient error or
                        // there are no pending events; yield to the
                        // scheduler so other green-threads can run.
                        async_std::task::yield_now().await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Notify watcher was dropped (the FileWatcher's
                        // `_watcher` field was dropped). Bridge exit.
                        return;
                    }
                }
            }
        });

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }

    /// Await the next path-change event, or `None` if the watcher has
    /// been dropped (the async-std channel closed). Maps the underlying
    /// `Result<PathBuf, RecvError>` to `Option<PathBuf>` per the public
    /// API contract.
    pub async fn next_event(&self) -> Option<PathBuf> {
        self.rx.recv().await.ok()
    }
}
