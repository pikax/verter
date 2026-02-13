use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform, common::Span, syntax_kai::binding_types::BindingType,
    utils::oxc::BindingExtractionResult,
};

pub fn patch_bindings<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    bindings: Option<&BindingExtractionResult<'alloc>>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
) {
    if let Some(bindings) = bindings {
        bindings.bindings.iter().for_each(|f| {
            if !f.ignore {
                if let Some(b) = map.get(&f.name) {
                    code_transform.prepend_left(f.span.start, b.accessor_prefix(is_inline));
                } else {
                    // Unresolved identifiers get _ctx. prefix (Vue behavior)
                    code_transform.prepend_left(f.span.start, "_ctx.");
                }
            }
        });
    }
}

/// Collect binding patches into a buffer for batch application.
///
/// Like `patch_bindings` but does NOT call `prepend_left`. Instead, pushes
/// `(position, prefix)` pairs into `out` for later batch application via
/// `CodeTransform::batch_prepend_left_static`.
///
/// This enables O(n+m) batch insertion instead of O(n*m) individual inserts.
pub fn collect_binding_patches<'alloc>(
    bindings: Option<&BindingExtractionResult<'alloc>>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
    out: &mut Vec<(u32, &'alloc str)>,
) {
    if let Some(bindings) = bindings {
        for f in &bindings.bindings {
            if !f.ignore {
                let prefix = if let Some(b) = map.get(&f.name) {
                    b.accessor_prefix(is_inline)
                } else {
                    "_ctx."
                };
                if !prefix.is_empty() {
                    out.push((f.span.start, prefix));
                }
            }
        }
    }
}

/// Apply accessor prefixes AND v-for/v-slot variable replacements in-place on code_transform.
///
/// Like `patch_bindings` but also handles variable mappings (e.g., `item` → `_for_item0.value`).
/// Variable mappings are checked BEFORE the `ignore` flag because v-for/v-slot locals are
/// marked `ignore: true` by OXC but still need replacement with their mapped values.
///
/// No sorting, no Vec allocation, no return value — follows the `patch_bindings` pattern.
#[allow(dead_code)]
pub fn patch_bindings_with_var_mappings<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    bindings: Option<&BindingExtractionResult<'alloc>>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
    var_mappings: &[(String, String)],
) {
    if let Some(br) = bindings {
        for b in &br.bindings {
            // var_mappings checked BEFORE ignore (v-for locals are ignore:true but still need replacement)
            if let Some((_, mapped)) = var_mappings.iter().find(|(orig, _)| orig == b.name) {
                code_transform.overwrite(b.span.start, b.span.end, mapped);
            } else if b.ignore {
                continue;
            } else if let Some(bt) = map.get(&b.name) {
                let prefix = bt.accessor_prefix(is_inline);
                if !prefix.is_empty() {
                    code_transform.prepend_left(b.span.start, prefix);
                }
            } else {
                code_transform.prepend_left(b.span.start, "_ctx.");
            }
        }
    }
}

/// Apply accessor prefixes to v-for external references in-place on code_transform.
///
/// Like `patch_bindings` but operates on v-for reference spans instead of OXC bindings.
/// No sorting, no Vec allocation, no return value.
#[allow(dead_code)]
pub fn patch_vfor_references(
    code_transform: &mut CodeTransform,
    references: &[Span],
    filter_range: Option<(u32, u32)>,
    input: &str,
    bindings: &FxHashMap<&str, BindingType>,
    is_production: bool,
) {
    for r in references {
        if let Some((start, end)) = filter_range {
            if r.start < start || r.end > end {
                continue;
            }
        }
        let name = &input[r.start as usize..r.end as usize];
        let prefix = if let Some(bt) = bindings.get(name) {
            bt.accessor_prefix(is_production)
        } else {
            "_ctx."
        };
        if !prefix.is_empty() {
            code_transform.prepend_left(r.start, prefix);
        }
    }
}

