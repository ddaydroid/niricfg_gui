//! In-process registry of active `ToolPlugin` instances.
//!
//! Wave 1 Step 4 of `.specs/tasks/todo/implement-dotcfg-gui.feature.md`.
//! Passive storage: `register` fails on duplicate id, `unregister` returns
//! the removed entry (or `None`), `iter` walks insertion order, `find_by_id`
//! does a linear scan for an exact id match, `is_plugin_compatible` answers
//! "is the tool with this id's `api_version()` >= the required version?".
//! No automatic unload on plugin error (delegated to Wave 2's `Shell`), no
//! hot-reload-from-file-watch (delegated to Wave 3's `load_config_into_registry`).

use crate::core::error::Error;
use crate::core::tool_plugin::DynTool;

/// In-process registry of active plugins, ordered by insertion.
pub struct ToolRegistry {
    tools: Vec<DynTool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a plugin. Errors with `Error::Plugin("Duplicate tool id: ...")`
    /// if a plugin with the same id is already registered (last-write-wins
    /// would silently shadow — duplicate detection is the safer default).
    pub fn register(&mut self, tool: DynTool) -> Result<(), Error> {
        let id = tool.id();
        if self.tools.iter().any(|t| t.id() == id) {
            return Err(Error::Plugin(format!("Duplicate tool id: {id}")));
        }
        self.tools.push(tool);
        Ok(())
    }

    /// Remove the plugin with the given id and return it, or `None` if no
    /// such plugin is registered.
    pub fn unregister(&mut self, tool_id: &str) -> Option<DynTool> {
        let pos = self.tools.iter().position(|t| t.id() == tool_id)?;
        Some(self.tools.remove(pos))
    }

    /// Slice of registered plugins in insertion order. Returns `&[DynTool]`
    /// so callers can use slice methods directly — `.len()`, `.first()`,
    /// `.is_empty()`, `.iter()` / `.into_iter()` yielding
    /// `Iterator<Item = &DynTool>`, or `for t in registry.as_slice() { ... }`.
    /// Method is named `as_slice` (not `iter`) so callers are not surprised
    /// that the return type is a slice rather than an `Iterator`.
    pub fn as_slice(&self) -> &[DynTool] {
        &self.tools
    }

    /// Find the plugin whose `id()` exactly matches `tool_id`, if any.
    /// Linear scan (O(N)); N expected to be small (< 20 plugins) so a hash
    /// indirection isn't justified in Wave 1.
    pub fn find_by_id(&self, tool_id: &str) -> Option<&DynTool> {
        self.tools.iter().find(|t| t.id() == tool_id)
    }

    /// Forward-compatibility gate: returns `true` iff a plugin with the given
    /// id is registered AND its `api_version()` is `>= api_version`. A
    /// missing id returns `false` (fail-safe: caller's requested plugin isn't
    /// loaded yet). Older plugins (api < requested) return `false` (rejected:
    /// they cannot implement newer trait methods the shell relies on).
    pub fn is_plugin_compatible(&self, tool_id: &str, api_version: u32) -> bool {
        self.find_by_id(tool_id)
            .map(|t| t.api_version() >= api_version)
            .unwrap_or(false)
    }
}

/// `Default::default()` is a zero-tool alias for `ToolRegistry::new()`. Implemented
/// manually (not via `#[derive(Default)]`) so callers see the canonical `new()`
/// as the only constructor and `Default` exists purely for trait interop.
/// Satisfies `clippy::new_without_default` without re-deriving `Default`,
/// preserving the earlier decision that the explicit `new()` is canonical.
impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
