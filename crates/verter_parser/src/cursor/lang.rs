//! Script language detection for Vue SFC files.
//!
//! This module provides multiple implementations for detecting the script language
//! from Vue Single File Components, optimized for performance.
//!
//! # Recommended Usage
//!
//! For production use, prefer the optimized [`ScriptDetector`](super::ScriptDetector)
//! from the `script_detector` module, which is re-exported at the cursor level.
//!
//! ```
//! use verter_core::cursor::ScriptDetector;
//!
//! let detector = ScriptDetector::new();
//! let result = detector.detect(b"<script lang=\"ts\"></script>");
//! ```

use memchr::memmem;

// Re-export the optimized detector for backwards compatibility
pub use super::script_detector::{
    DetectResult as LangDetectResult, ScriptDetector as CommentAwareDetector, ScriptLanguage,
};

// ============================================================================
// HTML Comment Detection
// ============================================================================

/// Check if the given position is inside an HTML comment.
/// Scans backwards from `pos` to find `<!--`, then checks if there's a `-->` before `pos`.
#[inline]
pub fn is_in_html_comment(bytes: &[u8], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }

    // Find the last `<!--` before pos
    let before = &bytes[..pos];
    let Some(comment_start) = memmem::rfind(before, b"<!--") else {
        return false;
    };

    // Check if there's a `-->` between comment_start and pos
    let between = &bytes[comment_start + 4..pos];
    memmem::find(between, b"-->").is_none()
}

/// Find `<script ` or `<script>` position, skipping any occurrences inside HTML comments.
/// Returns the position right after `<script ` or `<script>` (where attributes start or tag ends).
///
/// Optimized: only does comment checking if the file contains `<!--` before the script tag.
#[inline]
pub fn find_script_skipping_comments(bytes: &[u8]) -> Option<usize> {
    // Fast path: find first <script (with space or immediate close)
    let (script_pos, tag_len) = if bytes.starts_with(b"<script ") || bytes.starts_with(b"<script>")
    {
        (0, 8)
    } else {
        // Try to find <script with space first, then <script>
        match memmem::find(bytes, b"<script ") {
            Some(pos) => (pos, 8),
            None => (memmem::find(bytes, b"<script>")?, 8),
        }
    };

    // Check if there's any <!-- before this position
    // If not, we can skip comment checking entirely (common case)
    let before_script = &bytes[..script_pos];
    if memmem::find(before_script, b"<!--").is_none() {
        return Some(script_pos + tag_len);
    }

    // Slow path: there are comments, need to check each <script
    find_script_skipping_comments_slow(bytes, script_pos)
}

/// Slow path for when there are HTML comments before the script tag.
#[inline(never)]
fn find_script_skipping_comments_slow(bytes: &[u8], first_script_pos: usize) -> Option<usize> {
    let mut search_start = first_script_pos;

    loop {
        // Check if this <script is inside an HTML comment
        if !is_in_html_comment(bytes, search_start) {
            return Some(search_start + 8);
        }

        // Skip this match and continue searching
        search_start += 8;
        let remaining = &bytes[search_start..];
        let rel_pos = memmem::find(remaining, b"<script ")?;
        search_start += rel_pos;
    }
}

// ============================================================================
// Alternative implementations (for benchmarking/reference)
// ============================================================================

// ============================================================================
// Implementation 1: Multiple memmem calls (original approach)
// ============================================================================

