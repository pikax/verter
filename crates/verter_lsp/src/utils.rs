//! Shared utility functions used across LSP features.

/// Check if a byte is part of a JavaScript/TypeScript identifier.
#[inline]
pub fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Check if a byte is part of an identifier in HTML/template context.
/// Includes `-` for kebab-case HTML element and attribute names.
#[inline]
pub fn is_html_ident_byte(b: u8) -> bool {
    is_ident_byte(b) || b == b'-'
}

/// Extract the word at a byte offset in a source string.
///
/// Returns the word as a `String` if the offset points to an identifier character.
pub fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() || !is_ident_byte(bytes[offset]) {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

/// Extract the word at a byte offset, including hyphens for HTML/template context.
///
/// Useful for kebab-case component names and attributes like `my-component`.
pub fn word_at_offset_html(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() || !is_html_ident_byte(bytes[offset]) {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_html_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < bytes.len() && is_html_ident_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

/// Find the start position of the word at a given offset.
pub fn find_word_start(bytes: &[u8], offset: usize) -> usize {
    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Find all whole-word occurrences of `word` in `content`.
///
/// Returns byte offsets of each match start position, using word boundary checks.
pub fn find_all_word_occurrences(content: &str, word: &str) -> Vec<usize> {
    let mut results = Vec::new();
    let bytes = content.as_bytes();
    let word_len = word.len();

    let mut start = 0;
    while let Some(offset) = content[start..].find(word) {
        let abs = start + offset;
        let after = abs + word_len;

        let before_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);

        if before_ok && after_ok {
            results.push(abs);
        }

        start = abs + 1;
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_at_offset_basic() {
        assert_eq!(word_at_offset("hello world", 0), Some("hello".to_string()));
        assert_eq!(word_at_offset("hello world", 6), Some("world".to_string()));
        assert_eq!(word_at_offset("hello world", 5), None); // space
    }

    #[test]
    fn test_word_at_offset_identifiers() {
        assert_eq!(word_at_offset("foo_bar", 0), Some("foo_bar".to_string()));
        assert_eq!(word_at_offset("$ref", 0), Some("$ref".to_string()));
    }

    #[test]
    fn test_word_at_offset_html() {
        assert_eq!(
            word_at_offset_html("my-component", 0),
            Some("my-component".to_string())
        );
        assert_eq!(
            word_at_offset_html("my-component", 5),
            Some("my-component".to_string())
        );
    }

    #[test]
    fn test_find_all_word_occurrences() {
        let occurrences = find_all_word_occurrences("foo bar foo baz foo", "foo");
        assert_eq!(occurrences, vec![0, 8, 16]);
    }

    #[test]
    fn test_find_all_word_occurrences_no_partial() {
        let occurrences = find_all_word_occurrences("foobar foo barfoo", "foo");
        assert_eq!(occurrences, vec![7]);
    }
}
