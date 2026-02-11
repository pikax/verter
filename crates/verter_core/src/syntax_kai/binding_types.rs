//! Binding type classification for the syntax_kai pipeline.
//!
//! These types are used by codegen plugins (VDOM, Vapor, TSX) to determine
//! the correct accessor prefix/suffix for template expressions.
//!
//! This is a NEW implementation separate from `codegen::vue::template::types`
//! which is used by the legacy pipeline.

use crate::common::Span;

/// Classification of a binding for correct accessor prefix in template codegen.
/// Expanded from the legacy 3-variant BindingType to match Vue's official compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    /// `const x = 'literal'` — literal value that never changes, can be inlined.
    SetupConst,
    /// `let x = ...` — reassignable variable.
    SetupLet,
    /// `const x = ref(...)` / `computed(...)` / `shallowRef(...)` — needs `.value` in inline mode.
    SetupRef,
    /// `const x = reactive({})` — mutable properties but identity is stable.
    SetupReactiveConst,
    /// `const x = useSomething()` — return value might be a ref.
    SetupMaybeRef,
    /// Literal value that can be inlined (e.g., string/number constant).
    LiteralConst,
    /// `defineProps` prop — accessed via `__props.x` (inline) or `$props.x` (standalone).
    Props,
    /// Destructured prop alias — `const { msg: m } = defineProps<...>()`.
    PropsAliased,
    /// `data()` return property (Options API).
    Data,
    /// `computed`/`inject`/etc. from Options API.
    Options,
}

impl BindingType {
    /// Whether this binding's value never changes (can skip patch flags / renderEffect).
    #[inline]
    pub fn reactivity_level(&self) -> ReactivityLevel {
        match self {
            BindingType::SetupConst | BindingType::LiteralConst => ReactivityLevel::Static,
            _ => ReactivityLevel::Dynamic,
        }
    }

    #[inline]
    pub fn accessor_prefix(&self, is_inline: bool) -> &'static str {
        match self {
            BindingType::Props | BindingType::PropsAliased => {
                if is_inline {
                    "__props."
                } else {
                    "$props."
                }
            }
            BindingType::SetupConst
            | BindingType::SetupLet
            | BindingType::SetupRef
            | BindingType::SetupReactiveConst
            | BindingType::SetupMaybeRef
            | BindingType::LiteralConst => {
                if is_inline {
                    ""
                } else {
                    "$setup."
                }
            }
            BindingType::Data | BindingType::Options => "_ctx.",
        }
    }

    /// Whether this binding needs `.value` access in inline mode.
    #[inline]
    pub fn needs_value_access(&self) -> bool {
        matches!(self, BindingType::SetupRef | BindingType::SetupMaybeRef)
    }

    /// Whether this is a setup-type binding (non-props, non-options).
    #[inline]
    pub fn is_setup(&self) -> bool {
        matches!(
            self,
            BindingType::SetupConst
                | BindingType::SetupLet
                | BindingType::SetupRef
                | BindingType::SetupReactiveConst
                | BindingType::SetupMaybeRef
                | BindingType::LiteralConst
        )
    }

    /// Whether this is a props-type binding.
    #[inline]
    pub fn is_props(&self) -> bool {
        matches!(self, BindingType::Props | BindingType::PropsAliased)
    }
}

/// Reactivity classification — used by codegen to decide:
/// - **VDOM**: Static → skip dynamic_props/patch flag. Dynamic → add to patch flag.
/// - **Vapor**: Static → one-time `_setProp()`. Dynamic → wrap in `_renderEffect()`.
/// - **TSX**: Static → inline literal. Dynamic → accessor with correct prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactivityLevel {
    /// Value never changes — can be inlined, no patch flag, no renderEffect.
    Static,
    /// Value may change — needs patch flag (VDOM) or renderEffect (Vapor).
    Dynamic,
}

