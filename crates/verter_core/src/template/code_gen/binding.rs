//! Binding type classification and resolver for the AST-based codegen pipeline.

use rustc_hash::FxHashMap;

use crate::utils::oxc::bindings::keywords::{is_global, is_keyword};
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
    /// Import specifier — may be type-only usage. Included in `__returned__`
    /// only when the identifier appears in the template text (word-boundary match).
    SetupImport,
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
            BindingType::SetupConst | BindingType::SetupImport | BindingType::LiteralConst => {
                ReactivityLevel::Static
            }
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
                | BindingType::SetupImport
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
    is_vapor: bool,
    /// Props known to be constant across all call sites (from cross-file analysis).
    /// These are treated as `Static` for reactivity purposes while keeping
    /// their `$props.`/`__props.` prefix for correct runtime access.
    const_props: Option<rustc_hash::FxHashSet<&'alloc str>>,
}

impl<'alloc> BindingResolver<'alloc> {
    /// Create a new resolver from a binding map and inline mode flag.
    pub fn new(bindings: FxHashMap<&'alloc str, BindingType>, is_inline: bool) -> Self {
        Self {
            bindings,
            is_inline,
            is_vapor: false,
            const_props: None,
        }
    }

    /// Create a new resolver with cross-file const prop overrides.
    ///
    /// Props in the `const_props` set are treated as `Static` for reactivity,
    /// but still use `$props.`/`__props.` prefix for correct runtime access.
    pub fn new_with_const_props(
        bindings: FxHashMap<&'alloc str, BindingType>,
        is_inline: bool,
        const_props: Option<rustc_hash::FxHashSet<&'alloc str>>,
    ) -> Self {
        Self {
            bindings,
            is_inline,
            is_vapor: false,
            const_props,
        }
    }

    /// Set the vapor mode flag.
    #[inline]
    pub fn set_vapor(&mut self, vapor: bool) {
        self.is_vapor = vapor;
    }

    /// Look up the binding type for an identifier.
    #[inline]
    pub fn get(&self, ident: &str) -> Option<BindingType> {
        self.bindings.get(ident).copied()
    }

    /// Check if all non-ignored bindings in an expression are const props (cross-file override).
    ///
    /// Returns `true` ONLY when cross-file `const_props` data is available AND every
    /// non-ignored identifier is either a const prop or a literal/setup const. This is
    /// conservative: without `const_props` data, always returns `false` to match Vue's
    /// official compiler output (which never elides bound props from `dynamicProps`).
    ///
    /// Used to skip adding props to the VDOM `dynamicProps` array when cross-file analysis
    /// proves the prop value cannot change across re-renders.
    pub fn all_bindings_const_props(&self, bindings: Option<&BindingExtractionResult<'_>>) -> bool {
        // No const_props data → no optimization (match Vue's behavior)
        let Some(ref const_props) = self.const_props else {
            return false;
        };
        let Some(b) = bindings else {
            return false;
        };
        let names = b.non_ignored_binding_names();
        if names.is_empty() {
            // Pure literal expression — always static, but Vue still includes
            // the prop in dynamicProps so we stay compatible.
            return false;
        }
        // All identifiers must be either:
        // - A const prop (from cross-file analysis), or
        // - A setup const / literal const / import (inherently static)
        names.iter().all(|name| match self.bindings.get(*name) {
            Some(bt) if bt.is_props() => const_props.contains(*name),
            Some(bt) => bt.reactivity_level() == ReactivityLevel::Static,
            None => false,
        })
    }

    /// Whether this resolver is in inline mode.
    #[inline]
    #[allow(dead_code)]
    pub fn is_inline(&self) -> bool {
        self.is_inline
    }

