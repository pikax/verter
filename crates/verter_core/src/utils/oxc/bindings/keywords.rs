//! JavaScript/TypeScript keyword detection with byte-level optimization.
//!
//! This module provides fast keyword detection using byte comparisons and
//! first-byte dispatch to minimize comparisons.

/// Bitmap of valid keyword lengths (bits 2-10 set)
/// Binary: 0b11111111100 = 0x7FC
const VALID_KEYWORD_LENS: u16 = 0b11111111100;

/// Check if a byte slice is a JavaScript keyword using fast byte comparison.
///
/// Uses first-byte dispatch to reduce comparisons. Keywords range from 2 to 10 characters.
///
/// # Example
/// ```ignore
/// assert!(is_keyword(b"true"));
/// assert!(is_keyword(b"const"));
/// assert!(!is_keyword(b"myVariable"));
/// ```
#[inline]
pub fn is_keyword(name: &[u8]) -> bool {
    let len = name.len();

    // Keywords range from 2 to 10 characters - use bitmap for O(1) check
    if len > 15 || (VALID_KEYWORD_LENS >> len) & 1 == 0 {
        return false;
    }

    // Get first byte for dispatch - this eliminates most non-keywords quickly
    let first = name[0];

    // Group keywords by length, then dispatch on first byte
    match len {
        2 => match first {
            b'd' => name == b"do",
            b'i' => name == b"if" || name == b"in",
            _ => false,
        },
        3 => match first {
            b'f' => name == b"for",
            b'l' => name == b"let",
            b'n' => name == b"new",
            b't' => name == b"try",
            b'v' => name == b"var",
            _ => false,
        },
        4 => match first {
            b'c' => name == b"case",
            b'e' => name == b"else" || name == b"enum",
            b'n' => name == b"null",
            b't' => name == b"true" || name == b"this",
            b'v' => name == b"void",
            b'w' => name == b"with",
            _ => false,
        },
        5 => match first {
            b'a' => name == b"await",
            b'b' => name == b"break",
            b'c' => name == b"catch" || name == b"class" || name == b"const",
            b'f' => name == b"false",
            b's' => name == b"super",
            b't' => name == b"throw",
            b'w' => name == b"while",
            b'y' => name == b"yield",
            _ => false,
        },
        6 => match first {
            b'd' => name == b"delete",
            b'e' => name == b"export",
            b'i' => name == b"import",
            b'r' => name == b"return",
            b's' => name == b"static" || name == b"switch",
            b't' => name == b"typeof",
            _ => false,
        },
        7 => match first {
            b'd' => name == b"default",
            b'e' => name == b"extends",
            b'f' => name == b"finally",
            _ => false,
        },
        8 => match first {
            b'c' => name == b"continue",
            b'd' => name == b"debugger",
            b'f' => name == b"function",
            _ => false,
        },
        9 => first == b'u' && name == b"undefined",
        10 => first == b'i' && name == b"instanceof",
        _ => false,
    }
}