/// Build a value string with accessor prefixes AND optional variable replacements applied.
///
/// Iterates OXC-parsed bindings directly (already in source order — no sorting needed).
/// No `Vec`, no `Patch` enum, no `.sort()`. This correctly handles:
/// - Identifiers inside string literals (won't be in the binding list)
/// - Unicode identifiers (OXC parser handles these)
/// - v-for/v-slot variable mappings (e.g., `item` → `_for_item0.value`)
///
/// For each OXC-identified binding:
/// - If it matches a variable mapping key → replace the entire identifier
/// - If `ignore: true` with no mapping → skip
/// - Otherwise → insert the accessor prefix
///
/// Pass `var_mappings: &[]` when no variable replacements are needed (replaces
/// the removed `apply_dynamic_arg_prefix` and `build_prefixed_value` functions).
pub fn build_prefixed_value_with_var_mappings(
    val_text: &str,
    val_start: u32,
    bindings_result: Option<&BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
    var_mappings: &[(String, String)],
) -> String {
    let Some(br) = bindings_result else {
        return val_text.to_string();
    };

    let mut result = String::with_capacity(val_text.len() + br.bindings.len() * 6);
    let mut last = 0usize;
    let mut modified = false;

    for b in &br.bindings {
        let offset = (b.span.start - val_start) as usize;
        let ident_len = (b.span.end - b.span.start) as usize;

        // Check variable mappings first (v-for/v-slot variables take precedence).
        // This must happen BEFORE the `ignore` check because v-for/v-slot locals
        // are marked `ignore: true` by OXC but still need replacement.
        if let Some((_, mapped)) = var_mappings.iter().find(|(orig, _)| orig == b.name) {
            result.push_str(&val_text[last..offset]);
            result.push_str(mapped);
            last = offset + ident_len;
            modified = true;
        } else if b.ignore {
            continue;
        } else {
            let prefix = if let Some(bt) = map.get(b.name) {
                bt.accessor_prefix(is_inline)
            } else {
                "_ctx."
            };
            if !prefix.is_empty() {
                result.push_str(&val_text[last..offset]);
                result.push_str(prefix);
                last = offset; // keep original identifier
                modified = true;
            }
        }
    }

    if !modified {
        // No patches applied — return original text without extra allocation
        return val_text.to_string();
    }
    result.push_str(&val_text[last..]);
    result
}

/// Apply accessor prefixes (`_ctx.`, `$setup.`, etc.) to external references
/// in a v-for iterable expression.
///
/// Used by both VDOM and Vapor backends to prefix references in v-for right-hand
/// side expressions. References are processed in reverse order to preserve offsets.
///
/// - `text`: the expression string to prefix (may be the full v-for value or just the iterable)
/// - `base_offset`: absolute source offset of the start of `text`
/// - `references`: spans of external references from v-for parsing
/// - `filter_range`: if `Some((start, end))`, only prefix references within this absolute range
/// - `input`: full source input (for extracting reference names)
/// - `bindings`: binding map for determining accessor prefix
/// - `is_production`: controls accessor prefix style (inline vs setup)
pub fn prefix_vfor_references(
    text: &str,
    base_offset: u32,
    references: &[Span],
    filter_range: Option<(u32, u32)>,
    input: &str,
    bindings: &FxHashMap<&str, BindingType>,
    is_production: bool,
) -> String {
    // Forward iteration with push_str — no Vec allocation, no sort, no insert_str.
    // References from v-for parsing are already in source order.
    let mut result = String::with_capacity(text.len() + references.len() * 6);
    let mut last = 0usize;
    let mut modified = false;

    for r in references {
        if let Some((range_start, range_end)) = filter_range {
            if r.start < range_start || r.end > range_end {
                continue;
            }
        }
        let offset = (r.start - base_offset) as usize;
        let name = &input[r.start as usize..r.end as usize];
        let prefix = if let Some(bt) = bindings.get(name) {
            bt.accessor_prefix(is_production)
        } else {
            "_ctx."
        };
        if !prefix.is_empty() {
            result.push_str(&text[last..offset]);
            result.push_str(prefix);
            last = offset; // keep original identifier
            modified = true;
        }
    }

    if !modified {
        return text.to_string();
    }
    result.push_str(&text[last..]);
    result
}

/// Like `build_prefixed_value_with_var_mappings` but appends into an existing buffer.
/// Avoids allocating a new String — the caller provides the buffer.
pub fn build_prefixed_value_into(
    buf: &mut String,
    val_text: &str,
    val_start: u32,
    bindings_result: Option<&BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
    var_mappings: &[(String, String)],
) {
    let Some(br) = bindings_result else {
        buf.push_str(val_text);
        return;
    };

    let mut last = 0usize;
    let mut modified = false;

    for b in &br.bindings {
        let offset = (b.span.start - val_start) as usize;
        let ident_len = (b.span.end - b.span.start) as usize;

        if let Some((_, mapped)) = var_mappings.iter().find(|(orig, _)| orig == b.name) {
            buf.push_str(&val_text[last..offset]);
            buf.push_str(mapped);
            last = offset + ident_len;
            modified = true;
        } else if b.ignore {
            continue;
        } else {
            let prefix = if let Some(bt) = map.get(b.name) {
                bt.accessor_prefix(is_inline)
            } else {
                "_ctx."
            };
            if !prefix.is_empty() {
                buf.push_str(&val_text[last..offset]);
                buf.push_str(prefix);
                last = offset;
                modified = true;
            }
        }
    }

    if !modified {
        buf.push_str(val_text);
    } else {
        buf.push_str(&val_text[last..]);
    }
}

