//! Loads a KDL text into the untyped `ConfigDoc` representation.
//!
//! Wave 1 Step 2 of `.specs/tasks/todo/implement-dotcfg-gui.feature.md`.
//! Wraps `kdl::KdlDocument::from_str` for the real production parse path
//! (the same parser the cargo-fuzz harness at `fuzz/fuzz_targets/kdl_parser.rs`
//! has been exercising in shadow since the prior commits). Parse failures map
//! to `Error::Kdl` via the existing `#[from] kdl::KdlError` impl; syntactic
//! layer alone is exercised here — semantic validation (drift, deprecated
//! fields) is delegated to `ToolPlugin::validate`, not this function.

use std::str::FromStr;

use kdl::KdlDocument;

use crate::core::error::Error;

/// A zero-cost type alias over `kdl::KdlDocument`.
///
/// YAGNI for Wave 1: a type alias carries no API commitment, so callers may
/// pass `ConfigDoc` around by reference. In Wave 2 this can be promoted to a
/// newtype (`pub struct ConfigDoc(pub KdlDocument)`) carrying derived accessors
/// (typed getters, normalization, validation helpers) without breaking the
/// public signature of `load_config` for callers that just want `&ConfigDoc`.
pub type ConfigDoc = KdlDocument;

/// Parse a KDL text into a [`ConfigDoc`].
///
/// Returns `Err(Error::Kdl(..))` if the input is syntactically malformed
/// (unbalanced braces, missing terminators, invalid escapes). Empty input
/// (`""`) is accepted — `kdl::KdlDocument::from_str("")` returns an empty
/// document, which we surface to callers as `Ok(empty_config)`. Semantic
/// validation is not performed here; see `ToolPlugin::validate` for that.
pub fn load_config(text: &str) -> Result<ConfigDoc, Error> {
    let doc = KdlDocument::from_str(text)?;
    Ok(doc)
}