/// Detect script language using multiple memmem::find calls.
/// This is the original implementation.
#[inline]
pub fn detect_lang_memmem_multi(bytes: &[u8]) -> LangDetectResult {
    // Find <script with space (has attrs) or <script> (no attrs)
    let script_offset = if bytes.starts_with(b"<script ") || bytes.starts_with(b"<script>") {
        Some(8)
    } else {
        memmem::find(bytes, b"<script ")
            .map(|pos| pos + 8)
            .or_else(|| memmem::find(bytes, b"<script>").map(|pos| pos + 8))
    };

    let Some(n) = script_offset else {
        return LangDetectResult {
            language: ScriptLanguage::TypeScript,
            script_found: false,
            lang_attr_found: false,
            ..Default::default()
        };
    };

    let end = (n + 82).min(bytes.len());
    let haystack = &bytes[n..end];

    let (language, lang_attr_found) = if memmem::find(haystack, b"lang=\"ts\"").is_some()
        || memmem::find(haystack, b"lang='ts'").is_some()
        || memmem::find(haystack, b"lang=ts").is_some()
        || memmem::find(haystack, b"lang=\"typescript\"").is_some()
        || memmem::find(haystack, b"lang='typescript'").is_some()
    {
        (ScriptLanguage::TypeScript, true)
    } else if memmem::find(haystack, b"lang=\"tsx\"").is_some()
        || memmem::find(haystack, b"lang='tsx'").is_some()
        || memmem::find(haystack, b"lang=tsx").is_some()
    {
        (ScriptLanguage::TSX, true)
    } else if memmem::find(haystack, b"lang=\"jsx\"").is_some()
        || memmem::find(haystack, b"lang='jsx'").is_some()
        || memmem::find(haystack, b"lang=jsx").is_some()
    {
        (ScriptLanguage::JSX, true)
    } else if memmem::find(haystack, b"lang=\"js\"").is_some()
        || memmem::find(haystack, b"lang='js'").is_some()
        || memmem::find(haystack, b"lang=js").is_some()
        || memmem::find(haystack, b"lang=\"javascript\"").is_some()
        || memmem::find(haystack, b"lang='javascript'").is_some()
    {
        (ScriptLanguage::JavaScript, true)
    } else {
        (ScriptLanguage::TypeScript, false)
    };

    LangDetectResult {
        language,
        script_found: true,
        lang_attr_found,
        ..Default::default()
    }
}

// ============================================================================
// Implementation 2: Single scan with manual parsing (comment-aware)
// ============================================================================

/// Detect script language with a single scan looking for `lang=`.
/// After finding `lang=`, parse the value directly.
/// Skips `<script>` tags inside HTML comments.
#[inline]
pub fn detect_lang_single_scan(bytes: &[u8]) -> LangDetectResult {
    let Some(n) = find_script_skipping_comments(bytes) else {
        return LangDetectResult {
            language: ScriptLanguage::TypeScript,
            script_found: false,
            lang_attr_found: false,
            ..Default::default()
        };
    };

    let end = (n + 82).min(bytes.len());
    let haystack = &bytes[n..end];

    // Find `lang=` and parse the value
    if let Some(lang_pos) = memmem::find(haystack, b"lang=") {
        let value_start = lang_pos + 5; // skip "lang="
        if value_start < haystack.len() {
            let language = parse_lang_value(&haystack[value_start..]);
            return LangDetectResult {
                language,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            };
        }
    }

    LangDetectResult {
        language: ScriptLanguage::TypeScript,
        script_found: true,
        lang_attr_found: false,
        ..Default::default()
    }
}

/// Parse language value from bytes starting after `lang=`.
#[inline]
fn parse_lang_value(bytes: &[u8]) -> ScriptLanguage {
    if bytes.is_empty() {
        return ScriptLanguage::TypeScript;
    }

    // Skip opening quote if present
    let (bytes, quote) = match bytes[0] {
        b'"' | b'\'' => (&bytes[1..], Some(bytes[0])),
        _ => (bytes, None),
    };

    // Find end of value
    let end = if let Some(q) = quote {
        bytes.iter().position(|&b| b == q).unwrap_or(bytes.len())
    } else {
        bytes
            .iter()
            .position(|&b| b == b' ' || b == b'>' || b == b'/')
            .unwrap_or(bytes.len())
    };

    match &bytes[..end.min(bytes.len())] {
        b"ts" | b"typescript" => ScriptLanguage::TypeScript,
        b"tsx" => ScriptLanguage::TSX,
        b"jsx" => ScriptLanguage::JSX,
        b"js" | b"javascript" => ScriptLanguage::JavaScript,
        _ => ScriptLanguage::TypeScript,
    }
}

