//! GUI shell: GTK4 + libadwaita `Application` / `ApplicationWindow`. The
//! entire file is `#[cfg(feature = "gtk")]` so it contributes zero to a
//! no-default-features build (which is what the unit-test CI job runs).

#![cfg(feature = "gtk")]

use crate::core::error::Error;
use crate::core::DynTool;

/// Launch the dotcfg-gui GUI shell.
///
/// `plugins` is the registry of `Box<dyn ToolPlugin>` built by `main`.
/// Step 4 will populate it from build-time Cargo features; callers today
/// pass `vec![]`.
///
/// The body is a stub for Wave 0 Step 2 — the real `Adw.ApplicationWindow`
/// shell wiring lands in Step 6.
pub fn run_shell(plugins: Vec<DynTool>) -> Result<(), Error> {
    let _ = plugins;
    Ok(())
}
