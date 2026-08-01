//! Synthetic-source SFC block splicing for the override upsert paths.
//!
//! Pure string/byte helpers behind
//! [`crate::VerterHost::apply_block_overrides`]: given the original SFC
//! source and preprocessed block overrides, produce a synthetic source
//! with the block content replaced and a PREPROCESSOR `lang` stripped, so
//! the compiler reads what the override actually contains. A native script
//! dialect (`ts`/`tsx`/`js`/`jsx`) is kept — the override of a
//! `<script lang="ts">` block is still TypeScript. No host, cache, or
//! scheduler access — the cluster is text-only by construction (the
//! upsert/eviction-relevant logic stays in `host_upsert.rs`, the single
//! file the `host_upsert_performs_no_reverse_dependent_eviction` guard
//! scans).

use crate::types::{ContentOverride, FileMeta};

/// Build a synthetic SFC source with preprocessed content replacing original
/// block content.
///
/// The synthetic source preserves the same byte structure (tags, offsets) where
/// possible. A template's `lang` is always removed (an override is compiled
/// HTML); a script's is removed only when it names a preprocessor language —
/// see the call site.
pub(super) fn build_synthetic_source(
    original: &str,
    meta: &FileMeta,
    template_override: Option<&ContentOverride>,
    script_override: Option<&ContentOverride>,
) -> String {
    // Simple approach: scan and replace content using string markers.
    // We look for the block tags, strip lang attributes, and replace content.
    let mut result = original.to_string();

    // Replace template content (if override provided)
    if let Some(tpl) = template_override {
        result = replace_block_content(&result, "template", &tpl.code, true);
    }

    // Replace script content (if override provided).
    //
    // `lang` is stripped only for a NON-NATIVE script language — a preprocessor
    // (`coffee`, and anything else the compiler cannot read) whose override
    // content has already been compiled down to JavaScript, so the tag would be
    // lying if it kept saying `coffee`.
    //
    // A NATIVE dialect is the opposite case and must be kept. An override of a
    // `<script lang="ts">` block is still TypeScript — the preprocessor lane
    // never runs for it — and the tag is the ONLY place that says so.
    // Stripping it makes the synthetic SFC a JavaScript one, which changes both
    // how the body is PARSED (`defineProps<T>()` and every type annotation stop
    // being syntax) and how the generated companion is LABELLED (`.jsx`, never
    // typechecked). Both are silent: the macro simply stops being found.
    if let Some(scr) = script_override {
        let strip_lang = meta
            .script_lang
            .as_deref()
            .is_some_and(|lang| !is_native_script_lang(lang));
        result = replace_block_content(&result, "script", &scr.code, strip_lang);
    }

    result
}

/// Whether `lang` names a script dialect the compiler reads directly, rather
/// than a preprocessor language an override has already compiled away.
///
/// Decided through the parser's own `lang` classification, so the two cannot
/// disagree about what `typescript` or `jsx` mean.
fn is_native_script_lang(lang: &str) -> bool {
    !matches!(
        verter_compiler::cursor::ScriptLanguage::from_bytes(lang.as_bytes()),
        verter_compiler::cursor::ScriptLanguage::Unknown
    )
}

/// Replace the content of an SFC block tag and optionally strip its `lang` attribute.
///
/// Finds `<{tag}...>...content...</{tag}>` and replaces the content between
/// the opening and closing tags. If `strip_lang` is true, removes `lang="xxx"`
/// from the opening tag.
fn replace_block_content(source: &str, tag: &str, new_content: &str, strip_lang: bool) -> String {
    let bytes = source.as_bytes();

    // Find the opening tag
    let open_pattern = format!("<{}", tag);
    let Some(tag_start) = find_tag_start(bytes, &open_pattern) else {
        return source.to_string();
    };

    // Find the end of the opening tag (the `>`)
    let Some(tag_end) = find_char_after(bytes, tag_start, b'>') else {
        return source.to_string();
    };
    let content_start = tag_end + 1;

    // Find the closing tag
    let close_pattern = format!("</{}", tag);
    let Some(close_start) = find_pattern_after(bytes, content_start, close_pattern.as_bytes())
    else {
        return source.to_string();
    };

    // Build the result
    let mut result = String::with_capacity(source.len() + new_content.len());

    // Opening tag (with optional lang stripping)
    let opening_tag = &source[tag_start..content_start];
    if strip_lang {
        result.push_str(&source[..tag_start]);
        result.push_str(&strip_lang_attr(opening_tag));
    } else {
        result.push_str(&source[..content_start]);
    }

    // New content
    result.push_str(new_content);

    // From closing tag to end
    result.push_str(&source[close_start..]);

    result
}