// ============================================================================
// Implementation 3: memchr to find 'l', then check for "lang="
// ============================================================================

/// Detect script language using memchr to find potential `lang=` positions.
#[inline]
pub fn detect_lang_memchr(bytes: &[u8]) -> LangDetectResult {
    let script_offset = if bytes.starts_with(b"<script ") || bytes.starts_with(b"<script>") {
        Some(8)
    } else {
        memmem::find(bytes, b"<script ")
            .map(|pos| pos + 8)
            .or_else(|| memmem::find(bytes, b"<script>").map(|pos| pos + 8))
    };

    let Some(n) = script_offset else {
        return LangDetectResult {
            language: ScriptLanguage::TypeScript,
            script_found: false,
            lang_attr_found: false,
            ..Default::default()
        };
    };

    let end = (n + 82).min(bytes.len());
    let haystack = &bytes[n..end];

    // Use memchr to find 'l' characters, then verify "lang="
    let mut pos = 0;
    while pos < haystack.len() {
        if let Some(l_pos) = memchr::memchr(b'l', &haystack[pos..]) {
            let abs_pos = pos + l_pos;
            if abs_pos + 5 <= haystack.len() && &haystack[abs_pos..abs_pos + 5] == b"lang=" {
                let value_start = abs_pos + 5;
                if value_start < haystack.len() {
                    let language = parse_lang_value(&haystack[value_start..]);
                    return LangDetectResult {
                        language,
                        script_found: true,
                        lang_attr_found: true,
                        ..Default::default()
                    };
                }
            }
            pos = abs_pos + 1;
        } else {
            break;
        }
    }

    LangDetectResult {
        language: ScriptLanguage::TypeScript,
        script_found: true,
        lang_attr_found: false,
        ..Default::default()
    }
}

// ============================================================================
// Implementation 4: Precompiled Finder objects
// ============================================================================

/// Precompiled finders for repeated use.
pub struct LangDetector {
    script_finder: memmem::Finder<'static>,
    lang_finder: memmem::Finder<'static>,
}

impl Default for LangDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LangDetector {
    /// Create a new detector with precompiled patterns.
    pub fn new() -> Self {
        Self {
            script_finder: memmem::Finder::new(b"<script "),
            lang_finder: memmem::Finder::new(b"lang="),
        }
    }

    /// Detect script language using precompiled finders.
    #[inline]
    pub fn detect(&self, bytes: &[u8]) -> LangDetectResult {
        let script_offset = if bytes.starts_with(b"<script ") || bytes.starts_with(b"<script>") {
            Some(8)
        } else {
            self.script_finder
                .find(bytes)
                .map(|pos| pos + 8)
                .or_else(|| memmem::find(bytes, b"<script>").map(|pos| pos + 8))
        };

        let Some(n) = script_offset else {
            return LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: false,
                lang_attr_found: false,
                ..Default::default()
            };
        };

        let end = (n + 82).min(bytes.len());
        let haystack = &bytes[n..end];

        if let Some(lang_pos) = self.lang_finder.find(haystack) {
            let value_start = lang_pos + 5;
            if value_start < haystack.len() {
                let language = parse_lang_value(&haystack[value_start..]);
                return LangDetectResult {
                    language,
                    script_found: true,
                    lang_attr_found: true,
                    ..Default::default()
                };
            }
        }

        LangDetectResult {
            language: ScriptLanguage::TypeScript,
            script_found: true,
            lang_attr_found: false,
            ..Default::default()
        }
    }
}

// ============================================================================
// Implementation 5: Byte-by-byte scan (no memchr dependency)
// ============================================================================

