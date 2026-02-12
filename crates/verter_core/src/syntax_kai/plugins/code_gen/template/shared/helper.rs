use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform, syntax_kai::binding_types::BindingType,
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
pub fn build_prefixed_value(
    val_text: &str,
    val_start: u32,
    bindings_result: &Option<BindingExtractionResult>,
    map: &FxHashMap<&str, BindingType>,
    is_inline: bool,
) -> String {
    apply_dynamic_arg_prefix(val_text, val_start, bindings_result, map, is_inline)
}

/// Escape a string for use inside a JavaScript string literal (double-quoted).
pub fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
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
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("click"), "Click");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
        assert_eq!(capitalize_first("Click"), "Click");
    }
}
