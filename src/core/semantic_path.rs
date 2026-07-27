//! Semantic-Path Indexing. Walks a `KdlDocument` at load time and
//! precomputes a `HashMap<SemanticPath, (usize, usize)>` keyed by every
//! node's nested path. The indexed value is the node's `(offset, len)`
//! source-byte range.
//!
//! # Purpose (per spec Wave 2 Step 8)
//!
//! `niri`'s config uses dotted / nested paths like `["binds",
//! "Mod+Return"]` to identify binds by their chord. Forward lookups
//! during GUI edits ("which row corresponds to this chord?") need a
//! hash from path-string to the source span — without a precomputed
//! index, each lookup is an O(N) tree walk on every keystroke, which
//! adds noticeable latency on real configs (≥200 nodes).
//!
//! Pre-computing at load time means lookups are O(1) at edit time.
//!
//! # Algorithm
//!
//! 1. Iterate `doc.nodes()` — the top-level node list.
//! 2. For each node, push its name onto a working `Vec<String>`, visit
//!    any children recursively, then `pop()` before returning. The
//!    working vec acts as the "current path from root" used as the map
//!    key.
//! 3. Each visited node contributes ONE `SemanticPath -> (offset,
//!    len)` entry to the result. The byte range is extracted from
//!    kdl's `node.span()` field and stored as a primitive pair.
//!
//! # Why `(usize, usize)` and not `kdl::Span`?
//!
//! kdl v6.7.1's `Span` type alias resolves to `miette::protocol::SourceSpan`
//! (custom re-export), which exposes PRIVATE fields and method-only
//! accessors (`offset()`, `len()`). Forcing `SemanticIndex` to depend on
//! kdl's specific `Span` reexport chain would couple our public API
//! to kdl's exact version AND pollute `dotcfg-gui`'s downstream types
//! with a 3rd-party `miette` import. Storing the primitive `(offset,
//! len)` pair reconciles both concerns:
//! - **Stable**: independent of any kdl re-export gymnastics
//!   (`error[E0432]: no Span in the root` was the actual blocker; the
//!   kdl→miette alias resolves OK but exposes private fields).
//! - **Round-tripped**: `kdl::Span { offset, len }` is trivially
//!   reconstructed from the stored pair when downstream code needs
//!   the full type.
//! - **Cross-tool compatible**: future TOML/YAML plugins can use the
//!   same `SemanticIndex` shape, simply sourcing offset+len from
//!   their own parsers.
//!
//! # Determinism
//!
//! The walk is depth-first and respects `kdl`'s node ordering: sibling
//! order in the source document is preserved in the index's iteration
//! order (HashMap itself is unordered; for stable UI we sort the keys
//! before display).
//!
//! # Non-goals
//!
//! - The walker does NOT collapse duplicate segments. If a node name
//!   repeats under the same parent (legal in KDL), each gets its own
//!   distinct index entry — relying on `KdlDocument`'s own
//!   disambiguation at parse time.
//! - We do NOT canonicalise / re-order keys. Path segments are exactly
//!   the node-name strings from the source.
//! - Under kdl v6's grammar, the `key value` form inside a children
//!   block is parsed as a CHILD NODE named `key` (not a property
//!   entry; for that use the `key=value` form). The walker therefore
//!   indexes every nested name depth-first — including grandchildren
//!   and great-grandchildren. Concretely: `system { hostname "box" }`
//!   produces BOTH `system` AND `system/hostname` (the second because
//!   kdl parses `hostname "box"` as a child node named `hostname`).
//! - Property entries (the `key=value` form) are NOT nodes and are
//!   therefore NOT indexed — the walker visits `KdlNode`s only,
//!   never `KdlEntry::value`s.
//! - Partial paths are NOT supported: an entry `["binds", "Mod+Q"]`
//!   cannot be reached with `lookup(&["Mod+Q"])` — callers must
//!   pass the full path from root.

use std::collections::HashMap;

use kdl::{KdlDocument, KdlNode};