/// Detect script language with pure byte-by-byte scanning.
/// Useful as a baseline and for small inputs where SIMD overhead isn't worth it.
#[inline]
pub fn detect_lang_naive(bytes: &[u8]) -> LangDetectResult {
    // Find <script
    let script_offset = find_script_naive(bytes);

    let Some(n) = script_offset else {
        return LangDetectResult {
            language: ScriptLanguage::TypeScript,
            script_found: false,
            lang_attr_found: false,
            ..Default::default()
        };
    };

    let end = (n + 82).min(bytes.len());
    let haystack = &bytes[n..end];

    // Find lang=
    for i in 0..haystack.len().saturating_sub(4) {
        if haystack[i] == b'l'
            && haystack[i + 1] == b'a'
            && haystack[i + 2] == b'n'
            && haystack[i + 3] == b'g'
            && haystack[i + 4] == b'='
        {
            let value_start = i + 5;
            if value_start < haystack.len() {
                let language = parse_lang_value(&haystack[value_start..]);
                return LangDetectResult {
                    language,
                    script_found: true,
                    lang_attr_found: true,
                    ..Default::default()
                };
            }
        }
    }

    LangDetectResult {
        language: ScriptLanguage::TypeScript,
        script_found: true,
        lang_attr_found: false,
        ..Default::default()
    }
}

