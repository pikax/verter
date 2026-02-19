//! Binding type classification and resolver for the new_impl codegen pipeline.
//!
//! This is independent of [`crate::syntax::binding_types`] — it lives entirely
//! within `new_impl` and is designed for the AST-based codegen.

use rustc_hash::FxHashMap;

use crate::utils::oxc::BindingExtractionResult;

use super::types::CodeGenOutput;

/// Classification of a binding for correct accessor prefix/suffix in template codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Whether this binding needs `.value` access in inline mode.
    #[inline]
    pub fn needs_value_access(&self) -> bool {
        matches!(self, BindingType::SetupRef | BindingType::SetupMaybeRef)
    }
}

/// Reactivity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactivityLevel {
    /// Value never changes — can be inlined, no patch flag, no renderEffect.
    Static,
    /// Value may change — needs patch flag (VDOM) or renderEffect (Vapor).
    Dynamic,
}

/// Resolves identifiers to their correct accessor prefix/suffix.
///
/// Holds the binding map (populated from script analysis) and the `is_inline`
/// flag. Provides methods to resolve individual identifiers and to batch-collect
/// binding patches for an expression's extracted bindings.
pub struct BindingResolver<'alloc> {
    bindings: FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
}

impl<'alloc> BindingResolver<'alloc> {
    /// Create a new resolver from a binding map and inline mode flag.
    pub fn new(bindings: FxHashMap<&'alloc str, BindingType>, is_inline: bool) -> Self {
        Self {
            bindings,
            is_inline,
        }
    }

    /// Look up the binding type for an identifier.
    #[inline]
    pub fn get(&self, ident: &str) -> Option<BindingType> {
        self.bindings.get(ident).copied()
    }

    /// Whether this resolver is in inline mode.
    #[inline]
    pub fn is_inline(&self) -> bool {
        self.is_inline
    }