/// Like `prefix_vfor_references` but appends into an existing buffer.
/// Avoids allocating a new String — the caller provides the buffer.
pub fn prefix_vfor_references_into(
    buf: &mut String,
    text: &str,
    base_offset: u32,
    references: &[Span],
    filter_range: Option<(u32, u32)>,
    input: &str,
    bindings: &FxHashMap<&str, BindingType>,
    is_production: bool,
) {
    let mut last = 0usize;
    let mut modified = false;

    for r in references {
        if let Some((range_start, range_end)) = filter_range {
            if r.start < range_start || r.end > range_end {
                continue;
            }
        }
        let offset = (r.start - base_offset) as usize;
        let name = &input[r.start as usize..r.end as usize];
        let prefix = if let Some(bt) = bindings.get(name) {
            bt.accessor_prefix(is_production)
        } else {
            "_ctx."
        };
        if !prefix.is_empty() {
            buf.push_str(&text[last..offset]);
            buf.push_str(prefix);
            last = offset;
            modified = true;
        }
    }

    if !modified {
        buf.push_str(text);
    } else {
        buf.push_str(&text[last..]);
    }
}

/// Escape characters in-place within a code_transform range for a JavaScript string literal.
///
/// Reads characters from `input[start..end]` and pushes overwrites for those that need
/// escaping into `pending_overwrites`. Characters that don't need escaping stay untouched,
/// preserving their source positions.
///
/// The `code_transform` reference is only used for `alloc_str()` (control char hex escapes).
/// All overwrites are deferred for batch application.
pub fn escape_js_string_in_place<'a>(
    code_transform: &CodeTransform<'a>,
    start: u32,
    end: u32,
    input: &str,
    pending_overwrites: &mut Vec<(u32, u32, &'a str)>,
) {
    let bytes = input[start as usize..end as usize].as_bytes();

    // Fast path: check if any escaping is needed at all.
    // Most attribute values contain no special characters.
    let needs_escape = bytes.iter().any(|&b| {
        matches!(
            b,
            b'\\' | b'"' | b'\n' | b'\r' | b'\t' | b'\0' | 0xe2 // LS/PS start byte
        ) || b.is_ascii_control()
    });
    if !needs_escape {
        return;
    }

    // Slow path: iterate character by character
    let text = &input[start as usize..end as usize];
    let mut pos = start;
    for ch in text.chars() {
        let char_len = ch.len_utf8() as u32;
        let escape: Option<&str> = match ch {
            '\\' => Some("\\\\"),
            '"' => Some("\\\""),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            '\0' => Some("\\0"),
            '\u{2028}' => Some("\\u2028"),
            '\u{2029}' => Some("\\u2029"),
            _ => None,
        };
        if let Some(esc) = escape {
            pending_overwrites.push((pos, pos + char_len, esc));
        } else if ch.is_ascii_control() {
            let mut hex_buf = [0u8; 4]; // "\\xHH"
            hex_buf[0] = b'\\';
            hex_buf[1] = b'x';
            let val = ch as u8;
            hex_buf[2] = HEX_DIGITS[(val >> 4) as usize];
            hex_buf[3] = HEX_DIGITS[(val & 0xf) as usize];
            let s = unsafe { std::str::from_utf8_unchecked(&hex_buf) };
            let s = code_transform.alloc_str(s);
            pending_overwrites.push((pos, pos + char_len, s));
        }
        pos += char_len;
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Capitalize the first ASCII character in-place within a code_transform range.
///
/// Reads the byte at `input[start]` and if it's a lowercase ASCII letter,
/// overwrites it with its uppercase equivalent. Non-lowercase bytes are left untouched.
#[allow(dead_code)]
pub fn capitalize_first_in_place(code_transform: &mut CodeTransform, start: u32, input: &str) {
    let first_byte = input.as_bytes()[start as usize];
    if first_byte.is_ascii_lowercase() {
        let upper = (first_byte - b'a' + b'A') as char;
        let mut buf = [0u8; 1];
        code_transform.overwrite(start, start + 1, upper.encode_utf8(&mut buf));
    }
}

/// Escape a string for use inside a JavaScript string literal (double-quoted).
///
/// Handles backslash, double-quote, common whitespace escapes, null bytes,
/// ASCII control characters (U+0000–U+001F) via `\xNN` notation, and
/// U+2028/U+2029 (JS line terminators in pre-ES2019) via `\uXXXX` notation.
pub fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            // U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR are valid
            // in JSON but are line terminators in JS (pre-ES2019). Escape them
            // to ensure valid JS string literals in all runtimes.
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            // Other ASCII control characters → \xNN
            c if c.is_ascii_control() => {
                out.push_str(&format!("\\x{:02X}", c as u32));
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Classify an event modifier into one of three categories.
///
/// Used by both VDOM and Vapor backends to categorize `@event.mod` modifiers:
/// - **ListenerOption**: `capture`, `once`, `passive` — translated to event listener options
/// - **KeyFilter**: `enter`, `tab`, `delete`, `esc`, `space`, `up`, `down`, `left`, `right` — key guard modifiers
/// - **Runtime**: everything else — runtime behavior modifiers (e.g., `stop`, `prevent`, `self`, `exact`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKind {
    ListenerOption,
    KeyFilter,
    Runtime,
}

pub fn classify_modifier(name: &str) -> ModifierKind {
    match name {
        "capture" | "once" | "passive" => ModifierKind::ListenerOption,
        "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down" | "left" | "right" => {
            ModifierKind::KeyFilter
        }
        _ => ModifierKind::Runtime,
    }
}

/// Capitalize the first ASCII character of a string (e.g., `click` → `Click`).
pub fn capitalize_first(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    capitalize_first_into(s, &mut out);
    out
}

/// Capitalize the first character of `s` and append to `buf` (avoids allocation).
pub fn capitalize_first_into(s: &str, buf: &mut String) {
    let mut chars = s.chars();
    if let Some(c) = chars.next() {
        for upper in c.to_uppercase() {
            buf.push(upper);
        }
        buf.push_str(chars.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    // ── escape_js_string_in_place tests ─────────────────────────────────

    /// Helper: apply deferred escape overwrites and return the result string.
    fn escape_and_apply(
        ct: &mut crate::code_transform::CodeTransform,
        start: u32,
        end: u32,
        input: &str,
    ) -> String {
        let mut pending = Vec::new();
        escape_js_string_in_place(ct, start, end, input, &mut pending);
        if !pending.is_empty() {
            pending.sort_unstable_by_key(|(s, _, _)| *s);
            ct.batch_overwrite(&pending);
        }
        ct.to_string()
    }

    #[test]
    fn test_escape_js_string_in_place_no_op() {
        let input = "hello world";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 11, input), "hello world");
    }

    #[test]
    fn test_escape_js_string_in_place_backslash() {
        let input = "a\\b";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\\\b");
    }

    #[test]
    fn test_escape_js_string_in_place_double_quote() {
        let input = "he\"llo";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 6, input), "he\\\"llo");
    }

    #[test]
    fn test_escape_js_string_in_place_newline() {
        let input = "a\nb";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\nb");
    }

    #[test]
    fn test_escape_js_string_in_place_carriage_return() {
        let input = "a\rb";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\rb");
    }

    #[test]
    fn test_escape_js_string_in_place_tab() {
        let input = "a\tb";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\tb");
    }

    #[test]
    fn test_escape_js_string_in_place_null() {
        let input = "a\0b";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\0b");
    }

    #[test]
    fn test_escape_js_string_in_place_control_char() {
        let input = "a\x01b";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(escape_and_apply(&mut ct, 0, 3, input), "a\\x01b");
    }

    #[test]
    fn test_escape_js_string_in_place_line_separator() {
        let input = "a\u{2028}b";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(
            escape_and_apply(&mut ct, 0, input.len() as u32, input),
            "a\\u2028b"
        );
    }

    #[test]
    fn test_escape_js_string_in_place_paragraph_separator() {
        let input = "a\u{2029}b";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(
            escape_and_apply(&mut ct, 0, input.len() as u32, input),
            "a\\u2029b"
        );
    }

    #[test]
    fn test_escape_js_string_in_place_multiple_escapes() {
        let input = "a\"b\\c\nd";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        assert_eq!(
            escape_and_apply(&mut ct, 0, input.len() as u32, input),
            "a\\\"b\\\\c\\nd"
        );
    }

    #[test]
    fn test_escape_js_string_in_place_partial_range() {
        // Only escape within a sub-range, leaving surrounding content untouched
        let input = "prefix\"middle\"suffix";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        // Escape only "middle" (positions 7..13, which is `middle` without quotes)
        // Actually positions: p=0..6 is "prefix", 6 is ", 7..13 is "middle", 13 is ", 14..20 is "suffix"
        assert_eq!(
            escape_and_apply(&mut ct, 6, 14, input),
            "prefix\\\"middle\\\"suffix"
        );
    }

    // ── capitalize_first_in_place tests ─────────────────────────────────

    #[test]
    fn test_capitalize_first_in_place_lowercase() {
        let input = "click";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        capitalize_first_in_place(&mut ct, 0, input);
        assert_eq!(ct.to_string(), "Click");
    }

    #[test]
    fn test_capitalize_first_in_place_already_upper() {
        let input = "Click";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        capitalize_first_in_place(&mut ct, 0, input);
        assert_eq!(ct.to_string(), "Click");
    }

    #[test]
    fn test_capitalize_first_in_place_at_offset() {
        let input = "on-click";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        capitalize_first_in_place(&mut ct, 3, input);
        assert_eq!(ct.to_string(), "on-Click");
    }

    #[test]
    fn test_capitalize_first_in_place_non_ascii_no_op() {
        let input = "42abc";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        capitalize_first_in_place(&mut ct, 0, input);
        // '4' is not ascii_lowercase, so no change
        assert_eq!(ct.to_string(), "42abc");
    }

    #[test]
    fn test_escape_js_string() {
        assert_eq!(escape_js_string("hello"), "hello");
        assert_eq!(escape_js_string("he\"llo"), "he\\\"llo");
        assert_eq!(escape_js_string("a\\b"), "a\\\\b");
        assert_eq!(escape_js_string("a\nb"), "a\\nb");
        assert_eq!(escape_js_string("a\0b"), "a\\0b");
        assert_eq!(escape_js_string("a\x01b"), "a\\x01b");
        assert_eq!(escape_js_string("a\x1Fb"), "a\\x1Fb");
        assert_eq!(escape_js_string("a\u{2028}b"), "a\\u2028b");
        assert_eq!(escape_js_string("a\u{2029}b"), "a\\u2029b");
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("click"), "Click");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
        assert_eq!(capitalize_first("Click"), "Click");
    }

    // ── patch_bindings_with_var_mappings tests ────────────────────────

    /// Helper to build a BindingExtractionResult for tests.
    fn make_bindings<'a>(entries: &[(&'a str, u32, u32, bool)]) -> BindingExtractionResult<'a> {
        use crate::utils::oxc::Binding;
        BindingExtractionResult {
            bindings: entries
                .iter()
                .map(|(name, start, end, ignore)| Binding {
                    name,
                    span: Span {
                        start: *start,
                        end: *end,
                    },
                    pos: *start,
                    ignore: *ignore,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_prefix_only() {
        // Expression: "show && visible" at positions 0..15
        // Bindings: show@0..4 (unresolved), visible@8..15 (unresolved)
        let input = "show && visible";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let br = make_bindings(&[("show", 0, 4, false), ("visible", 8, 15, false)]);
        let map = FxHashMap::default(); // empty → all get _ctx.
        patch_bindings_with_var_mappings(&mut ct, Some(&br), &map, false, &[]);
        assert_eq!(ct.to_string(), "_ctx.show && _ctx.visible");
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_replace_only() {
        // Expression: "item.name" at positions 0..9
        // Bindings: item@0..4 (ignore:true, but has var_mapping)
        let input = "item.name";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let br = make_bindings(&[("item", 0, 4, true)]);
        let map = FxHashMap::default();
        let var_mappings = vec![("item".to_string(), "_for_item0.value".to_string())];
        patch_bindings_with_var_mappings(&mut ct, Some(&br), &map, false, &var_mappings);
        assert_eq!(ct.to_string(), "_for_item0.value.name");
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_mixed() {
        // Expression: "item + count" at positions 0..12
        // item@0..4 (ignore:true, has var_mapping), count@7..12 (not ignored, unresolved)
        let input = "item + count";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let br = make_bindings(&[("item", 0, 4, true), ("count", 7, 12, false)]);
        let map = FxHashMap::default();
        let var_mappings = vec![("item".to_string(), "_for_item0.value".to_string())];
        patch_bindings_with_var_mappings(&mut ct, Some(&br), &map, false, &var_mappings);
        assert_eq!(ct.to_string(), "_for_item0.value + _ctx.count");
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_empty_bindings() {
        let input = "42";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        patch_bindings_with_var_mappings(&mut ct, None, &FxHashMap::default(), false, &[]);
        assert_eq!(ct.to_string(), "42");
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_ignored_no_mapping() {
        // Expression: "x" at 0..1, ignore:true, no var_mapping → skip entirely
        let input = "x";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let br = make_bindings(&[("x", 0, 1, true)]);
        patch_bindings_with_var_mappings(&mut ct, Some(&br), &FxHashMap::default(), false, &[]);
        assert_eq!(ct.to_string(), "x");
    }

    #[test]
    fn test_patch_bindings_with_var_mappings_with_binding_type() {
        use crate::syntax_kai::binding_types::BindingType;
        // Expression: "count" at 0..5, resolved as SetupRef
        let input = "count";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let br = make_bindings(&[("count", 0, 5, false)]);
        let mut map = FxHashMap::default();
        map.insert("count" as &str, BindingType::SetupRef);
        patch_bindings_with_var_mappings(&mut ct, Some(&br), &map, false, &[]);
        assert_eq!(ct.to_string(), "$setup.count");
    }

    // ── patch_vfor_references tests ─────────────────────────────────────

    #[test]
    fn test_patch_vfor_references_basic() {
        // Expression: "items" at position 10..15 in the source
        let input = "xxxxxxxxxx".to_string() + "items";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(&input, &alloc);
        let refs = vec![Span { start: 10, end: 15 }];
        patch_vfor_references(&mut ct, &refs, None, &input, &FxHashMap::default(), false);
        assert_eq!(&ct.to_string()[10..], "_ctx.items");
    }

    #[test]
    fn test_patch_vfor_references_with_filter_range() {
        // Two refs: one inside filter range, one outside
        let input = "xxxxxxxxxxitems + other";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let refs = vec![
            Span { start: 10, end: 15 }, // "items"
            Span { start: 18, end: 23 }, // "other"
        ];
        // Only prefix refs within range 10..16 (includes "items" but not "other")
        patch_vfor_references(
            &mut ct,
            &refs,
            Some((10, 16)),
            input,
            &FxHashMap::default(),
            false,
        );
        let result = ct.to_string();
        assert!(result.contains("_ctx.items"));
        assert!(!result.contains("_ctx.other"));
    }

    #[test]
    fn test_patch_vfor_references_empty_refs() {
        let input = "hello";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        patch_vfor_references(&mut ct, &[], None, input, &FxHashMap::default(), false);
        assert_eq!(ct.to_string(), "hello");
    }

    #[test]
    fn test_patch_vfor_references_with_binding_type() {
        use crate::syntax_kai::binding_types::BindingType;
        let input = "xxxxxxxxxxitems";
        let alloc = Allocator::default();
        let mut ct = crate::code_transform::CodeTransform::new(input, &alloc);
        let refs = vec![Span { start: 10, end: 15 }];
        let mut bindings = FxHashMap::default();
        bindings.insert("items" as &str, BindingType::SetupRef);
        patch_vfor_references(&mut ct, &refs, None, input, &bindings, false);
        assert_eq!(&ct.to_string()[10..], "$setup.items");
    }

    #[test]
    fn test_classify_modifier() {
        // Listener options
        assert_eq!(classify_modifier("capture"), ModifierKind::ListenerOption);
        assert_eq!(classify_modifier("once"), ModifierKind::ListenerOption);
        assert_eq!(classify_modifier("passive"), ModifierKind::ListenerOption);
        // Key filters
        assert_eq!(classify_modifier("enter"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("tab"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("esc"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("space"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("up"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("down"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("left"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("right"), ModifierKind::KeyFilter);
        assert_eq!(classify_modifier("delete"), ModifierKind::KeyFilter);
        // Runtime
        assert_eq!(classify_modifier("stop"), ModifierKind::Runtime);
        assert_eq!(classify_modifier("prevent"), ModifierKind::Runtime);
        assert_eq!(classify_modifier("self"), ModifierKind::Runtime);
        assert_eq!(classify_modifier("exact"), ModifierKind::Runtime);
    }
}
