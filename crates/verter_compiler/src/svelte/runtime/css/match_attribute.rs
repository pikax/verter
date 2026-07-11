//! The official `test_attribute` operator semantics plus the JS
//! string/number helpers it depends on (the `\s` whitespace class, the
//! `/^\s/` / `/\s$/` edge tests, the backslash unescape, and `unquote`).
//! Pure functions over decoded attribute values.

use super::{synthetic_span, MatchResult, MatcherRefusal};

/// The official `test_attribute(operator, expected_value, case_insensitive,
/// value)`.
pub(super) fn test_attribute(
    operator: &str,
    expected_value: &str,
    case_insensitive: bool,
    value: &str,
) -> MatchResult<bool> {
    let (expected, value) = if case_insensitive {
        (expected_value.to_lowercase(), value.to_lowercase())
    } else {
        (expected_value.to_string(), value.to_string())
    };
    match operator {
        "=" => Ok(value == expected),
        // `value.split(/\s/).includes(expected)` — split on EVERY single JS
        // whitespace char (empty pieces included).
        "~=" => Ok(value.split(is_js_whitespace).any(|part| part == expected)),
        "|=" => Ok(format!("{value}-").starts_with(&format!("{expected}-"))),
        "^=" => Ok(value.starts_with(&expected)),
        "$=" => Ok(value.ends_with(&expected)),
        "*=" => Ok(value.contains(&expected)),
        // The parser only produces the six operators; anything else is
        // unprovable rather than the official throw.
        _ => Err(MatcherRefusal::at(
            synthetic_span(),
            "an unknown attribute matcher operator",
        )),
    }
}

/// The JS `\s` regex class (NOT Rust `char::is_whitespace`: JS includes
/// U+FEFF and excludes U+0085).
pub(super) fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `regex_starts_with_whitespace` (`/^\s/`).
pub(super) fn starts_with_js_whitespace(s: &str) -> bool {
    s.chars().next().is_some_and(is_js_whitespace)
}

/// `regex_ends_with_whitespace` (`/\s$/`).
pub(super) fn ends_with_js_whitespace(s: &str) -> bool {
    s.chars().last().is_some_and(is_js_whitespace)
}

/// The official `regex_backslash_and_following_character` unescape
/// (`name.replace(/\\(.)/g, '$1')` — the dot does NOT match line
/// terminators, exactly as the JS regex).
pub(super) fn unescape_backslashes(name: &str) -> String {
    if !name.contains('\\') {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&next) if !matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                    out.push(next);
                    chars.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The official `unquote(str)` (the parser already strips quote marks; this
/// stays for exactness on any residual quoted payload).
pub(super) fn unquote(s: &str) -> &str {
    let chars: Vec<char> = s.chars().collect();
    let Some(&first) = chars.first() else {
        return s;
    };
    let last = *chars.last().expect("non-empty checked via first");
    if (first == last && first == '\'') || first == '"' {
        let mut iter = s.char_indices();
        let Some((_, first_char)) = iter.next() else {
            return s;
        };
        let start = first_char.len_utf8();
        let end = s.len() - last.len_utf8();
        if start <= end {
            return &s[start..end];
        }
    }
    s
}
