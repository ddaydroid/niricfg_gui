//! Plugin implementations for specific tools / compositors. Each submodule
//! owns a concrete `ToolPlugin` + `KdlBackedTool` impl. The first and primary
//! plugin is `niri` (the Wayland compositor this GUI was built for), backed
//! by the `niri_validator` that spawns `niri msg validate` via async-process.

pub mod niri;
pub mod niri_validator;

#[cfg(feature = "gtk")]
pub mod niri_shell;
