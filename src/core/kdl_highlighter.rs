//! KDL syntax highlighting via GtkTextTag. Entire file is gated behind
//! `#[cfg(feature = "gtk")]` so it contributes zero to
//! `--no-default-features` builds.
//!
//! # Highlighting scheme
//!
//! | Token        | Color      | Weight / Style | Example             |
//! |--------------|------------|----------------|---------------------|
//! | Comment      | `#6a9955`  | Italic         | `// tiling`         |
//! | Keyword      | `#569cd6`  | Bold           | `binds`, `output`   |
//! | String       | `#ce9178`  | Normal         | `"alacritty"`       |
//! | Number       | `#b5cea8`  | Normal         | `42`, `3.14`        |
//! | Punctuation  | `#808080`  | Normal         | `{`, `}`            |

#![cfg(feature = "gtk")]

use gtk4::pango;
use gtk4::TextBuffer;

// ---- Token recogniser ----

/// A span of text with an associated token kind.
#[derive(Debug, Clone)]
struct Token {
    start: usize,
    end: usize,
    kind: TokenKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Comment,
    Keyword,
    String,
    Number,
    Punctuation,
}

/// KDL keywords that get special highlighting.
const KDL_KEYWORDS: &[&str] = &[
    "binds",
    "output",
    "input",
    "environment",
    "window-rules",
    "layout",
    "spawn-at-startup",
    "spawn",
    "focus-ring",
    "cursor",
    "hotkey-overlay",
];

/// Scan `text` and return highlightable token spans.
///
/// Uses a simple char-by-char state machine. Non-highlighted spans
/// (bare identifiers, whitespace) are simply omitted from the result.
fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut tokens: Vec<Token> = Vec::new();

    while i < len {
        // ---- Line comment ----
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            let start = i;
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            tokens.push(Token {
                start,
                end: i,
                kind: TokenKind::Comment,
            });
            continue;
        }

        // ---- Block comment (/* */) ---
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
            tokens.push(Token {
                start,
                end: i,
                kind: TokenKind::Comment,
            });
            continue;
        }

        // ---- String ----
        if chars[i] == '"' {
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push(Token {
                start,
                end: i,
                kind: TokenKind::String,
            });
            continue;
        }

        // ---- Number ----
        if chars[i].is_ascii_digit()
            || (chars[i] == '-' && i + 1 < len && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            if chars[i] == '-' {
                i += 1;
            }
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            tokens.push(Token {
                start,
                end: i,
                kind: TokenKind::Number,
            });
            continue;
        }

        // ---- Punctuation ----
        if matches!(chars[i], '{' | '}' | '(' | ')') {
            tokens.push(Token {
                start: i,
                end: i + 1,
                kind: TokenKind::Punctuation,
            });
            i += 1;
            continue;
        }

        // ---- Word boundary ----
        if chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_' || chars[i] == '.' {
            let start = i;
            while i < len
                && (chars[i].is_alphanumeric()
                    || chars[i] == '-'
                    || chars[i] == '_'
                    || chars[i] == '.')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if KDL_KEYWORDS.contains(&word.as_str()) {
                tokens.push(Token {
                    start,
                    end: i,
                    kind: TokenKind::Keyword,
                });
            }
            continue;
        }

        i += 1;
    }

    tokens
}

// ---- Highlight tags holder ----

/// A set of GtkTextTag instances registered on a specific buffer.
struct HighlightTags {
    comment: gtk4::TextTag,
    keyword: gtk4::TextTag,
    string: gtk4::TextTag,
    number: gtk4::TextTag,
    punctuation: gtk4::TextTag,
}

impl HighlightTags {
    fn new(buffer: &TextBuffer) -> Self {
        fn make_tag(buf: &TextBuffer, name: &str) -> gtk4::TextTag {
            buf.create_tag(Some(name))
        }

        let mut tags = Self {
            comment: make_tag(buffer, "kdl_comment"),
            keyword: make_tag(buffer, "kdl_keyword"),
            string: make_tag(buffer, "kdl_string"),
            number: make_tag(buffer, "kdl_number"),
            punctuation: make_tag(buffer, "kdl_punctuation"),
        };

        tags.comment.set_foreground(Some("#6a9955"));
        tags.comment.set_style(pango::Style::Italic);

        tags.keyword.set_foreground(Some("#569cd6"));
        tags.keyword.set_weight(pango::Weight::Bold);

        tags.string.set_foreground(Some("#ce9178"));

        tags.number.set_foreground(Some("#b5cea8"));

        tags.punctuation.set_foreground(Some("#808080"));

        tags
    }

    fn apply(&self, buffer: &TextBuffer) {
        let start = buffer.start_iter();
        let end = buffer.end_iter();
        buffer.remove_all_tags(&start, &end);

        let text = buffer.text(&start, &end, false).unwrap_or_default();
        let tokens = tokenize(&text);

        for tok in tokens {
            let s = buffer.iter_at_offset(tok.start as i32);
            let e = buffer.iter_at_offset(tok.end as i32);
            let tag = match tok.kind {
                TokenKind::Comment => &self.comment,
                TokenKind::Keyword => &self.keyword,
                TokenKind::String => &self.string,
                TokenKind::Number => &self.number,
                TokenKind::Punctuation => &self.punctuation,
            };
            buffer.apply_tag(tag, &s, &e);
        }
    }
}

/// Apply KDL syntax highlighting to a text buffer.
///
/// Creates GtkTextTag instances, registers them with the buffer's tag
/// table, and connects to the `::changed` signal so highlighting
/// follows edits.
pub fn apply_highlighting(buffer: &TextBuffer) {
    let tags = Rc::new(HighlightTags::new(buffer));
    let tags_c = tags.clone();

    buffer.connect_changed(move |buf| {
        tags_c.apply(buf);
    });

    tags.apply(buffer);
}

// Re-export Rc for the shell's use (we use it inside the module).
use std::rc::Rc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_empty_string() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_line_comment() {
        let tokens = tokenize("// this is a comment");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Comment);
    }

    #[test]
    fn tokenize_keyword_binds() {
        let tokens = tokenize("binds");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Keyword);
    }

    #[test]
    fn tokenize_string() {
        let tokens = tokenize("\"alacritty\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
    }

    #[test]
    fn tokenize_string_with_escape() {
        let tokens = tokenize("\"hello\\\"world\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
    }

    #[test]
    fn tokenize_number() {
        let tokens = tokenize("42");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Number);
    }

    #[test]
    fn tokenize_negative_number() {
        let tokens = tokenize("-17");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Number);
    }

    #[test]
    fn tokenize_float() {
        let tokens = tokenize("3.14");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Number);
    }

    #[test]
    fn tokenize_punctuation() {
        let tokens = tokenize("{ }");
        assert_eq!(tokens.len(), 2);
        assert!(tokens.iter().all(|t| t.kind == TokenKind::Punctuation));
    }

    #[test]
    fn tokenize_mixed() {
        let tokens = tokenize("binds Mod4 T spawn \"alacritty\"");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Keyword));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::String));
    }

    #[test]
    fn tokenize_unclosed_string() {
        // An unclosed string should still produce a single token.
        let tokens = tokenize("\"hello world");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
    }

    #[test]
    fn tokenize_unclosed_block_comment() {
        // Unterminated /* should still produce a comment token.
        let tokens = tokenize("/* oops no closing");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Comment);
    }
}