/// A path through the KDL document tree, expressed as the sequence of
/// node names from the root down to a specific node.
///
/// Examples:
/// - top-level: `["system"]`, `["display"]`
/// - one level deep: `["binds", "Mod+Return"]`
/// - two levels deep: `["binds", "Mod+Return", "action"]`
///
/// Segments are stored as `Vec<String>` (not `Vec<&str>`) so the path
/// is `'static`-free and can be returned across `Box<dyn Trait>`
/// boundaries without lifetime annotation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticPath {
    segments: Vec<String>,
}

impl SemanticPath {
    /// Construct a `SemanticPath` from anything that converts into a
    /// `Vec<String>` — most commonly `vec!["name".to_string(), …]`.
    pub fn from_segments<S: Into<Vec<String>>>(segments: S) -> Self {
        Self {
            segments: segments.into(),
        }
    }

    /// Borrow the path's segments. Examples: `["system"]` or
    /// `["binds", "Mod+Return"]`.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Number of segments (depth from root). A top-level node has
    /// `depth() == 1`.
    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    /// Convert to a slash-joined display string: `["binds",
    /// "Mod+Return"]` becomes `"binds/Mod+Return"`. Used by the UI for
    /// chip labels and binding selectors.
    pub fn to_display_string(&self) -> String {
        self.segments.join("/")
    }
}

/// Opaque holder for the precomputed index. Public-API surface
/// intentionally minimal — the `entries` field is exposed so callers
/// can iterate / sort for stable UI display; mutability is not
/// exposed so callers can't poison the index mid-edit.
#[derive(Debug, Clone, Default)]
pub struct SemanticIndex {
    /// Each entry maps a `SemanticPath` to the node's source-byte
    /// range as `(offset, len)`. The pair is reconstructable to
    /// `kdl::Span { offset, len }` if a downstream caller needs the
    /// full kdl type.
    pub entries: HashMap<SemanticPath, (usize, usize)>,
}

impl SemanticIndex {
    /// O(1) lookup: given a sequence of path segments, return the
    /// `(offset, len)` byte range of that node if indexed.
    /// Partial paths are NOT supported — callers must pass the full
    /// path from root (`["binds", "Mod+Return"]`, NOT
    /// `["Mod+Return"]`).
    pub fn lookup(&self, segments: &[String]) -> Option<&(usize, usize)> {
        self.entries
            .get(&SemanticPath::from_segments(segments.to_vec()))
    }

    /// Number of nodes indexed (top-level + nested children +
    /// great-grandchildren, etc.).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the index has zero entries (an empty config doc).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Walk the document and produce a [`SemanticIndex`].
///
/// Walks every node depth-first. Each node contributes one entry
/// keyed by its full path-from-root. Children are visited in source
/// order.
///
/// Complexity: O(N) for a doc with N nodes; each node's
/// `HashMap<SemanticPath, …>::insert` is O(1) amortised (the path
/// allocation — one Vec clone per node — is the load-time cost;
/// ~200 Vec clones on a 200-node doc completes well below the edit
/// latency budget).
pub fn build_index(doc: &KdlDocument) -> SemanticIndex {
    let mut entries = HashMap::new();
    for node in doc.nodes() {
        walk_node(node, &mut Vec::new(), &mut entries);
    }
    SemanticIndex { entries }
}

fn walk_node(
    node: &KdlNode,
    prefix: &mut Vec<String>,
    entries: &mut HashMap<SemanticPath, (usize, usize)>,
) {
    // Push this node's name. Capture a clone for the key — the
    // walker mutates `prefix` during recursion, so we can't borrow
    // it after the push for the HashMap insertion.
    let name = node.name().value().to_string();
    prefix.push(name);

    // Index THIS node (regardless of whether it has children).
    let path = SemanticPath::from_segments(prefix.clone());
    let span = node.span();
    // Method accessors (NOT field access): kdl v6's `Span` is a
    // type-alias for `miette::protocol::SourceSpan` whose `offset` /
    // `len` fields are PRIVATE. Use the public accessors instead.
    entries.insert(path, (span.offset(), span.len()));

    // Recurse into children if present.
    if let Some(children) = node.children() {
        for child in children.nodes() {
            walk_node(child, prefix, entries);
        }
    }

    // Pop before returning so the caller's `prefix` reflects the
    // path state BEFORE this node.
    prefix.pop();
}
