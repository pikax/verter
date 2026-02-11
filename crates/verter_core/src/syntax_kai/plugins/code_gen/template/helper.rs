use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::binding_types::BindingType,
    utils::{
        oxc::BindingExtractionResult,
        vue::{PatchFlag, PatchFlags},
    },
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

/// Returns the prefix for a child node based on context.
///
/// - **Root template**: first child gets `"return "`, subsequent get `", "`
/// - **Element children**: ALL children get `", "` (separates from the props arg)
#[inline]
pub fn child_separator(is_root: bool, children_count: u16) -> &'static str {
    if is_root {
        if children_count > 0 {
            ", "
        } else {
            "return "
        }
    } else {
        ", "
    }
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

/// Build the patch flag + dynamic props suffix for an element close.
///
/// Returns something like `, 9 /* TEXT, PROPS */, ["id"]` or an empty string
/// if the patch flag is zero.
pub fn build_patch_flag_suffix(
    patch_flag: PatchFlag,
    dynamic_props: &[String],
    is_production: bool,
) -> String {
    if patch_flag.0 == 0 {
        return String::new();
    }

    let mut suffix = String::new();
    suffix.push_str(", ");

    // Numeric value
    suffix.push_str(&patch_flag.0.to_string());

    // Dev-mode comment with flag names
    if !is_production {
        suffix.push_str(" /* ");
        let names = patch_flag_names(patch_flag);
        suffix.push_str(&names.join(", "));
        suffix.push_str(" */");
    }

    // Dynamic props array
    if !dynamic_props.is_empty() {
        suffix.push_str(", [");
        for (i, prop) in dynamic_props.iter().enumerate() {
            if i > 0 {
                suffix.push_str(", ");
            }
            // Dynamic arg expressions (e.g. `"on" + _ctx.event`) already start
            // with `"` — emit them verbatim without extra quoting.
            if prop.starts_with('"') {
                suffix.push_str(prop);
            } else {
                suffix.push('"');
                suffix.push_str(prop);
                suffix.push('"');
            }
        }
        suffix.push(']');
    }

    suffix
}

/// Returns the list of flag names set in a PatchFlag bitmask.
fn patch_flag_names(flag: PatchFlag) -> Vec<&'static str> {
    if flag.is_special() {
        return vec![flag.name()];
    }

    let all_flags = [
        PatchFlags::Text,
        PatchFlags::Class,
        PatchFlags::Style,
        PatchFlags::Props,
        PatchFlags::FullProps,
        PatchFlags::NeedHydration,
        PatchFlags::StableFragment,
        PatchFlags::KeyedFragment,
        PatchFlags::UnkeyedFragment,
        PatchFlags::NeedPatch,
        PatchFlags::DynamicSlots,
        PatchFlags::DevRootFragment,
    ];

    let mut names = Vec::new();
    for f in all_flags {
        if flag.contains(f) {
            names.push(f.name());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_child_separator() {
        // Root: first child gets "return ", subsequent get ", "
        assert_eq!(child_separator(true, 0), "return ");
        assert_eq!(child_separator(true, 1), ", ");
        assert_eq!(child_separator(true, 5), ", ");

        // Element: all children get ", " (separates from props arg)
        assert_eq!(child_separator(false, 0), ", ");
        assert_eq!(child_separator(false, 1), ", ");
        assert_eq!(child_separator(false, 5), ", ");
    }

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

    #[test]
    fn test_build_patch_flag_suffix_empty() {
        assert_eq!(build_patch_flag_suffix(PatchFlag::empty(), &[], false), "");
    }

    #[test]
    fn test_build_patch_flag_suffix_single_flag_dev() {
        let flag = PatchFlags::Text.into_flag();
        let result = build_patch_flag_suffix(flag, &[], false);
        assert_eq!(result, ", 1 /* TEXT */");
    }

    #[test]
    fn test_build_patch_flag_suffix_combined_dev() {
        let flag = PatchFlags::Text.into_flag().add(PatchFlags::Props);
        let result = build_patch_flag_suffix(flag, &["id".to_string()], false);
        assert_eq!(result, ", 9 /* TEXT, PROPS */, [\"id\"]");
    }

    #[test]
    fn test_build_patch_flag_suffix_production() {
        let flag = PatchFlags::Text.into_flag().add(PatchFlags::Props);
        let result = build_patch_flag_suffix(flag, &["id".to_string()], true);
        assert_eq!(result, ", 9, [\"id\"]");
    }
}
