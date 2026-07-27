//! Simple line-based diff algorithm (LCS-based). Pure Rust, no GTK
//! dependency — usable from both `--no-default-features` and
//! `--features gtk` builds.//!
//! # Example
//!
//! ```ignore
//! use dotcfg_gui::core::diff::line_diff;
//! let ops = line_diff("a\nb\nc", "a\nd\nc");
//! // ops contains Same(a), Removed(b), Added(d), Same(c) in some order
//! ```
//!
//! The `ignore` attribute keeps this from running as a doctest because
//! the LCS backtrack order is implementation-defined when multiple
//! equal-cost paths exist.

/// A single line in the diff output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// Line present in both original and modified (unchanged).
    Same(String),
    /// Line present only in the modified text (insertion).
    Added(String),
    /// Line present only in the original text (deletion).
    Removed(String),
}

/// Compute a simple LCS-based line diff between `original` and `modified`.
///
/// The algorithm:
/// 1. Split both inputs into lines (preserving empty trailing line semantics).
/// 2. Build an LCS length table via dynamic programming (O(mn)).
/// 3. Backtrack through the table to produce operations in left-to-right
///    order of the *original*.
///
/// This is intentionally **not** a Myers diff — the O(mn) space cost is
/// acceptable for config files (typically <10k lines) and the simpler
/// implementation is easier to verify.
pub fn line_diff(original: &str, modified: &str) -> Vec<DiffLine> {
    let orig_lines: Vec<&str> = original.lines().collect();
    let mod_lines: Vec<&str> = modified.lines().collect();

    let m = orig_lines.len();
    let n = mod_lines.len();

    // Build LCS length table (DP).
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if orig_lines[i - 1] == mod_lines[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }

    // Backtrack to build the diff.
    let mut result: Vec<DiffLine> = Vec::new();
    let mut i = m;
    let mut j = n;

    // Temporary buffers for the reverse path; we'll reverse at the end.
    let mut rev: Vec<DiffLine> = Vec::new();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && orig_lines[i - 1] == mod_lines[j - 1] {
            rev.push(DiffLine::Same(orig_lines[i - 1].to_string()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            rev.push(DiffLine::Added(mod_lines[j - 1].to_string()));
            j -= 1;
        } else if i > 0 {
            rev.push(DiffLine::Removed(orig_lines[i - 1].to_string()));
            i -= 1;
        }
    }

    rev.reverse();
    result.extend(rev);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_produce_no_changes() {
        let text = "line1\nline2\nline3";
        let ops = line_diff(text, text);
        assert!(ops.iter().all(|op| matches!(op, DiffLine::Same(_))));
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn single_addition() {
        let orig = "a\nb";
        let modd = "a\nb\nc";
        let ops = line_diff(orig, modd);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0], DiffLine::Same("a".into()));
        assert_eq!(ops[1], DiffLine::Same("b".into()));
        assert_eq!(ops[2], DiffLine::Added("c".into()));
    }

    #[test]
    fn single_deletion() {
        let orig = "a\nb\nc";
        let modd = "a\nc";
        let ops = line_diff(orig, modd);
        assert_eq!(ops.len(), 3);
        assert!(ops.contains(&DiffLine::Removed("b".into())));
    }

    #[test]
    fn empty_original() {
        let ops = line_diff("", "a\nb");
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().all(|op| matches!(op, DiffLine::Added(_))));
    }

    #[test]
    fn empty_modified() {
        let ops = line_diff("a\nb", "");
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().all(|op| matches!(op, DiffLine::Removed(_))));
    }

    #[test]
    fn both_empty() {
        let ops = line_diff("", "");
        assert_eq!(ops.len(), 0);
    }

    #[test]
    fn modification_detected() {
        let orig = "a\nold\nc";
        let modd = "a\nnew\nc";
        let ops = line_diff(orig, modd);
        // Expect: Same(a), Removed(old), Added(new), Same(c) — order varies
        // by DP backtrack but should contain all four tokens.
        assert_eq!(ops.len(), 4);
        assert!(ops.contains(&DiffLine::Same("a".into())));
        assert!(ops.contains(&DiffLine::Removed("old".into())));
        assert!(ops.contains(&DiffLine::Added("new".into())));
        assert!(ops.contains(&DiffLine::Same("c".into())));
    }
}
