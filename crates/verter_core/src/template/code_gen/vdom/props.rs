//! VDOM prop serialization utilities.
//!
//! Provides pure helper functions for transforming HTML attributes and Vue
//! directives into JavaScript object properties in the VNode call.
//!
//! - `camelize` — hyphenated → camelCase conversion
//! - `format_event_handler_key` — event arg name → `onClick` style handler key
//! - `needs_quoted_key` — check if a JS object key needs quoting in a JS object literal
//! - `compute_patch_flags` — derive patch flag from pre-computed metadata
//!
//! The element-level orchestration (building the full `{ key: value, ... }`
//! object with overwrites) lives in [`super::element`].

use crate::ast::types::{ChildrenMode, PropFlag, PropFlags};
use crate::template::oxc::types::{ExpressionFlag, ExpressionFlags};

use super::super::shared::helpers;

// ======================== String transformation ========================

/// Convert a hyphenated string to camelCase.
///
/// - `"my-event"` → `"myEvent"`
/// - `"foo-bar-baz"` → `"fooBarBaz"`
/// - `"click"` → `"click"` (unchanged)
/// - `"-leading"` → `"Leading"` (leading dash capitalizes)
pub fn camelize(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('-') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    std::borrow::Cow::Owned(result)
}

/// Convert an event argument name to a JS handler property key.
///
/// Applies Vue's event naming convention: `"on"` + capitalize-first + camelize.
///
/// - `"click"` → `"onClick"`
/// - `"my-event"` → `"onMyEvent"`
/// - `"update:modelValue"` → `"onUpdate:modelValue"`
/// - `"keyup"` → `"onKeyup"`
#[cfg(test)]
pub fn format_event_handler_key(event_name: &str) -> String {
    let mut result = String::with_capacity(event_name.len() + 2);
    format_event_handler_key_into(&mut result, event_name);
    result
}

/// Append a formatted event handler key to an existing buffer.
///
/// Same transformation as [`format_event_handler_key`] but avoids allocation.
pub fn format_event_handler_key_into(buf: &mut String, event_name: &str) {
    buf.push_str("on");
    let mut capitalize_next = true;
    for ch in event_name.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                buf.push(upper);
            }
            capitalize_next = false;
        } else {
            buf.push(ch);
        }
    }
}

// ======================== JS key validation ========================

/// Whether a property key needs to be quoted in a JS object literal.
///
/// Bare identifiers (`class`, `onClick`) don't need quotes.
/// Keys with special characters need quotes: `"data-id"`, `"onUpdate:modelValue"`.
pub fn needs_quoted_key(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let bytes = key.as_bytes();
    let first = bytes[0];
    // Must start with letter, _, or $
    if !first.is_ascii_alphabetic() && first != b'_' && first != b'$' {
        return true;
    }
    // Rest must be alphanumeric, _, or $
    bytes[1..]
        .iter()
        .any(|&b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'$')
}

// ======================== Static style parsing ========================

/// Emit a static CSS style string as a JavaScript object literal.
///
/// Vue's compiler parses `style="margin-top: 15px; color: red"` into
/// `{ "margin-top": "15px", color: "red" }` so that the SSR renderer
/// serializes it in a normalized compact form.
///
/// Writes `{ "prop": "val", ... }` directly into `buf`.
/// Falls back to a quoted string if the style text can't be parsed.
pub fn emit_static_style_object(buf: &mut String, style: &str) {
    let trimmed = style.trim();
    if trimmed.is_empty() {
        buf.push_str("{}");
        return;
    }

    buf.push_str("{ ");
    let mut first = true;
    for decl in trimmed.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        // Split at the first colon — property name vs value
        if let Some(colon_pos) = decl.find(':') {
            let prop = decl[..colon_pos].trim();
            let val = decl[colon_pos + 1..].trim();
            if prop.is_empty() || val.is_empty() {
                continue;
            }
            if !first {
                buf.push_str(", ");
            }
            first = false;
            // Quote the property key if it contains hyphens (CSS properties)
            if needs_quoted_key(prop) {
                buf.push('"');
                helpers::escape_js_string_into(buf, prop);
                buf.push('"');
            } else {
                buf.push_str(prop);
            }
            buf.push_str(": \"");
            helpers::escape_js_string_into(buf, val);
            buf.push('"');
        }
    }
    buf.push_str(" }");
}

// ======================== Patch flag computation ========================