/// Strip `lang="..."` or `lang='...'` from an opening tag string.
fn strip_lang_attr(tag: &str) -> String {
    // Match lang="..." or lang='...' with optional whitespace around =
    let bytes = tag.as_bytes();
    let mut result = String::with_capacity(tag.len());
    let mut i = 0;
    while i < bytes.len() {
        // Check if we're at "lang"
        if i + 4 <= bytes.len()
            && bytes[i..i + 4].eq_ignore_ascii_case(b"lang")
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            // Skip past lang="..."
            let mut j = i + 4;
            // Skip whitespace around =
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    let quote = bytes[j];
                    j += 1;
                    while j < bytes.len() && bytes[j] != quote {
                        j += 1;
                    }
                    if j < bytes.len() {
                        j += 1; // skip closing quote
                    }
                }
                // Also consume any trailing whitespace after the value
                // but keep at least one space if we're between attributes
                i = j;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn find_tag_start(bytes: &[u8], pattern: &str) -> Option<usize> {
    let pat = pattern.as_bytes();
    bytes
        .windows(pat.len())
        .position(|w| w.eq_ignore_ascii_case(pat))
}

fn find_char_after(bytes: &[u8], start: usize, ch: u8) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| start + p)
}

fn find_pattern_after(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    if start >= bytes.len() {
        return None;
    }
    bytes[start..]
        .windows(pattern.len())
        .position(|w| w.eq_ignore_ascii_case(pattern))
        .map(|p| start + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script_override(code: &str) -> ContentOverride {
        ContentOverride {
            code: std::sync::Arc::from(code),
            source_map: None,
        }
    }

    fn meta_with_script_lang(lang: Option<&str>) -> FileMeta {
        FileMeta {
            has_script: true,
            script_lang: lang.map(str::to_owned),
            ..FileMeta::default()
        }
    }

    /// A NATIVE script dialect survives the splice; a PREPROCESSOR language does
    /// not.
    ///
    /// The synthetic source is what the compiler then parses and labels, and
    /// `lang` is the only thing on it that says which dialect the block is.
    /// Stripping `lang="ts"` makes the spliced SFC JavaScript: `defineProps<T>()`
    /// stops being syntax at all (so the macro is silently not found) and the
    /// generated companion is labelled `.jsx`, which is never typechecked.
    /// Stripping `lang="coffee"` is the opposite and correct — the override
    /// content has already been compiled to JavaScript, so a tag still claiming
    /// `coffee` would be the lie.
    ///
    /// Fails against an unconditional strip: `lang="ts"` disappears.
    #[test]
    fn splicing_keeps_a_native_script_lang_and_strips_a_preprocessor_one() {
        for lang in ["ts", "tsx", "js", "jsx", "typescript"] {
            let original =
                format!("<script setup lang=\"{lang}\">\nconst a = 1\n</script>\n<template><div/></template>");
            let spliced = build_synthetic_source(
                &original,
                &meta_with_script_lang(Some(lang)),
                None,
                Some(&script_override("defineProps<{ p: number }>()")),
            );
            assert!(
                spliced.contains(&format!("lang=\"{lang}\"")),
                "a native `{lang}` block keeps its dialect through the splice: {spliced}"
            );
            assert!(
                spliced.contains("defineProps<{ p: number }>()"),
                "the override content really was spliced in: {spliced}"
            );
        }

        // Negative: a preprocessor language IS stripped — the override has
        // already compiled it away. Without this half the rule above would pass
        // for a splice that never strips anything.
        let original =
            "<script setup lang=\"coffee\">\na = 1\n</script>\n<template><div/></template>";
        let spliced = build_synthetic_source(
            original,
            &meta_with_script_lang(Some("coffee")),
            None,
            Some(&script_override("const a = 1")),
        );
        assert!(
            !spliced.contains("lang=\"coffee\""),
            "a compiled-away preprocessor lang must not survive: {spliced}"
        );
        assert!(
            spliced.contains("const a = 1"),
            "the compiled content really was spliced in: {spliced}"
        );
    }
}
