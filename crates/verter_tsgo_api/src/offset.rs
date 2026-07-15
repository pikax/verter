//! The tsgo `--api` diagnostic offset WIRE CONTRACT.
//!
//! Every diagnostic the `--api` surface returns carries its `pos`/`end` as
//! **UTF-16 code-unit** offsets into the containing file — TypeScript position
//! semantics (`getLineAndCharacterOfPosition` / the `LanguageService`), which
//! the Go engine preserves on this surface. This is the wire contract: the raw
//! [`Diagnostic::pos`](crate::proto::types::Diagnostic) / `end` are NOT UTF-8
//! byte offsets.
//!
//! This module is the single boundary that TURNS that contract into the two
//! coordinate forms consumers need. It owns the *contract* (that the offsets are
//! UTF-16) and DELEGATES the numeric conversion to `verter_span` — the leaf
//! coordinate owner — so there is exactly ONE UTF-16-offset conversion
//! implementation across the workspace, not one per consuming crate.
//!
//! A consumer must never read `d.pos` / `d.end` directly into a byte-offset or a
//! `(line, col)` field: it drifts on any non-ASCII content before the diagnostic
//! (e.g. an em-dash `—` — 3 UTF-8 bytes / 1 UTF-16 unit — in generated-carrier
//! comments). Route through [`api_offset_to_byte`] / [`api_offset_to_line_col`].

use crate::proto::types::Diagnostic;

/// Convert an `--api` diagnostic UTF-16 code-unit `offset` into `content` to a
/// **byte offset** into the same content (the byte contract Verter's span /
/// diagnostic carriers use). Delegates to the shared `verter_span` primitive.
///
/// Use this at any boundary that stores the offset as a byte position (e.g. the
/// `TypeDiagnostic.start`/`end` byte contract).
#[must_use]
pub fn api_offset_to_byte(content: &str, offset: u32) -> u32 {
    verter_span::utf16_offset_to_byte(content, offset)
}

/// Convert an `--api` diagnostic UTF-16 code-unit `offset` into `content` to a
/// 1-based `(line, col)` position, with `col` also in UTF-16 code units (the
/// coordinate inline source maps are keyed in). Delegates to the shared
/// `verter_span` primitive.
#[must_use]
pub fn api_offset_to_line_col(content: &str, offset: u32) -> (u32, u32) {
    verter_span::utf16_offset_to_line_col(content, offset)
}

/// A diagnostic's `pos`/`end` converted to **byte offsets** into `content`.
///
/// Convenience over [`api_offset_to_byte`] for the common case of normalizing a
/// whole diagnostic's span at once.
#[must_use]
pub fn diagnostic_byte_span(d: &Diagnostic, content: &str) -> (u32, u32) {
    (
        api_offset_to_byte(content, d.pos),
        api_offset_to_byte(content, d.end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(pos: u32, end: u32) -> Diagnostic {
        Diagnostic {
            code: 2322,
            category: 1,
            text: "x".to_string(),
            pos,
            end,
            file_name: Some("/f.ts".to_string()),
        }
    }

    #[test]
    fn byte_conversion_matches_shared_primitive_and_is_utf16_aware() {
        // "a—b": UTF-16 units a(0) —(1) b(2); bytes a(0) —(1..4) b(4).
        let s = "a\u{2014}b";
        // The contract helper agrees with the shared verter_span primitive (single impl).
        assert_eq!(
            api_offset_to_byte(s, 2),
            verter_span::utf16_offset_to_byte(s, 2)
        );
        // And it is UTF-16-aware: offset 2 ('b') → byte 4, NOT a passthrough 2.
        assert_eq!(api_offset_to_byte(s, 2), 4);
    }

    #[test]
    fn line_col_conversion_matches_shared_primitive() {
        let s = "a\u{2014}b\ncd";
        assert_eq!(
            api_offset_to_line_col(s, 4),
            verter_span::utf16_offset_to_line_col(s, 4)
        );
        assert_eq!(api_offset_to_line_col(s, 4), (2, 1));
    }

    #[test]
    fn diagnostic_byte_span_converts_both_endpoints() {
        // "a—bc": units a(0) —(1) b(2) c(3); bytes a(0) —(1..4) b(4) c(5).
        let s = "a\u{2014}bc";
        let d = diag(2, 3); // pos at 'b' (byte 4), end at 'c' (byte 5)
        assert_eq!(diagnostic_byte_span(&d, s), (4, 5));
    }
}
