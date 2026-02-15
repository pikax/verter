//! Character classification, skip helpers, and trim utilities for CSS byte-level scanning.

// =============================================================================
// Character classification
// =============================================================================

/// Returns `true` if `b` is ASCII whitespace (space, tab, newline, carriage return).
#[inline]
pub fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

/// Returns `true` if `b` is a valid CSS identifier character (alphanumeric, `-`, `_`).
#[inline]
pub fn is_css_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Returns `true` if `b` is a valid CSS identifier start character (alpha, `_`, `-`).
#[inline]
pub fn is_css_ident_start_char(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'-'
}

/// Returns `true` if `b` can start a CSS selector (element, class, id, pseudo, attr, universal, `&`).
#[inline]
pub fn is_selector_start_char(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || b == b'.'
        || b == b'#'
        || b == b':'
        || b == b'['
        || b == b'*'
        || b == b'&'
}

// =============================================================================
// Skip helpers
// =============================================================================

/// Skip a block comment `/* ... */`. Returns position after `*/`.
pub fn skip_block_comment(css: &[u8], start: usize) -> usize {
    let mut i = start + 2; // skip `/*`
    while i + 1 < css.len() {
        if css[i] == b'*' && css[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    css.len()
}

/// Skip a line comment `// ... \n`. Returns position after `\n`.
pub fn skip_line_comment(css: &[u8], start: usize) -> usize {
    let mut i = start + 2; // skip `//`
    while i < css.len() && css[i] != b'\n' {
        i += 1;
    }
    if i < css.len() {
        i + 1 // skip '\n'
    } else {
        css.len()
    }
}

/// Skip a quoted string (single or double). Returns position after closing quote.
/// Handles escape sequences.
pub fn skip_string(css: &[u8], start: usize) -> usize {
    let quote = css[start];
    let mut i = start + 1;
    while i < css.len() {
        if css[i] == b'\\' && i + 1 < css.len() {
            i += 2; // skip escaped char
            continue;
        }
        if css[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    css.len()
}

/// Skip a parenthesized group `(...)`. Returns position after closing `)`.
/// Handles nested parentheses and strings.
pub fn skip_parens(css: &[u8], start: usize) -> usize {
    let mut depth = 1u32;
    let mut i = start + 1;
    while i < css.len() && depth > 0 {
        match css[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'"' | b'\'' => {
                i = skip_string(css, i);
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            i += 1;
        }
    }
    if i < css.len() {
        i + 1
    } else {
        css.len()
    }
}

/// Skip a bracketed group `[...]`. Returns position after closing `]`.
/// Handles escape sequences.
pub fn skip_brackets(css: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < css.len() {
        if css[i] == b'\\' && i + 1 < css.len() {
            i += 2;
            continue;
        }
        if css[i] == b']' {
            return i + 1;
        }
        i += 1;
    }
    css.len()
}

// =============================================================================
// Trim helpers
// =============================================================================

/// Trim leading and trailing whitespace from a byte slice.
pub fn trim_bytes(bytes: &[u8]) -> &[u8] {
    let start = ltrim_pos(bytes);
    let end = rtrim_pos(bytes);
    if start < end {
        &bytes[start..end]
    } else {
        &[]
    }
}

/// Position of the first non-whitespace byte (or `bytes.len()` if all whitespace).
pub fn ltrim_pos(bytes: &[u8]) -> usize {
    bytes.iter().position(|&b| !is_ws(b)).unwrap_or(bytes.len())
}

/// Position one past the last non-whitespace byte (or `0` if all whitespace).
pub fn rtrim_pos(bytes: &[u8]) -> usize {
    bytes.iter().rposition(|&b| !is_ws(b)).map_or(0, |e| e + 1)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Character classification ---

    #[test]
    fn test_is_ws() {
        assert!(is_ws(b' '));
        assert!(is_ws(b'\t'));
        assert!(is_ws(b'\n'));
        assert!(is_ws(b'\r'));
        assert!(!is_ws(b'a'));
        assert!(!is_ws(b'.'));
    }

    #[test]
    fn test_is_css_ident_char() {
        assert!(is_css_ident_char(b'a'));
        assert!(is_css_ident_char(b'Z'));
        assert!(is_css_ident_char(b'0'));
        assert!(is_css_ident_char(b'-'));
        assert!(is_css_ident_char(b'_'));
        assert!(!is_css_ident_char(b'.'));
        assert!(!is_css_ident_char(b' '));
    }

    #[test]
    fn test_is_selector_start_char() {
        assert!(is_selector_start_char(b'.'));
        assert!(is_selector_start_char(b'#'));
        assert!(is_selector_start_char(b':'));
        assert!(is_selector_start_char(b'['));
        assert!(is_selector_start_char(b'*'));
        assert!(is_selector_start_char(b'&'));
        assert!(is_selector_start_char(b'a'));
        assert!(!is_selector_start_char(b' '));
        assert!(!is_selector_start_char(b'{'));
    }

    // --- Skip helpers ---

    #[test]
    fn test_skip_block_comment() {
        assert_eq!(skip_block_comment(b"/* comment */rest", 0), 13);
        assert_eq!(skip_block_comment(b"/* unclosed", 0), 11);
        assert_eq!(skip_block_comment(b"/**/", 0), 4);
    }

    #[test]
    fn test_skip_line_comment() {
        assert_eq!(skip_line_comment(b"// comment\nrest", 0), 11);
        assert_eq!(skip_line_comment(b"// no newline", 0), 13);
    }

    #[test]
    fn test_skip_string() {
        assert_eq!(skip_string(b"'hello'rest", 0), 7);
        assert_eq!(skip_string(b"\"hello\"rest", 0), 7);
        assert_eq!(skip_string(b"'esc\\'aped'rest", 0), 11);
        assert_eq!(skip_string(b"'unclosed", 0), 9);
    }

    #[test]
    fn test_skip_parens() {
        assert_eq!(skip_parens(b"(inner)rest", 0), 7);
        assert_eq!(skip_parens(b"(a(b)c)rest", 0), 7);
        assert_eq!(skip_parens(b"('str')rest", 0), 7);
        assert_eq!(skip_parens(b"(unclosed", 0), 9);
    }

    #[test]
    fn test_skip_brackets() {
        assert_eq!(skip_brackets(b"[attr]rest", 0), 6);
        assert_eq!(skip_brackets(b"[a\\]b]rest", 0), 6);
        assert_eq!(skip_brackets(b"[unclosed", 0), 9);
    }

    // --- Trim helpers ---

    #[test]
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hello  "), b"hello");
        assert_eq!(trim_bytes(b"hello"), b"hello");
        assert_eq!(trim_bytes(b"   "), b"" as &[u8]);
        assert_eq!(trim_bytes(b""), b"" as &[u8]);
    }

    #[test]
    fn test_ltrim_pos() {
        assert_eq!(ltrim_pos(b"  hello"), 2);
        assert_eq!(ltrim_pos(b"hello"), 0);
        assert_eq!(ltrim_pos(b"   "), 3);
    }

    #[test]
    fn test_rtrim_pos() {
        assert_eq!(rtrim_pos(b"hello  "), 5);
        assert_eq!(rtrim_pos(b"hello"), 5);
        assert_eq!(rtrim_pos(b"   "), 0);
    }
}