#[inline]
fn find_script_naive(bytes: &[u8]) -> Option<usize> {
    const NEEDLE_SPACE: &[u8] = b"<script ";
    const NEEDLE_CLOSE: &[u8] = b"<script>";
    if bytes.len() < NEEDLE_SPACE.len() {
        return None;
    }

    for i in 0..=bytes.len() - NEEDLE_SPACE.len() {
        if &bytes[i..i + NEEDLE_SPACE.len()] == NEEDLE_SPACE {
            return Some(i + NEEDLE_SPACE.len());
        }
        if &bytes[i..i + NEEDLE_CLOSE.len()] == NEEDLE_CLOSE {
            return Some(i + NEEDLE_CLOSE.len());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare only the core fields (ignoring lang_start/lang_end which are only
    /// implemented in CommentAwareDetector)
    fn assert_result_matches(result: &LangDetectResult, expected: &LangDetectResult, name: &str) {
        assert_eq!(
            result.language, expected.language,
            "{} language mismatch",
            name
        );
        assert_eq!(
            result.script_found, expected.script_found,
            "{} script_found mismatch",
            name
        );
        assert_eq!(
            result.lang_attr_found, expected.lang_attr_found,
            "{} lang_attr_found mismatch",
            name
        );
    }

    /// Test all implementations (for cases without HTML comments)
    fn test_all_implementations(input: &[u8], expected: LangDetectResult) {
        let result1 = detect_lang_memmem_multi(input);
        let result2 = detect_lang_single_scan(input);
        let result3 = detect_lang_memchr(input);
        let detector4 = LangDetector::new();
        let result4 = detector4.detect(input);
        let result5 = detect_lang_naive(input);
        let detector6 = CommentAwareDetector::new();
        let result6 = detector6.detect(input);

        assert_result_matches(&result1, &expected, "memmem_multi");
        assert_result_matches(&result2, &expected, "single_scan");
        assert_result_matches(&result3, &expected, "memchr");
        assert_result_matches(&result4, &expected, "precompiled");
        assert_result_matches(&result5, &expected, "naive");
        assert_result_matches(&result6, &expected, "comment_aware");
    }

    /// Test only comment-aware implementations (for cases with HTML comments)
    fn test_comment_aware(input: &[u8], expected: LangDetectResult) {
        let result1 = detect_lang_single_scan(input);
        let detector2 = CommentAwareDetector::new();
        let result2 = detector2.detect(input);

        assert_result_matches(&result1, &expected, "single_scan");
        assert_result_matches(&result2, &expected, "comment_aware");
    }

    #[test]
    fn no_script_tag() {
        test_all_implementations(
            b"<template><div>Hello</div></template>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: false,
                lang_attr_found: false,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_no_lang() {
        test_all_implementations(
            b"<template/><script></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: false,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_ts_double_quotes() {
        test_all_implementations(
            b"<template/><script lang=\"ts\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_ts_single_quotes() {
        test_all_implementations(
            b"<template/><script lang='ts'></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_ts_no_quotes() {
        test_all_implementations(
            b"<template/><script lang=ts></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_tsx() {
        test_all_implementations(
            b"<template/><script lang=\"tsx\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_jsx() {
        test_all_implementations(
            b"<template/><script lang=\"jsx\"></script>",
            LangDetectResult {
                language: ScriptLanguage::JSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_js() {
        test_all_implementations(
            b"<template/><script lang=\"js\"></script>",
            LangDetectResult {
                language: ScriptLanguage::JavaScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_at_beginning() {
        test_all_implementations(
            b"<script lang=\"tsx\"></script><template/>",
            LangDetectResult {
                language: ScriptLanguage::TSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_with_setup_and_lang() {
        test_all_implementations(
            b"<template/><script setup lang=\"ts\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_typescript_full() {
        test_all_implementations(
            b"<script lang=\"typescript\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_lang_javascript_full() {
        test_all_implementations(
            b"<script lang=\"javascript\"></script>",
            LangDetectResult {
                language: ScriptLanguage::JavaScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn ignores_lang_attr_in_template_components() {
        // This tests that lang="js" inside a component doesn't confuse the detector
        test_all_implementations(
            b"<template>\n  <Comp lang=\"js\"/>\n</template>\n<script setup lang=\"ts\">\n</script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn ignores_script_like_text_content() {
        // Text containing "<script " but not as an actual tag
        test_all_implementations(
            b"<template><div>Use &lt;script lang=\"js\"&gt; for JS</div></template>\n<script lang=\"tsx\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    // ========================================================================
    // HTML Comment Tests (comment-aware implementations only)
    // ========================================================================

    #[test]
    fn skips_script_in_html_comment() {
        test_comment_aware(
            b"<!-- <script lang=\"js\"> -->\n<script lang=\"ts\">\n</script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn skips_script_in_html_comment_at_start() {
        test_comment_aware(
            b"<!-- <script lang=\"jsx\"></script> --><script lang=\"tsx\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn handles_multiple_comments_before_script() {
        test_comment_aware(
            b"<!-- comment 1 --><!-- <script lang=\"js\"> -->\n<script lang=\"ts\"></script>",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn script_after_closed_comment_is_valid() {
        // Comment is properly closed, so the script after it is valid
        test_comment_aware(
            b"<!-- comment --><script lang=\"jsx\"></script>",
            LangDetectResult {
                language: ScriptLanguage::JSX,
                script_found: true,
                lang_attr_found: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn no_script_when_only_commented_script() {
        test_comment_aware(
            b"<template/><!-- <script lang=\"ts\"></script> -->",
            LangDetectResult {
                language: ScriptLanguage::TypeScript,
                script_found: false,
                lang_attr_found: false,
                ..Default::default()
            },
        );
    }

    // ========================================================================
    // is_in_html_comment unit tests
    // ========================================================================

    #[test]
    fn test_is_in_html_comment_basic() {
        let bytes = b"<!-- hello --> world";
        assert!(is_in_html_comment(bytes, 5)); // inside comment
        assert!(is_in_html_comment(bytes, 10)); // inside comment
        assert!(!is_in_html_comment(bytes, 15)); // after -->
        assert!(!is_in_html_comment(bytes, 18)); // after -->
    }

    #[test]
    fn test_is_in_html_comment_no_comment() {
        let bytes = b"hello world";
        assert!(!is_in_html_comment(bytes, 5));
    }

    #[test]
    fn test_is_in_html_comment_multiple() {
        let bytes = b"<!-- a --> b <!-- c -->";
        assert!(is_in_html_comment(bytes, 6)); // in first comment
        assert!(!is_in_html_comment(bytes, 12)); // between comments
        assert!(is_in_html_comment(bytes, 18)); // in second comment
    }
}
