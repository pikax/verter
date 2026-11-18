//! VDOM prop serialization utilities.
//!
//! Provides pure helper functions for transforming HTML attributes and Vue
//! directives into JavaScript object properties in the VNode call.
//!
//! - `compute_patch_flags` — derive patch flag from pre-computed metadata
//!
//! The element-level orchestration (building the full `{ key: value, ... }`
//! object with overwrites) lives in [`super::element`].

use crate::ast::types::{ChildrenMode, PropFlag, PropFlags};
use crate::template::oxc::types::{ExpressionFlag, ExpressionFlags};

use crate::template::code_gen::shared::helpers;
use crate::template::code_gen::shared::inline_style::parse_style_declarations;

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
    // Collect declarations into a Vec, deduplicating keys (last wins, matching CSS cascade).
    // This matches Vue's parseStringStyle which uses a plain JS object for the same effect.
    // Declarations are read from the shared CSS declaration-list parse
    // entry point (`super::super::shared::inline_style`), never re-derived
    // from raw bytes here — a hand-rolled `;`/`:` scan cannot tell a
    // statement-separating `;` from one inside a quoted string value.
    let mut entries: Vec<(&str, &str)> = Vec::new();
    for (prop, val) in parse_style_declarations(style) {
        if prop.is_empty() || val.is_empty() {
            continue;
        }
        // Overwrite existing entry with same key (last wins)
        if let Some(existing) = entries.iter_mut().find(|(k, _)| *k == prop) {
            existing.1 = val;
        } else {
            entries.push((prop, val));
        }
    }

    if entries.is_empty() {
        buf.push_str("{}");
        return;
    }

    buf.push_str("{ ");
    for (i, (prop, val)) in entries.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        // Official Vue always quotes style-object keys as strings — even
        // valid-identifier CSS properties like `color` — unlike a general
        // props object, where only keys that need it are quoted.
        buf.push('"');
        crate::template::vue_syntax::escape_js_string_into(buf, prop);
        buf.push('"');
        buf.push_str(": \"");
        crate::template::vue_syntax::escape_js_string_into(buf, val);
        buf.push('"');
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

    // ==================== emit_static_style_object ====================

    fn style_to_obj(style: &str) -> String {
        let mut buf = String::new();
        emit_static_style_object(&mut buf, style);
        buf
    }

    #[test]
    fn style_obj_simple() {
        // Official Vue quotes EVERY style-object key as a string, even a
        // valid identifier like `color` — verified against
        // `elements-text/static-class-style.js`'s golden.
        assert_eq!(style_to_obj("color: red"), r#"{ "color": "red" }"#);
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
            r#"{ "margin-top": "15px", "color": "red" }"#
        );
    }

    #[test]
    fn style_obj_empty() {
        assert_eq!(style_to_obj(""), "{}");
        assert_eq!(style_to_obj("   "), "{}");
    }

    #[test]
    fn style_obj_trailing_semicolon() {
        assert_eq!(style_to_obj("color: red;"), r#"{ "color": "red" }"#);
    }

    #[test]
    fn style_obj_unparseable_garbage_yields_empty_object() {
        // A bare `{` is not valid CSS declaration text — no property name
        // can contain it. The naive `split(';')`/`find(':')` scanner this
        // superseded didn't know that and silently smuggled the `{` and the
        // newline after it into a fabricated key
        // (`"{\n        padding": "'20px'"`); the shared grammar-aware
        // parser correctly finds no valid declaration here and degrades to
        // an empty object, rather than emitting garbage as if it were real
        // CSS.
        let style = "{\n        padding: '20px'";
        let result = style_to_obj(style);
        assert!(
            !result.contains('\n'),
            "output must not contain literal newlines: {result:?}"
        );
        assert_eq!(result, "{}");
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

    #[test]
    fn style_obj_duplicate_keys_last_wins() {
        // CSS cascade: last declaration wins. Matches Vue's parseStringStyle (plain object).
        assert_eq!(
            style_to_obj("position: absolute; position: relative; width: 100%"),
            r#"{ "position": "relative", "width": "100%" }"#
        );
    }

    #[test]
    fn style_obj_duplicate_keys_three_values() {
        // Three declarations of same key → last wins
        assert_eq!(
            style_to_obj("color: red; color: blue; color: green"),
            r#"{ "color": "green" }"#
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
