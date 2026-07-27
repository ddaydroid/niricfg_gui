//! Plugin implementations for specific tools / compositors. Each submodule
//! owns a concrete `ToolPlugin` + `KdlBackedTool` impl. The first and primary
//! plugin is `niri` (the Wayland compositor this GUI was built for).

pub mod niri;