    /// Resolve the accessor prefix for an identifier.
    ///
    /// - **Vapor mode**: all bindings use `_ctx.` (matching Vue's official vapor compiler)
    /// - **VDOM mode**:
    ///   - Props: `__props.` (inline) or `$props.` (standalone)
    ///   - Setup bindings: `""` (inline) or `$setup.` (standalone)
    ///   - Data/Options/Unresolved: `_ctx.`
    #[inline]
    pub fn resolve_prefix(&self, ident: &str) -> &'static str {
        if self.is_vapor {
            return "_ctx.";
        }
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
    /// empty string otherwise. Vapor mode never adds `.value`.
    #[inline]
    pub fn resolve_suffix(&self, ident: &str) -> &'static str {
        if self.is_vapor || !self.is_inline {
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
    /// When the identifier is a JS keyword (e.g., `class`) but exists as a registered
    /// binding (e.g., a prop), bracket notation is used: `$props["class"]` instead of
    /// `$props.class` which would be a syntax error.
    ///
    /// This is useful for places that need a fully-resolved expression string
    /// but only have a raw expression (not OXC-parsed bindings).
    pub fn resolve_simple_expr(&self, expr: &str) -> String {
        let trimmed = expr.trim();
        if !is_simple_ident(trimmed) {
            return trimmed.to_string();
        }

        let is_kw = is_keyword(trimmed.as_bytes());

        // Keywords that are not registered bindings are left unchanged (e.g., `true`, `false`).
        // Globals are also left unchanged.
        if (is_kw && !self.bindings.contains_key(trimmed)) || is_global(trimmed.as_bytes()) {
            return trimmed.to_string();
        }

        let prefix = self.resolve_prefix(trimmed);
        let suffix = self.resolve_suffix(trimmed);

        // Keywords used as member access require bracket notation:
        // `$props["class"]` instead of `$props.class`.
        if is_kw && !prefix.is_empty() {
            // Convert dot prefix (e.g., "$props.") to bracket prefix (e.g., "$props[\"")
            let base = prefix.trim_end_matches('.');
            let mut result =
                String::with_capacity(base.len() + 2 + trimmed.len() + 2 + suffix.len());
            result.push_str(base);
            result.push_str("[\"");
            result.push_str(trimmed);
            result.push_str("\"]");
            result.push_str(suffix);
            result
        } else {
            let mut result = String::with_capacity(prefix.len() + trimmed.len() + suffix.len());
            result.push_str(prefix);
            result.push_str(trimmed);
            result.push_str(suffix);
            result
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
            let suffix = self.resolve_suffix(binding.name);

            // When a shorthand property `{ foo }` gets a prefix/suffix, expand
            // it to `{ foo: $setup.foo }` to keep valid JS. We prepend "foo: "
            // at the same position before the prefix; stable sort preserves order.
            if binding.is_shorthand && (!prefix.is_empty() || !suffix.is_empty()) {
                out.prepend_alloc(binding.pos, &format!("{}: ", binding.name));
            }

            if !prefix.is_empty() {
                out.prepend_static(binding.pos, prefix);
            }

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
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    // First byte must be ASCII letter, '_', or '$'
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return false;
    }
    // Remaining bytes must be ASCII alphanumeric, '_', or '$'
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
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

    // ==================== Vapor mode bindings ====================

    fn make_vapor_resolver(entries: &[(&'static str, BindingType)]) -> BindingResolver<'static> {
        let mut map = FxHashMap::default();
        for &(name, bt) in entries {
            map.insert(name, bt);
        }
        let mut r = BindingResolver::new(map, false);
        r.set_vapor(true);
        r
    }

    #[test]
    fn vapor_setup_ref_prefix_is_ctx() {
        let resolver = make_vapor_resolver(&[("count", BindingType::SetupRef)]);
        assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
    }

    #[test]
    fn vapor_setup_const_prefix_is_ctx() {
        let resolver = make_vapor_resolver(&[("fn", BindingType::SetupConst)]);
        assert_eq!(resolver.resolve_prefix("fn"), "_ctx.");
    }

    #[test]
    fn vapor_setup_let_prefix_is_ctx() {
        let resolver = make_vapor_resolver(&[("x", BindingType::SetupLet)]);
        assert_eq!(resolver.resolve_prefix("x"), "_ctx.");
    }

    #[test]
    fn vapor_props_prefix_is_ctx() {
        let resolver = make_vapor_resolver(&[("msg", BindingType::Props)]);
        assert_eq!(resolver.resolve_prefix("msg"), "_ctx.");
    }

    #[test]
    fn vapor_unresolved_prefix_is_ctx() {
        let resolver = make_vapor_resolver(&[]);
        assert_eq!(resolver.resolve_prefix("unknown"), "_ctx.");
    }

    #[test]
    fn vapor_suffix_is_always_empty() {
        let resolver = make_vapor_resolver(&[("count", BindingType::SetupRef)]);
        assert_eq!(resolver.resolve_suffix("count"), "");
    }

    #[test]
    fn vapor_resolve_simple_expr_uses_ctx() {
        let resolver = make_vapor_resolver(&[("msg", BindingType::SetupRef)]);
        assert_eq!(resolver.resolve_simple_expr("msg"), "_ctx.msg");
    }

    #[test]
    fn vapor_resolve_simple_expr_props_uses_ctx() {
        let resolver = make_vapor_resolver(&[("title", BindingType::Props)]);
        assert_eq!(resolver.resolve_simple_expr("title"), "_ctx.title");
    }

    // ==================== Reserved word bindings ====================

    #[test]
    fn resolve_simple_expr_keyword_prop_uses_bracket_notation() {
        let resolver = make_resolver(&[("class", BindingType::Props)], false);
        assert_eq!(resolver.resolve_simple_expr("class"), r#"$props["class"]"#);
    }

    #[test]
    fn resolve_simple_expr_keyword_prop_inline_uses_bracket_notation() {
        let resolver = make_resolver(&[("class", BindingType::Props)], true);
        assert_eq!(resolver.resolve_simple_expr("class"), r#"__props["class"]"#);
    }

    #[test]
    fn resolve_simple_expr_keyword_not_in_bindings_unchanged() {
        let resolver = make_resolver(&[], false);
        assert_eq!(resolver.resolve_simple_expr("class"), "class");
    }

    #[test]
    fn resolve_simple_expr_keyword_for_as_prop() {
        let resolver = make_resolver(&[("for", BindingType::Props)], false);
        assert_eq!(resolver.resolve_simple_expr("for"), r#"$props["for"]"#);
    }

    #[test]
    fn vapor_resolve_simple_expr_keyword_prop_uses_bracket_notation() {
        let resolver = make_vapor_resolver(&[("class", BindingType::Props)]);
        assert_eq!(resolver.resolve_simple_expr("class"), r#"_ctx["class"]"#);
    }

    // ==================== Const prop overrides ====================

    fn make_resolver_with_const_props(
        entries: &[(&'static str, BindingType)],
        is_inline: bool,
        const_props: &[&'static str],
    ) -> BindingResolver<'static> {
        let mut map = FxHashMap::default();
        for &(name, bt) in entries {
            map.insert(name, bt);
        }
        let const_set: rustc_hash::FxHashSet<&str> = const_props.iter().copied().collect();
        BindingResolver::new_with_const_props(map, is_inline, Some(const_set))
    }

    /// @ai-generated - Const prop still uses $props. prefix (not $setup.)
    #[test]
    fn const_prop_still_uses_props_prefix() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
        assert_eq!(resolver.resolve_prefix("msg"), "$props.");
    }

    /// @ai-generated - Const prop still uses __props. prefix in inline mode
    #[test]
    fn const_prop_still_uses_dunder_props_prefix_inline() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
        assert_eq!(resolver.resolve_prefix("msg"), "__props.");
    }

    /// @ai-generated - Const prop has no .value suffix
    #[test]
    fn const_prop_still_no_value_suffix() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
        assert_eq!(resolver.resolve_suffix("msg"), "");
    }

    // ==================== all_bindings_const_props ====================

    use crate::utils::oxc::BindingExtractionResult;

    fn make_bindings_result<'a>(names: &[(&'a str, bool)]) -> BindingExtractionResult<'a> {
        use crate::common::Span;
        use crate::utils::oxc::bindings::Binding;
        BindingExtractionResult {
            bindings: names
                .iter()
                .map(|&(name, ignore)| Binding {
                    name,
                    span: Span {
                        start: 0,
                        end: name.len() as u32,
                    },
                    pos: 0,
                    ignore,
                    is_shorthand: false,
                })
                .collect(),
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
        }
    }

    /// Without const_props data, always returns false (Vue compatibility)
    #[test]
    fn all_const_props_no_data_returns_false() {
        let resolver = make_resolver(&[("msg", BindingType::Props)], false);
        let bindings = make_bindings_result(&[("msg", false)]);
        assert!(!resolver.all_bindings_const_props(Some(&bindings)));
    }

    /// With const_props, a const prop expression returns true
    #[test]
    fn all_const_props_with_const_prop_returns_true() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
        let bindings = make_bindings_result(&[("msg", false)]);
        assert!(resolver.all_bindings_const_props(Some(&bindings)));
    }

    /// Non-const prop expression returns false
    #[test]
    fn all_const_props_with_non_const_prop_returns_false() {
        let resolver = make_resolver_with_const_props(
            &[("msg", BindingType::Props), ("count", BindingType::Props)],
            false,
            &["msg"], // only msg is const
        );
        let bindings = make_bindings_result(&[("count", false)]);
        assert!(!resolver.all_bindings_const_props(Some(&bindings)));
    }

    /// Mixed const prop + setup const returns true
    #[test]
    fn all_const_props_mixed_const_prop_and_setup_const() {
        let resolver = make_resolver_with_const_props(
            &[
                ("msg", BindingType::Props),
                ("LABEL", BindingType::SetupConst),
            ],
            false,
            &["msg"],
        );
        let bindings = make_bindings_result(&[("msg", false), ("LABEL", false)]);
        assert!(resolver.all_bindings_const_props(Some(&bindings)));
    }

    /// Mixed const prop + reactive ref returns false
    #[test]
    fn all_const_props_mixed_const_prop_and_ref_returns_false() {
        let resolver = make_resolver_with_const_props(
            &[
                ("msg", BindingType::Props),
                ("count", BindingType::SetupRef),
            ],
            false,
            &["msg"],
        );
        let bindings = make_bindings_result(&[("msg", false), ("count", false)]);
        assert!(!resolver.all_bindings_const_props(Some(&bindings)));
    }

    /// No bindings data returns false (conservative)
    #[test]
    fn all_const_props_none_bindings_returns_false() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
        assert!(!resolver.all_bindings_const_props(None));
    }

    /// Empty non-ignored names returns false (literal expression — Vue compat)
    #[test]
    fn all_const_props_pure_literal_returns_false() {
        let resolver =
            make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
        // All identifiers are ignored (v-for locals, etc.)
        let bindings = make_bindings_result(&[("item", true)]);
        assert!(!resolver.all_bindings_const_props(Some(&bindings)));
    }
}
