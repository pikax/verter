use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform, common::Span, syntax_kai::binding_types::BindingType,
    utils::oxc::BindingExtractionResult,
};

pub fn patch_bindings<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    bindings: &Option<BindingExtractionResult<'alloc>>,
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

/// Apply accessor prefixes to identifiers inside a dynamic arg expression string.
///
/// For dynamic args like `:[foo]` or `v-slot:[foo]`, the arg text includes brackets
/// (e.g. `[foo]`). Identifiers within need accessor prefixes (e.g. `_ctx.` or `$setup.`).
///
/// This builds a new string with prefixes inserted, because the arg text is used
/// inside `overwrite` calls (not at its original source position).
///
/// `arg_start` is the absolute position of the arg span start in the original source,
/// used to compute offsets for each binding within the arg text.
pub fn apply_dynamic_arg_prefix(
    raw: &str,
    arg_start: u32,
    bindings_result: &Option<BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
) -> String {
    let Some(br) = bindings_result else {
        return raw.to_string();
    };

    // Collect non-ignored bindings with their offsets within the arg text, sorted by position
    let mut patches: Vec<(usize, &str)> = Vec::new();
    for b in &br.bindings {
        if b.ignore {
            continue;
        }
        let prefix = if let Some(bt) = map.get(b.name) {
            bt.accessor_prefix(is_inline)
        } else {
            "_ctx."
        };
        if prefix.is_empty() {
            continue;
        }
        // b.span.start is the absolute position in the original source
        let offset = (b.span.start - arg_start) as usize;
        patches.push((offset, prefix));
    }

    if patches.is_empty() {
        return raw.to_string();
    }

    // Sort by offset (should already be in order, but be safe)
    patches.sort_by_key(|&(off, _)| off);

    // Build the result string with prefixes inserted
    let mut result = String::with_capacity(raw.len() + patches.len() * 6);
    let mut last = 0;
    for (offset, prefix) in &patches {
        result.push_str(&raw[last..*offset]);
        result.push_str(prefix);
        last = *offset;
    }
    result.push_str(&raw[last..]);
    result
}

/// Build a value string with accessor prefixes applied, for use in `overwrite` calls
/// where we can't use `patch_bindings` (because the position falls inside an overwritten range).
///
/// This reads binding positions relative to `val_start` in the original source and inserts
/// accessor prefixes at the corresponding offsets in the value text.
///
/// Shares the same implementation as [`apply_dynamic_arg_prefix`] because both need to
/// insert prefixes at binding offsets within a string. The separate function exists for
/// semantic clarity: `apply_dynamic_arg_prefix` is for dynamic arg expressions like `[foo]`,
/// while this function is for value expressions in overwrite contexts.
pub fn build_prefixed_value(
    val_text: &str,
    val_start: u32,
    bindings_result: &Option<BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
) -> String {
    apply_dynamic_arg_prefix(val_text, val_start, bindings_result, map, is_inline)
}

/// Build a value string with both accessor prefixes AND variable replacements applied.
///
/// This is a unified version of `build_prefixed_value` + `apply_var_mappings` that uses
/// OXC-parsed binding positions for precise identifier handling instead of naive string
/// replacement. This correctly handles:
/// - Identifiers inside string literals (won't be in the binding list)
/// - Unicode identifiers (OXC parser handles these)
/// - v-for/v-slot variable mappings (e.g., `item` → `_for_item0.value`)
///
/// For each OXC-identified binding:
/// - If it matches a variable mapping key → replace the entire identifier
/// - Otherwise → insert the accessor prefix as normal
pub fn build_prefixed_value_with_var_mappings(
    val_text: &str,
    val_start: u32,
    bindings_result: &Option<BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
    var_mappings: &[(String, String)],
) -> String {
    let Some(br) = bindings_result else {
        return val_text.to_string();
    };

    // Collect patches: (offset_in_val, identifier_len, replacement_text)
    // A "replacement" can be either a prefix insertion (len=0 insert) or a full replacement.
    enum Patch<'a> {
        Prefix {
            offset: usize,
            prefix: &'a str,
        },
        Replace {
            offset: usize,
            len: usize,
            replacement: String,
        },
    }

    let mut patches: Vec<Patch> = Vec::new();
    for b in &br.bindings {
        let offset = (b.span.start - val_start) as usize;
        let ident_len = (b.span.end - b.span.start) as usize;

        // Check variable mappings first (v-for/v-slot variables take precedence).
        // This must happen BEFORE the `ignore` check because v-for/v-slot locals
        // are marked `ignore: true` by OXC (they're in the BindingContext's
        // ignored_identifiers set), but still need to be replaced with their
        // mapped values (e.g., `item` → `_for_item0.value`).
        if let Some((_, mapped)) = var_mappings.iter().find(|(orig, _)| orig == b.name) {
            patches.push(Patch::Replace {
                offset,
                len: ident_len,
                replacement: mapped.clone(),
            });
        } else if b.ignore {
            // Ignored binding with no var_mapping — skip entirely (no prefix needed).
            continue;
        } else {
            // Normal accessor prefix.
            let prefix = if let Some(bt) = map.get(b.name) {
                bt.accessor_prefix(is_inline)
            } else {
                "_ctx."
            };
            if !prefix.is_empty() {
                patches.push(Patch::Prefix { offset, prefix });
            }
        }
    }

    if patches.is_empty() {
        return val_text.to_string();
    }

    // Sort by offset (should already be in order, but be safe).
    patches.sort_by_key(|p| match p {
        Patch::Prefix { offset, .. } | Patch::Replace { offset, .. } => *offset,
    });

    // Build result string.
    let mut result = String::with_capacity(val_text.len() + patches.len() * 8);
    let mut last = 0;
    for patch in &patches {
        match patch {
            Patch::Prefix { offset, prefix } => {
                result.push_str(&val_text[last..*offset]);
                result.push_str(prefix);
                last = *offset;
            }
            Patch::Replace {
                offset,
                len,
                replacement,
            } => {
                result.push_str(&val_text[last..*offset]);
                result.push_str(replacement);
                last = offset + len;
            }
        }
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
    let mut result_str = text.to_string();
    let mut refs: Vec<_> = references.iter().collect();
    refs.sort_by(|a, b| b.start.cmp(&a.start));
    for r in refs {
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
            result_str.insert_str(offset, prefix);
        }
    }
    result_str
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
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut out = String::with_capacity(s.len());
            for upper in c.to_uppercase() {
                out.push(upper);
            }
            out.extend(chars);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