/// Look up binding type by comparing identifier bytes against source spans.
pub fn get_binding_type(
    entries: &[(Span, BindingType)],
    ident: &[u8],
    source: &[u8],
) -> Option<BindingType> {
    entries
        .iter()
        .find(|(span, _)| &source[span.start as usize..span.end as usize] == ident)
        .map(|(_, bt)| *bt)
}

/// Resolve the correct accessor prefix for an identifier.
/// Returns a static `&str` — no allocation.
///
/// The `is_inline` flag controls the prefix style:
/// - **Inline mode** (`true`): Template is inlined inside setup() closure.
///   Props use `__props.`, setup bindings use bare identifier (no prefix).
/// - **Standalone mode** (`false`): Template is a separate `export function render(...)`.
///   Props use `$props.`, setup bindings use `$setup.`.
pub fn resolve_binding_prefix(
    ident: &[u8],
    entries: &[(Span, BindingType)],
    source: &[u8],
    is_inline: bool,
) -> &'static str {
    match get_binding_type(entries, ident, source) {
        Some(bt) if bt.is_props() => {
            if is_inline {
                "__props."
            } else {
                "$props."
            }
        }
        Some(bt) if bt.is_setup() => {
            if is_inline {
                ""
            } else {
                "$setup."
            }
        }
        Some(BindingType::Data | BindingType::Options) => "_ctx.",
        None => "_ctx.",
        // unreachable but needed for exhaustiveness since we use guards above
        _ => "_ctx.",
    }
}