    /// Resolve the accessor prefix for an identifier.
    ///
    /// - Props: `__props.` (inline) or `$props.` (standalone)
    /// - Setup bindings: `""` (inline) or `$setup.` (standalone)
    /// - Data/Options/Unresolved: `_ctx.`
    #[inline]
    pub fn resolve_prefix(&self, ident: &str) -> &'static str {
        match self.bindings.get(ident) {
            Some(bt) if bt.is_props() => {
                if self.is_inline {
                    "__props."
                } else {
                    "$props."
                }
            }
            Some(bt) if bt.is_setup() => {
                if self.is_inline {
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

    /// Resolve the accessor suffix for an identifier.
    ///
    /// Returns `.value` for `SetupRef` and `SetupMaybeRef` bindings in inline mode,
    /// empty string otherwise.
    #[inline]
    pub fn resolve_suffix(&self, ident: &str) -> &'static str {
        if !self.is_inline {
            return "";
        }
        match self.bindings.get(ident) {
            Some(bt) if bt.needs_value_access() => ".value",
            _ => "",
        }
    }

    /// Resolve a simple identifier expression to its prefixed/suffixed form.
    ///
    /// If the expression is a simple identifier (no dots, brackets, operators),
    /// returns `prefix + ident + suffix`. Otherwise returns the expression unchanged.
    ///
    /// This is useful for places that need a fully-resolved expression string
    /// but only have a raw expression (not OXC-parsed bindings).
    pub fn resolve_simple_expr(&self, expr: &str) -> String {
        let trimmed = expr.trim();
        if is_simple_ident(trimmed) {
            let prefix = self.resolve_prefix(trimmed);
            let suffix = self.resolve_suffix(trimmed);
            let mut result = String::with_capacity(prefix.len() + trimmed.len() + suffix.len());
            result.push_str(prefix);
            result.push_str(trimmed);
            result.push_str(suffix);
            result
        } else {
            trimmed.to_string()
        }
    }

    /// For each non-ignored identifier in the expression's extracted bindings,
    /// resolve its accessor prefix and suffix, then push the corresponding
    /// `(position, text)` tuples into the output's prepends vec.
    ///
    /// # Example
    ///
    /// Expression `foo + bar.x` where `foo` = `SetupRef` (inline), `bar` = `Props` (inline):
    ///
    /// Bindings extracted by OXC: `[{name:"foo", pos:10, len:3}, {name:"bar", pos:16, len:3}]`
    ///
    /// - For `"foo"` (`SetupRef`, inline): prefix = `""` (bare), suffix = `".value"`
    ///   → push `(13, ".value")` to prepends   (pos 10 + 3 = 13)
    ///
    /// - For `"bar"` (`Props`, inline): prefix = `"__props."`, suffix = `""`
    ///   → push `(16, "__props.")` to prepends
    ///
    /// All prefix/suffix strings are `&'static str` — zero allocation.
    pub fn collect_binding_patches(
        &self,
        bindings: &BindingExtractionResult<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        for binding in &bindings.bindings {
            if binding.ignore {
                continue; // v-for/v-slot locals — already in scope
            }

            let prefix = self.resolve_prefix(binding.name);
            if !prefix.is_empty() {
                out.prepend_static(binding.pos, prefix);
            }

            let suffix = self.resolve_suffix(binding.name);
            if !suffix.is_empty() {
                out.prepend_static(binding.pos + binding.name.len() as u32, suffix);
            }
        }
    }
}

/// Check if a string is a simple JavaScript identifier.
///
/// Returns `true` for identifiers like `foo`, `_bar`, `$baz`, `count123`.
/// Returns `false` for compound expressions, member access, etc.
pub fn is_simple_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== BindingType ====================

    #[test]
    fn reactivity_level_static_for_const() {
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
    fn reactivity_level_dynamic_for_ref() {
        assert_eq!(
            BindingType::SetupRef.reactivity_level(),
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
    }

    #[test]
    fn is_setup_true_for_setup_types() {
        assert!(BindingType::SetupConst.is_setup());
        assert!(BindingType::SetupLet.is_setup());
        assert!(BindingType::SetupRef.is_setup());
        assert!(BindingType::SetupReactiveConst.is_setup());
        assert!(BindingType::SetupMaybeRef.is_setup());
        assert!(BindingType::LiteralConst.is_setup());
    }

    #[test]
    fn is_setup_false_for_non_setup() {
        assert!(!BindingType::Props.is_setup());
        assert!(!BindingType::PropsAliased.is_setup());
        assert!(!BindingType::Data.is_setup());
        assert!(!BindingType::Options.is_setup());
    }

    #[test]
    fn is_props_correct() {
        assert!(BindingType::Props.is_props());
        assert!(BindingType::PropsAliased.is_props());
        assert!(!BindingType::SetupConst.is_props());
        assert!(!BindingType::Data.is_props());
    }

    #[test]
    fn needs_value_access_correct() {
        assert!(BindingType::SetupRef.needs_value_access());
        assert!(BindingType::SetupMaybeRef.needs_value_access());
        assert!(!BindingType::SetupConst.needs_value_access());
        assert!(!BindingType::SetupLet.needs_value_access());
        assert!(!BindingType::Props.needs_value_access());
    }

    // ==================== BindingResolver ====================

    fn make_resolver(
        entries: &[(&'static str, BindingType)],
        is_inline: bool,
    ) -> BindingResolver<'static> {
        let mut map = FxHashMap::default();
        for &(name, bt) in entries {
            map.insert(name, bt);
        }
        BindingResolver::new(map, is_inline)
    }

    // ---- resolve_prefix ----

    #[test]
    fn setup_ref_inline_prefix_is_empty() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
        assert_eq!(resolver.resolve_prefix("count"), "");
    }

    #[test]
    fn setup_ref_standalone_prefix_is_setup() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
        assert_eq!(resolver.resolve_prefix("count"), "$setup.");
    }

    #[test]
    fn props_inline_prefix_is_dunder_props() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], true);
        assert_eq!(resolver.resolve_prefix("msg"), "__props.");
    }

    #[test]
    fn props_standalone_prefix_is_dollar_props() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], false);
        assert_eq!(resolver.resolve_prefix("msg"), "$props.");
    }

    #[test]
    fn props_aliased_prefix_same_as_props() {
        let resolver = make_resolver(&[("m", BindingType::PropsAliased)], true);
        assert_eq!(resolver.resolve_prefix("m"), "__props.");
    }

    #[test]
    fn data_prefix_is_ctx() {
        let resolver = make_resolver(&[("count", BindingType::Data)], true);
        assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
    }

    #[test]
    fn options_prefix_is_ctx() {
        let resolver = make_resolver(&[("count", BindingType::Options)], false);
        assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
    }

    #[test]
    fn unknown_binding_prefix_is_ctx() {
        let resolver = make_resolver(&[], true);
        assert_eq!(resolver.resolve_prefix("unknown"), "_ctx.");
    }

    #[test]
    fn setup_const_inline_prefix_is_empty() {
        let resolver = make_resolver(&[("fn", BindingType::SetupConst)], true);
        assert_eq!(resolver.resolve_prefix("fn"), "");
    }

    #[test]
    fn setup_const_standalone_prefix_is_setup() {
        let resolver = make_resolver(&[("fn", BindingType::SetupConst)], false);
        assert_eq!(resolver.resolve_prefix("fn"), "$setup.");
    }

    // ---- resolve_suffix ----

    #[test]
    fn setup_ref_inline_suffix_is_value() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
        assert_eq!(resolver.resolve_suffix("count"), ".value");
    }

    #[test]
    fn setup_maybe_ref_inline_suffix_is_value() {
        let resolver = make_resolver(&[("data", BindingType::SetupMaybeRef)], true);
        assert_eq!(resolver.resolve_suffix("data"), ".value");
    }

    #[test]
    fn setup_ref_standalone_suffix_is_empty() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
        assert_eq!(resolver.resolve_suffix("count"), "");
    }

    #[test]
    fn setup_const_inline_suffix_is_empty() {
        let resolver = make_resolver(&[("fn", BindingType::SetupConst)], true);
        assert_eq!(resolver.resolve_suffix("fn"), "");
    }

    #[test]
    fn props_inline_suffix_is_empty() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], true);
        assert_eq!(resolver.resolve_suffix("msg"), "");
    }

    #[test]
    fn unknown_binding_suffix_is_empty() {
        let resolver = make_resolver(&[], true);
        assert_eq!(resolver.resolve_suffix("unknown"), "");
    }

    // ---- collect_binding_patches ----

    #[test]
    fn collect_patches_setup_ref_inline_adds_value_suffix() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        // Simulate a binding extracted by OXC: "count" at pos 10, len 5
        let bindings = BindingExtractionResult {
            bindings: vec![crate::utils::oxc::Binding {
                name: "count",
                span: crate::common::Span::new(10, 15),
                pos: 10,
                ignore: false,
                is_shorthand: false,
            }],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
        };

        resolver.collect_binding_patches(&bindings, &mut out);

        // SetupRef inline: prefix="" (empty, not pushed), suffix=".value" at pos 15
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0].0, 15); // pos 10 + len 5
        assert_eq!(out.prepends[0].1, ".value");
    }

    #[test]
    fn collect_patches_props_inline_adds_prefix() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], true);
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        let bindings = BindingExtractionResult {
            bindings: vec![crate::utils::oxc::Binding {
                name: "msg",
                span: crate::common::Span::new(5, 8),
                pos: 5,
                ignore: false,
                is_shorthand: false,
            }],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
        };

        resolver.collect_binding_patches(&bindings, &mut out);

        // Props inline: prefix="__props." at pos 5, suffix="" (not pushed)
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0].0, 5);
        assert_eq!(out.prepends[0].1, "__props.");
    }

    #[test]
    fn collect_patches_ignored_binding_skipped() {
        let resolver = make_resolver(&[("item", BindingType::SetupRef)], true);
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        let bindings = BindingExtractionResult {
            bindings: vec![crate::utils::oxc::Binding {
                name: "item",
                span: crate::common::Span::new(0, 4),
                pos: 0,
                ignore: true, // v-for local
                is_shorthand: false,
            }],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::Dynamic,
        };

        resolver.collect_binding_patches(&bindings, &mut out);

        // Ignored bindings produce no patches
        assert!(out.prepends.is_empty());
    }

    #[test]
    fn collect_patches_unresolved_adds_ctx_prefix() {
        let resolver = make_resolver(&[], false); // standalone, no bindings registered
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        let bindings = BindingExtractionResult {
            bindings: vec![crate::utils::oxc::Binding {
                name: "foo",
                span: crate::common::Span::new(0, 3),
                pos: 0,
                ignore: false,
                is_shorthand: false,
            }],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
        };

        resolver.collect_binding_patches(&bindings, &mut out);

        // Unresolved: prefix="_ctx." at pos 0, suffix="" (not pushed)
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0].0, 0);
        assert_eq!(out.prepends[0].1, "_ctx.");
    }

    #[test]
    fn collect_patches_multiple_bindings() {
        let resolver = make_resolver(
            &[
                ("count", BindingType::SetupRef),
                ("msg", BindingType::Props),
            ],
            true,
        );
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        let bindings = BindingExtractionResult {
            bindings: vec![
                crate::utils::oxc::Binding {
                    name: "count",
                    span: crate::common::Span::new(0, 5),
                    pos: 0,
                    ignore: false,
                    is_shorthand: false,
                },
                crate::utils::oxc::Binding {
                    name: "msg",
                    span: crate::common::Span::new(8, 11),
                    pos: 8,
                    ignore: false,
                    is_shorthand: false,
                },
            ],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
        };

        resolver.collect_binding_patches(&bindings, &mut out);

        // count: SetupRef inline → suffix ".value" at pos 5
        // msg: Props inline → prefix "__props." at pos 8
        assert_eq!(out.prepends.len(), 2);
        assert_eq!(out.prepends[0], (5, ".value"));
        assert_eq!(out.prepends[1], (8, "__props."));
    }

    // ==================== is_simple_ident ====================

    #[test]
    fn is_simple_ident_basic() {
        assert!(is_simple_ident("foo"));
        assert!(is_simple_ident("_bar"));
        assert!(is_simple_ident("$baz"));
        assert!(is_simple_ident("count123"));
        assert!(!is_simple_ident(""));
        assert!(!is_simple_ident("123abc"));
        assert!(!is_simple_ident("foo.bar"));
        assert!(!is_simple_ident("a + b"));
        assert!(!is_simple_ident("foo[0]"));
    }

    // ==================== resolve_simple_expr ====================

    #[test]
    fn resolve_simple_expr_setup_ref_standalone() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
        assert_eq!(resolver.resolve_simple_expr("count"), "$setup.count");
    }

    #[test]
    fn resolve_simple_expr_setup_ref_inline() {
        let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
        assert_eq!(resolver.resolve_simple_expr("count"), "count.value");
    }

    #[test]
    fn resolve_simple_expr_props_inline() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], true);
        assert_eq!(resolver.resolve_simple_expr("msg"), "__props.msg");
    }

    #[test]
    fn resolve_simple_expr_unresolved() {
        let resolver = make_resolver(&[], false);
        assert_eq!(resolver.resolve_simple_expr("foo"), "_ctx.foo");
    }

    #[test]
    fn resolve_simple_expr_compound_passthrough() {
        let resolver = make_resolver(&[("a", BindingType::SetupRef)], true);
        assert_eq!(resolver.resolve_simple_expr("a + b"), "a + b");
    }

    #[test]
    fn resolve_simple_expr_trims_whitespace() {
        let resolver = make_resolver(&[("foo", BindingType::SetupConst)], false);
        assert_eq!(resolver.resolve_simple_expr("  foo  "), "$setup.foo");
    }
}
