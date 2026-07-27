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
//! - The `notify::RecommendedWatcher` itself owns the OS inotify backend.
//!   We hold it inside `FileWatcher::_watcher` so its lifetime matches the
//!   receive loop. Dropping `FileWatcher` closes the watcher → the upstream
//!   `std::sync::mpsc::Receiver` disconnects → the `spawn_blocking` bridge
//!   exits → `next_event` then returns `None`.
//! - `notify`'s recommended watcher emits events on a `std::sync::mpsc::Sender`.
//!   An `async_std::task::spawn_blocking` task bridges that synchronous
//!   channel into our async-std channel: it `recv()`s each event off the
//!   std channel on a dedicated OS thread (won't starve async-std's worker
//!   pool), and `blocking_send`s the path into the async-std channel for
//!   downstream consumption.
//!
//! # Trade-offs
//!
//! - **One `PathBuf` per `next_event`**: events that carry multiple paths
//!   produce multiple consecutive `next_event` calls (in path order).
//!   Sufficient for Wave 3's external-change wiring; Wave 2's batched UI
//!   can drain a burst.
//! - **Bridging tone**: notify is fundamentally synchronous
//!   (`std::sync::mpsc::recv()` blocks). The separate blocking thread is
//!   necessary because `recv()` would otherwise park on an async-std
//!   worker, blocking every other green-thread on that scheduler thread.
//! - **Bounded channel (cap 64)**: a relink storm (e.g. `git checkout`
//!   inside the watched dir) won't grow memory unbounded; the oldest
//!   in-flight events stay in the watcher's std mpsc buffer instead.
//!
//! # Future migrations
//!
//! - Wave 2 may want batched events (`Option<Vec<PathBuf>>` per call) — at
//!   that point we'd swap the channel payload for a small `Vec`.
//! - Wave 3 may want a debouncer (notify-debouncer-full) —

use std::path::PathBuf;

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};

use crate::core::error::Error;

/// Async file-system watcher. Drop to stop watching (the channel closes
/// and `next_event` returns `None` on the next call).
pub struct FileWatcher {
    rx: async_std::channel::Receiver<PathBuf>,
    // Held to keep the OS inotify backend alive; dropped when
    // `FileWatcher` drops → closes the upstream notify channel → lets the
    // `spawn_blocking` bridge exit.
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Begin watching each entry in `paths` (non-recursive: hooks to the
    /// path itself, not its children). Returns a `FileWatcher` whose
    /// `next_event` yields one `PathBuf` per path per `notify::Event`.
    pub async fn watch(paths: Vec<PathBuf>) -> Result<Self, Error> {
        // Bounded async-std channel: cap 64. Bridges backpressure without
        // unbounded growth in pathological storm scenarios.
        let (tx, rx) = async_std::channel::bounded::<PathBuf>(64);

        // `notify`'s recommended watcher takes a synchronous handler; we
        // give it a closure that pushes events into a std mpsc channel
        // that the bridge below drains.
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

        // `spawn_blocking` parks the closure on a dedicated blocking
        // thread (`async-std`'s blocking pool) so `raw_rx.recv()` (which
        // blocks on the std mpsc) doesn't tie up the async-std worker
        // pool. `tx.blocking_send` is OK because we're on that blocking
        // thread, not a green-thread.
        async_std::task::spawn_blocking(move || {
            while let Ok(res) = raw_rx.recv() {
                match res {
                    Ok(event) => {
                        for p in event.paths {
                            if tx.blocking_send(p).is_err() {
                                // Receiver (FileWatcher) dropped; bail.
                                return;
                            }
                        }
                    }
                    Err(_) => {
                        // notify errors are typically transient races
                        // (e.g. file removed mid-watch). Silently drop so
                        // the bridge stays alive for subsequent events.
                    }
                }
            }
            // raw_rx disconnected -> notify watcher dropped -> no more
            // events incoming. Bridge exit.
        });

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }

    /// Await the next path-change event, or `None` if the watcher has
    /// been dropped (the async-std channel closed).
    pub async fn next_event(&self) -> Option<PathBuf> {
        self.rx.recv().await
    }
}

// `RecommendedWatcher` is inotify-backed on Linux; on macOS FSEvents; on
// Windows ReadDirectoryChangesW. The bridge above is platform-agnostic
// because it operates entirely above notify's runtime level.