/// Check if a byte slice is a JavaScript built-in global identifier.
///
/// These are globals that Vue's template compiler recognizes as built-in and
/// should NOT be prefixed with `_ctx.` in compiled template expressions.
///
/// List from `@vue/shared` `makeMap(...)`:
/// `Infinity, undefined, NaN, isFinite, isNaN, parseFloat, parseInt,
/// decodeURI, decodeURIComponent, encodeURI, encodeURIComponent, Math,
/// Number, Date, Array, Object, Boolean, String, RegExp, Map, Set, JSON,
/// Intl, BigInt, console, Error, Symbol, WeakMap, WeakSet, WeakRef, Proxy,
/// Reflect, Promise, globalThis, require`
///
/// Note: `undefined` is already covered by [`is_keyword`].
///
/// # Example
/// ```ignore
/// assert!(is_global(b"String"));
/// assert!(is_global(b"Math"));
/// assert!(!is_global(b"myVariable"));
/// ```
#[inline]
pub fn is_global(name: &[u8]) -> bool {
    let len = name.len();

    // Globals range from 3 to 18 characters
    if !(3..=18).contains(&len) {
        return false;
    }

    let first = name[0];

    match len {
        3 => match first {
            b'M' => name == b"Map",
            b'N' => name == b"NaN",
            b'S' => name == b"Set",
            _ => false,
        },
        4 => match first {
            b'D' => name == b"Date",
            b'I' => name == b"Intl",
            b'J' => name == b"JSON",
            b'M' => name == b"Math",
            _ => false,
        },
        5 => match first {
            b'A' => name == b"Array",
            b'E' => name == b"Error",
            b'P' => name == b"Proxy",
            b'i' => name == b"isNaN",
            _ => false,
        },
        6 => match first {
            b'B' => name == b"BigInt",
            b'N' => name == b"Number",
            b'O' => name == b"Object",
            b'R' => name == b"RegExp",
            b'S' => name == b"String" || name == b"Symbol",
            _ => false,
        },
        7 => match first {
            b'B' => name == b"Boolean",
            b'P' => name == b"Promise",
            b'R' => name == b"Reflect",
            b'W' => name == b"WeakMap" || name == b"WeakRef" || name == b"WeakSet",
            b'c' => name == b"console",
            b'r' => name == b"require",
            _ => false,
        },
        8 => match first {
            b'I' => name == b"Infinity",
            b'i' => name == b"isFinite",
            b'p' => name == b"parseInt",
            _ => false,
        },
        9 => match first {
            b'd' => name == b"decodeURI",
            b'e' => name == b"encodeURI",
            _ => false,
        },
        10 => match first {
            b'g' => name == b"globalThis",
            b'p' => name == b"parseFloat",
            _ => false,
        },
        18 => match first {
            b'd' => name == b"decodeURIComponent",
            b'e' => name == b"encodeURIComponent",
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords_by_length() {
        // 2-char keywords
        assert!(is_keyword(b"do"));
        assert!(is_keyword(b"if"));
        assert!(is_keyword(b"in"));

        // 3-char keywords
        assert!(is_keyword(b"for"));
        assert!(is_keyword(b"let"));
        assert!(is_keyword(b"new"));
        assert!(is_keyword(b"try"));
        assert!(is_keyword(b"var"));

        // 4-char keywords
        assert!(is_keyword(b"case"));
        assert!(is_keyword(b"else"));
        assert!(is_keyword(b"enum"));
        assert!(is_keyword(b"null"));
        assert!(is_keyword(b"true"));
        assert!(is_keyword(b"this"));
        assert!(is_keyword(b"void"));
        assert!(is_keyword(b"with"));

        // 5-char keywords
        assert!(is_keyword(b"await"));
        assert!(is_keyword(b"break"));
        assert!(is_keyword(b"catch"));
        assert!(is_keyword(b"class"));
        assert!(is_keyword(b"const"));
        assert!(is_keyword(b"false"));
        assert!(is_keyword(b"super"));
        assert!(is_keyword(b"throw"));
        assert!(is_keyword(b"while"));
        assert!(is_keyword(b"yield"));

        // 6-char keywords
        assert!(is_keyword(b"delete"));
        assert!(is_keyword(b"export"));
        assert!(is_keyword(b"import"));
        assert!(is_keyword(b"return"));
        assert!(is_keyword(b"static"));
        assert!(is_keyword(b"switch"));
        assert!(is_keyword(b"typeof"));

        // 7-char keywords
        assert!(is_keyword(b"default"));
        assert!(is_keyword(b"extends"));
        assert!(is_keyword(b"finally"));

        // 8-char keywords
        assert!(is_keyword(b"continue"));
        assert!(is_keyword(b"debugger"));
        assert!(is_keyword(b"function"));

        // 9-char keywords
        assert!(is_keyword(b"undefined"));

        // 10-char keywords
        assert!(is_keyword(b"instanceof"));
    }

    #[test]
    fn test_non_keywords() {
        assert!(!is_keyword(b"foo"));
        assert!(!is_keyword(b"bar"));
        assert!(!is_keyword(b"myVariable"));
        assert!(!is_keyword(b"a")); // too short
        assert!(!is_keyword(b"verylongidentifier")); // too long
        assert!(!is_keyword(b"Class")); // case-sensitive
        assert!(!is_keyword(b"TRUE")); // case-sensitive
    }

    #[test]
    fn test_edge_cases() {
        assert!(!is_keyword(b"")); // empty
        assert!(!is_keyword(b"x")); // 1 char
        assert!(!is_keyword(b"for_")); // almost a keyword
        assert!(!is_keyword(b"_for")); // almost a keyword
    }

    // ===========================================
    // is_global tests
    // ===========================================

    #[test]
    fn test_globals_3_char() {
        assert!(is_global(b"NaN"));
        assert!(is_global(b"Map"));
        assert!(is_global(b"Set"));
    }

    #[test]
    fn test_globals_4_char() {
        assert!(is_global(b"Date"));
        assert!(is_global(b"Math"));
        assert!(is_global(b"JSON"));
        assert!(is_global(b"Intl"));
    }

    #[test]
    fn test_globals_5_char() {
        assert!(is_global(b"Array"));
        assert!(is_global(b"Error"));
        assert!(is_global(b"Proxy"));
        assert!(is_global(b"isNaN"));
    }

    #[test]
    fn test_globals_6_char() {
        assert!(is_global(b"Object"));
        assert!(is_global(b"Number"));
        assert!(is_global(b"String"));
        assert!(is_global(b"BigInt"));
        assert!(is_global(b"RegExp"));
        assert!(is_global(b"Symbol"));
    }

    #[test]
    fn test_globals_7_char() {
        assert!(is_global(b"Boolean"));
        assert!(is_global(b"Promise"));
        assert!(is_global(b"Reflect"));
        assert!(is_global(b"WeakMap"));
        assert!(is_global(b"WeakRef"));
        assert!(is_global(b"WeakSet"));
        assert!(is_global(b"console"));
        assert!(is_global(b"require"));
    }

    #[test]
    fn test_globals_8_char() {
        assert!(is_global(b"Infinity"));
        assert!(is_global(b"isFinite"));
        assert!(is_global(b"parseInt"));
    }

    #[test]
    fn test_globals_9_10_18_char() {
        assert!(is_global(b"decodeURI")); // 9
        assert!(is_global(b"encodeURI")); // 9
        assert!(is_global(b"globalThis")); // 10
        assert!(is_global(b"parseFloat")); // 10
        assert!(is_global(b"decodeURIComponent")); // 18
        assert!(is_global(b"encodeURIComponent")); // 18
    }

    #[test]
    fn test_non_globals() {
        assert!(!is_global(b"foo"));
        assert!(!is_global(b"bar"));
        assert!(!is_global(b"myVariable"));
        assert!(!is_global(b"")); // empty
        assert!(!is_global(b"x")); // too short
        assert!(!is_global(b"string")); // lowercase
        assert!(!is_global(b"MATH")); // uppercase
        assert!(!is_global(b"array")); // lowercase
        assert!(!is_global(b"Console")); // wrong case
    }
}