/// Compute the VDOM patch flag value from pre-computed element metadata.
///
/// Combines:
/// - `PropFlag` (syntax layer): which props exist and their static/dynamic nature
/// - `ExpressionFlag` (OXC layer): expression-level static analysis results
/// - `ChildrenMode`: children classification
/// - `has_other_dynamic_binds`: whether there are dynamic binds beyond `:class`/`:style`
///   (determined by element.rs during prop iteration)
///
/// Returns the `u32` patch flag value. Fragment flags (`STABLE_FRAGMENT`,
/// `KEYED_FRAGMENT`, etc.) are handled by the element/directive layer, not here.
///
/// Rules:
/// 1. Spread (`v-bind="obj"` / `v-on="obj"`) → `FULL_PROPS` (overrides CLASS/STYLE/PROPS)
/// 2. Dynamic `:class` (not statically analyzed) → `CLASS`
/// 3. Dynamic `:style` (not statically analyzed) → `STYLE`
/// 4. Other dynamic binds or events → `PROPS`
/// 5. Text-only dynamic children → `TEXT`
/// 6. `ref` attribute → `NEED_HYDRATION`
pub fn compute_patch_flags(
    prop_flag: PropFlag,
    expr_flag: ExpressionFlag,
    children_mode: ChildrenMode,
) -> u32 {
    // Spread overrides individual CLASS/STYLE/PROPS flags
    if prop_flag.has_spread() {
        let mut flag = helpers::PATCH_FULL_PROPS;
        // TEXT is additive even with spread
        if matches!(children_mode, ChildrenMode::TextOnlyDynamic)
            && !expr_flag.has(ExpressionFlags::AllInterpolationsStatic)
        {
            flag |= helpers::PATCH_TEXT;
        }
        if prop_flag.has(PropFlags::HasRef) {
            flag |= helpers::PATCH_NEED_HYDRATION;
        }
        return flag;
    }

    let mut flag = 0u32;

    // TEXT: dynamic text children (interpolation present, not all static)
    if matches!(children_mode, ChildrenMode::TextOnlyDynamic)
        && !expr_flag.has(ExpressionFlags::AllInterpolationsStatic)
    {
        flag |= helpers::PATCH_TEXT;
    }

    // CLASS: dynamic class binding (not statically analyzed)
    if prop_flag.has(PropFlags::HasDynamicClass) && !expr_flag.has(ExpressionFlags::StaticClassExpr)
    {
        flag |= helpers::PATCH_CLASS;
    }

    // STYLE: dynamic style binding (not statically analyzed)
    if prop_flag.has(PropFlags::HasDynamicStyle) && !expr_flag.has(ExpressionFlags::StaticStyleExpr)
    {
        flag |= helpers::PATCH_STYLE;
    }

    // PROPS: dynamic binds (beyond class/style) or events
    if prop_flag.has(PropFlags::HasDynamicBinding) || prop_flag.has(PropFlags::HasEventListener) {
        flag |= helpers::PATCH_PROPS;
    }

    // NEED_HYDRATION: ref attribute
    if prop_flag.has(PropFlags::HasRef) {
        flag |= helpers::PATCH_NEED_HYDRATION;
    }

    flag
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== camelize ====================

    #[test]
    fn camelize_no_hyphens() {
        assert_eq!(camelize("click"), "click");
    }

    #[test]
    fn camelize_single_hyphen() {
        assert_eq!(camelize("my-event"), "myEvent");
    }

    #[test]
    fn camelize_multiple_hyphens() {
        assert_eq!(camelize("foo-bar-baz"), "fooBarBaz");
    }

    #[test]
    fn camelize_leading_hyphen() {
        assert_eq!(camelize("-leading"), "Leading");
    }

    #[test]
    fn camelize_trailing_hyphen() {
        assert_eq!(camelize("trailing-"), "trailing");
    }

    #[test]
    fn camelize_empty() {
        assert_eq!(camelize(""), "");
    }

    #[test]
    fn camelize_preserves_colons() {
        assert_eq!(camelize("update:modelValue"), "update:modelValue");
    }

    // ==================== format_event_handler_key ====================

    #[test]
    fn event_key_click() {
        assert_eq!(format_event_handler_key("click"), "onClick");
    }

    #[test]
    fn event_key_hyphenated() {
        assert_eq!(format_event_handler_key("my-event"), "onMyEvent");
    }

    #[test]
    fn event_key_with_colon() {
        assert_eq!(
            format_event_handler_key("update:modelValue"),
            "onUpdate:modelValue"
        );
    }

    #[test]
    fn event_key_keyup() {
        assert_eq!(format_event_handler_key("keyup"), "onKeyup");
    }

    #[test]
    fn event_key_empty() {
        assert_eq!(format_event_handler_key(""), "on");
    }

    #[test]
    fn event_key_multi_hyphen() {
        assert_eq!(
            format_event_handler_key("my-custom-event"),
            "onMyCustomEvent"
        );
    }

    #[test]
    fn event_key_into_appends() {
        let mut buf = String::from("prefix:");
        format_event_handler_key_into(&mut buf, "click");
        assert_eq!(buf, "prefix:onClick");
    }

    // ==================== needs_quoted_key ====================

    #[test]
    fn quoted_key_simple_identifiers() {
        assert!(!needs_quoted_key("class"));
        assert!(!needs_quoted_key("onClick"));
        assert!(!needs_quoted_key("_private"));
        assert!(!needs_quoted_key("$data"));
    }

    #[test]
    fn quoted_key_with_hyphen() {
        assert!(needs_quoted_key("data-id"));
        assert!(needs_quoted_key("my-prop"));
    }

    #[test]
    fn quoted_key_with_colon() {
        assert!(needs_quoted_key("onUpdate:modelValue"));
    }

    #[test]
    fn quoted_key_starts_with_digit() {
        assert!(needs_quoted_key("0abc"));
    }

    #[test]
    fn quoted_key_empty() {
        assert!(needs_quoted_key(""));
    }

    #[test]
    fn quoted_key_alphanumeric() {
        assert!(!needs_quoted_key("prop1"));
        assert!(!needs_quoted_key("a123"));
    }

    // ==================== emit_static_style_object ====================

    fn style_to_obj(style: &str) -> String {
        let mut buf = String::new();
        emit_static_style_object(&mut buf, style);
        buf
    }

    #[test]
    fn style_obj_simple() {
        assert_eq!(style_to_obj("color: red"), r#"{ color: "red" }"#);
    }

    #[test]
    fn style_obj_hyphenated_key() {
        assert_eq!(
            style_to_obj("margin-top: 15px"),
            r#"{ "margin-top": "15px" }"#
        );
    }

    #[test]
    fn style_obj_multiple() {
        assert_eq!(
            style_to_obj("margin-top: 15px; color: red"),
            r#"{ "margin-top": "15px", color: "red" }"#
        );
    }

    #[test]
    fn style_obj_empty() {
        assert_eq!(style_to_obj(""), "{}");
        assert_eq!(style_to_obj("   "), "{}");
    }

    #[test]
    fn style_obj_trailing_semicolon() {
        assert_eq!(style_to_obj("color: red;"), r#"{ color: "red" }"#);
    }

    #[test]
    fn style_obj_newlines_in_key() {
        // Matches Vue official: { "{\n        padding": "'20px'" }
        // The key must have newlines escaped in the JS string
        let style = "{\n        padding: '20px'";
        let result = style_to_obj(style);
        assert!(
            !result.contains('\n'),
            "output must not contain literal newlines: {result:?}"
        );
        assert_eq!(
            result,
            r#"{ "{\\n        padding": "'20px'" }"#.replace("\\\\n", "\\n")
        );
    }

    #[test]
    fn style_obj_quotes_in_key() {
        // Key containing a double quote must be escaped
        let style = r#"foo"bar: baz"#;
        let result = style_to_obj(style);
        assert!(
            !result.contains(r#"foo"bar"#),
            "output must escape quotes in key: {result:?}"
        );
    }

    #[test]
    fn style_obj_backslash_in_key() {
        // Key containing a backslash must be escaped
        let style = r"foo\bar: baz";
        let result = style_to_obj(style);
        assert!(
            result.contains(r"foo\\bar"),
            "output must escape backslashes in key: {result:?}"
        );
    }

    #[test]
    fn style_obj_newlines_in_value() {
        // Values are already escaped through escape_js_string_into
        let style = "color: red\nblue";
        let result = style_to_obj(style);
        assert!(
            !result.contains('\n'),
            "output must not contain literal newlines in value: {result:?}"
        );
    }

    // ==================== compute_patch_flags ====================

    #[test]
    fn patch_flags_empty() {
        let flag = compute_patch_flags(
            PropFlag::empty(),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_text_only_dynamic() {
        let flag = compute_patch_flags(
            PropFlag::empty(),
            ExpressionFlag::empty(),
            ChildrenMode::TextOnlyDynamic,
        );
        assert_eq!(flag, helpers::PATCH_TEXT);
    }

    #[test]
    fn patch_flags_text_dynamic_but_all_static_interps() {
        let flag = compute_patch_flags(
            PropFlag::empty(),
            ExpressionFlag::empty().add(ExpressionFlags::AllInterpolationsStatic),
            ChildrenMode::TextOnlyDynamic,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_dynamic_class() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasDynamicClass),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_CLASS);
    }

    #[test]
    fn patch_flags_dynamic_class_static_expr() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasDynamicClass),
            ExpressionFlag::empty().add(ExpressionFlags::StaticClassExpr),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_dynamic_style() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasDynamicStyle),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_STYLE);
    }

    #[test]
    fn patch_flags_dynamic_style_static_expr() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasDynamicStyle),
            ExpressionFlag::empty().add(ExpressionFlags::StaticStyleExpr),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_event_listener() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasEventListener),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_PROPS);
    }

    #[test]
    fn patch_flags_dynamic_binding() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasDynamicBinding),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_PROPS);
    }

    #[test]
    fn patch_flags_ref() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasRef),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_NEED_HYDRATION);
    }

    #[test]
    fn patch_flags_spread_overrides() {
        let flag = compute_patch_flags(
            PropFlag::empty()
                .add(PropFlags::HasBindSpread)
                .add(PropFlags::HasDynamicClass)
                .add(PropFlags::HasEventListener),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        // Spread → FULL_PROPS only (CLASS/PROPS overridden)
        assert_eq!(flag, helpers::PATCH_FULL_PROPS);
    }

    #[test]
    fn patch_flags_on_spread_overrides() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasOnSpread),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_FULL_PROPS);
    }

    #[test]
    fn patch_flags_spread_with_text() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasBindSpread),
            ExpressionFlag::empty(),
            ChildrenMode::TextOnlyDynamic,
        );
        assert_eq!(flag, helpers::PATCH_FULL_PROPS | helpers::PATCH_TEXT);
    }

    #[test]
    fn patch_flags_spread_with_ref() {
        let flag = compute_patch_flags(
            PropFlag::empty()
                .add(PropFlags::HasBindSpread)
                .add(PropFlags::HasRef),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(
            flag,
            helpers::PATCH_FULL_PROPS | helpers::PATCH_NEED_HYDRATION
        );
    }

    #[test]
    fn patch_flags_combined_class_style() {
        let flag = compute_patch_flags(
            PropFlag::empty()
                .add(PropFlags::HasDynamicClass)
                .add(PropFlags::HasDynamicStyle),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, helpers::PATCH_CLASS | helpers::PATCH_STYLE);
    }

    #[test]
    fn patch_flags_combined_text_and_props() {
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasEventListener),
            ExpressionFlag::empty(),
            ChildrenMode::TextOnlyDynamic,
        );
        assert_eq!(flag, helpers::PATCH_TEXT | helpers::PATCH_PROPS);
    }

    #[test]
    fn patch_flags_text_only_static_no_flag() {
        let flag = compute_patch_flags(
            PropFlag::empty(),
            ExpressionFlag::empty(),
            ChildrenMode::TextOnlyStatic,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_static_class_no_flag() {
        // Static class only → no patch flag (it never changes)
        let flag = compute_patch_flags(
            PropFlag::empty().add(PropFlags::HasStaticClass),
            ExpressionFlag::empty(),
            ChildrenMode::Empty,
        );
        assert_eq!(flag, 0);
    }

    #[test]
    fn patch_flags_all_combined() {
        let flag = compute_patch_flags(
            PropFlag::empty()
                .add(PropFlags::HasDynamicClass)
                .add(PropFlags::HasDynamicStyle)
                .add(PropFlags::HasEventListener)
                .add(PropFlags::HasDynamicBinding)
                .add(PropFlags::HasRef),
            ExpressionFlag::empty(),
            ChildrenMode::TextOnlyDynamic,
        );
        assert_eq!(
            flag,
            helpers::PATCH_TEXT
                | helpers::PATCH_CLASS
                | helpers::PATCH_STYLE
                | helpers::PATCH_PROPS
                | helpers::PATCH_NEED_HYDRATION
        );
    }
}
