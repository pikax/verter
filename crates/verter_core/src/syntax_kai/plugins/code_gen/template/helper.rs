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
                }
            }
        });
    }
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
            suffix.push('"');
            suffix.push_str(prop);
            suffix.push('"');
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