/// Resolve the correct accessor suffix for an identifier.
/// Returns `.value` for ref-type bindings in inline mode, empty string otherwise.
///
/// In inline mode, ref-type bindings (created by `ref()`, `computed()`, etc.)
/// need `.value` appended to access the underlying value.
pub fn resolve_binding_suffix(
    ident: &[u8],
    entries: &[(Span, BindingType)],
    source: &[u8],
    is_inline: bool,
) -> &'static str {
    if !is_inline {
        return "";
    }
    match get_binding_type(entries, ident, source) {
        Some(bt) if bt.needs_value_access() => ".value",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build binding entries from a source string and list of (name, type) pairs.
    fn make_entries(source: &str, bindings: &[(&str, BindingType)]) -> Vec<(Span, BindingType)> {
        let mut entries = Vec::new();
        for (name, bt) in bindings {
            if let Some(start) = source.find(name) {
                entries.push((
                    Span {
                        start: start as u32,
                        end: (start + name.len()) as u32,
                    },
                    *bt,
                ));
            }
        }
        entries
    }

    // ==================== ReactivityLevel ====================

    #[test]
    fn test_reactivity_level_static() {
        assert_eq!(
            BindingType::SetupConst.reactivity_level(),
            ReactivityLevel::Static
        );
        assert_eq!(
            BindingType::LiteralConst.reactivity_level(),
            ReactivityLevel::Static
        );
    }

    #[test]
    fn test_reactivity_level_dynamic() {
        assert_eq!(
            BindingType::SetupRef.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::SetupLet.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::SetupReactiveConst.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::SetupMaybeRef.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::Props.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::PropsAliased.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::Data.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::Options.reactivity_level(),
            ReactivityLevel::Dynamic
        );
    }

    // ==================== needs_value_access ====================

    #[test]
    fn test_needs_value_access() {
        assert!(BindingType::SetupRef.needs_value_access());
        assert!(BindingType::SetupMaybeRef.needs_value_access());
        assert!(!BindingType::SetupConst.needs_value_access());
        assert!(!BindingType::SetupLet.needs_value_access());
        assert!(!BindingType::Props.needs_value_access());
    }

    // ==================== is_setup / is_props ====================

    #[test]
    fn test_is_setup() {
        assert!(BindingType::SetupConst.is_setup());
        assert!(BindingType::SetupLet.is_setup());
        assert!(BindingType::SetupRef.is_setup());
        assert!(BindingType::SetupReactiveConst.is_setup());
        assert!(BindingType::SetupMaybeRef.is_setup());
        assert!(BindingType::LiteralConst.is_setup());
        assert!(!BindingType::Props.is_setup());
        assert!(!BindingType::Data.is_setup());
    }

    #[test]
    fn test_is_props() {
        assert!(BindingType::Props.is_props());
        assert!(BindingType::PropsAliased.is_props());
        assert!(!BindingType::SetupConst.is_props());
        assert!(!BindingType::Data.is_props());
    }

    // ==================== resolve_binding_prefix ====================

    #[test]
    fn test_props_standalone_uses_dollar_props_prefix() {
        let source = "title count";
        let entries = make_entries(source, &[("title", BindingType::Props)]);
        let prefix = resolve_binding_prefix(b"title", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "$props.");
    }

    #[test]
    fn test_props_inline_uses_dunder_props_prefix() {
        let source = "title count";
        let entries = make_entries(source, &[("title", BindingType::Props)]);
        let prefix = resolve_binding_prefix(b"title", &entries, source.as_bytes(), true);
        assert_eq!(prefix, "__props.");
    }

    #[test]
    fn test_props_aliased_uses_props_prefix() {
        let source = "msg";
        let entries = make_entries(source, &[("msg", BindingType::PropsAliased)]);
        let prefix = resolve_binding_prefix(b"msg", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "$props.");
    }

    #[test]
    fn test_setup_const_standalone_uses_setup_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupConst)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "$setup.");
    }

    #[test]
    fn test_setup_const_inline_uses_bare_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupConst)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), true);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_setup_ref_standalone_uses_setup_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupRef)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "$setup.");
    }

    #[test]
    fn test_setup_ref_inline_uses_bare_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupRef)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), true);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_data_uses_ctx_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::Data)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "_ctx.");
    }

    #[test]
    fn test_options_uses_ctx_prefix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::Options)]);
        let prefix = resolve_binding_prefix(b"count", &entries, source.as_bytes(), true);
        assert_eq!(prefix, "_ctx.");
    }

    #[test]
    fn test_unknown_binding_uses_ctx_prefix() {
        let source = "title count";
        let entries: Vec<(Span, BindingType)> = Vec::new();
        let prefix = resolve_binding_prefix(b"unknown", &entries, source.as_bytes(), false);
        assert_eq!(prefix, "_ctx.");
    }

    // ==================== resolve_binding_suffix ====================

    #[test]
    fn test_setup_ref_inline_has_value_suffix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupRef)]);
        let suffix = resolve_binding_suffix(b"count", &entries, source.as_bytes(), true);
        assert_eq!(suffix, ".value");
    }

    #[test]
    fn test_setup_maybe_ref_inline_has_value_suffix() {
        let source = "data";
        let entries = make_entries(source, &[("data", BindingType::SetupMaybeRef)]);
        let suffix = resolve_binding_suffix(b"data", &entries, source.as_bytes(), true);
        assert_eq!(suffix, ".value");
    }

    #[test]
    fn test_setup_ref_standalone_has_no_suffix() {
        let source = "count";
        let entries = make_entries(source, &[("count", BindingType::SetupRef)]);
        let suffix = resolve_binding_suffix(b"count", &entries, source.as_bytes(), false);
        assert_eq!(suffix, "");
    }

    #[test]
    fn test_setup_const_inline_has_no_suffix() {
        let source = "myFunc";
        let entries = make_entries(source, &[("myFunc", BindingType::SetupConst)]);
        let suffix = resolve_binding_suffix(b"myFunc", &entries, source.as_bytes(), true);
        assert_eq!(suffix, "");
    }

    #[test]
    fn test_literal_const_inline_has_no_suffix() {
        let source = "msg";
        let entries = make_entries(source, &[("msg", BindingType::LiteralConst)]);
        let suffix = resolve_binding_suffix(b"msg", &entries, source.as_bytes(), true);
        assert_eq!(suffix, "");
    }
}
