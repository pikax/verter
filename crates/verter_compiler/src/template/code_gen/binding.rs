//! Binding type classification and resolver for the AST-based codegen pipeline.

use rustc_hash::FxHashMap;

use crate::utils::oxc::bindings::keywords::{is_global, is_keyword};
use crate::utils::oxc::BindingExtractionResult;

use super::types::CodeGenOutput;

// BindingType and ReactivityLevel canonical definitions are in verter_parser::types.
// Re-exported here for backward compatibility.
pub use verter_parser::types::{BindingType, ReactivityLevel};

/// Resolves identifiers to their correct accessor prefix/suffix.
///
/// Holds the binding map (populated from script analysis) and the `is_inline`
/// flag. Provides methods to resolve individual identifiers and to batch-collect
/// binding patches for an expression's extracted bindings.
pub struct BindingResolver<'alloc> {
    bindings: FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
    is_vapor: bool,
    /// TSX mode: props use `__props.`, known bindings are bare, unresolved use
    /// `___VERTER___instance.` for instance property access. No `.value` suffix.
    /// Block scope `shallowUnwrapRef` handles unwrapping.
    is_tsx: bool,
    /// Props known to be constant across all call sites (from cross-file analysis).
    /// These are treated as `Static` for reactivity purposes while keeping
    /// their `$props.`/`__props.` prefix for correct runtime access.
    const_props: Option<rustc_hash::FxHashSet<&'alloc str>>,
    /// Named/default user imports official marks `setup-maybe-ref` — inline
    /// template refs to these names bind `ref_key`/`ref: name`. Owned (tiny
    /// set, cloned from the codegen options).
    ref_bindable_imports: rustc_hash::FxHashSet<String>,
    /// Stack of active v-for loop-variable rename maps — Vapor-only. Each
    /// entry maps a v-for's raw loop-variable name (the item, or a
    /// destructured sub-binding's leaf name) to its renamed accessor text
    /// (`_for_item{depth}.value`, `_for_key{depth}.value`, ...), mirroring
    /// official's real `context.withId(fn, idMap)` scoping (confirmed
    /// directly against the vendored rc.5 `@vue/compiler-vapor` source:
    /// `genFor`'s `itemVar = _for_item${depth}` + `buildDestructureIdMap`).
    /// VDOM/SSR/IDE never push here — official's VDOM `genFor` does not
    /// rename loop variables at all, only Vapor does (`Two Template Codegen
    /// Paths`). Pushed/popped by Vapor's own `enter`/`leave` of a v-for
    /// element around the SAME extent as the loop body's own AST subtree —
    /// never touched by v-slot scoped-slot locals, which official does not
    /// rename either.
    for_scope_stack: Vec<FxHashMap<String, String>>,
}

impl<'alloc> BindingResolver<'alloc> {
    /// Create a new resolver from a binding map and inline mode flag.
    pub fn new(bindings: FxHashMap<&'alloc str, BindingType>, is_inline: bool) -> Self {
        Self {
            bindings,
            is_inline,
            is_vapor: false,
            is_tsx: false,
            const_props: None,
            ref_bindable_imports: rustc_hash::FxHashSet::default(),
            for_scope_stack: Vec::new(),
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
            is_tsx: false,
            const_props,
            ref_bindable_imports: rustc_hash::FxHashSet::default(),
            for_scope_stack: Vec::new(),
        }
    }

    /// Push a v-for scope's loop-variable rename map — see
    /// `for_scope_stack`'s doc comment. Called by Vapor codegen only,
    /// around the SAME extent as the v-for's own item-body AST subtree.
    pub fn push_for_scope(&mut self, map: FxHashMap<String, String>) {
        self.for_scope_stack.push(map);
    }

    /// Pop the innermost v-for scope's rename map. Must be called exactly
    /// once per `push_for_scope`, in matching enter/leave order.
    pub fn pop_for_scope(&mut self) {
        self.for_scope_stack.pop();
    }

    /// Resolve a v-for loop-variable name to its renamed accessor text,
    /// searching from the INNERMOST active scope outward (nested v-for
    /// shadowing — an inner loop's own `item` shadows an outer loop's
    /// same-named variable, matching official's real nested-scope
    /// resolution). Returns `None` for any identifier that isn't a
    /// currently-active v-for loop variable (including v-slot scoped-slot
    /// locals, which are never pushed here and so always fall through to
    /// the existing bare-passthrough behavior).
    pub(crate) fn resolve_for_local(&self, name: &str) -> Option<&str> {
        self.for_scope_stack
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map(String::as_str)
    }

    /// Whether `name` is a user import official marks `setup-maybe-ref` —
    /// an inline template `ref="name"` binds `ref_key`/`ref: name`.
    #[inline]
    pub fn is_ref_bindable_import(&self, name: &str) -> bool {
        self.ref_bindable_imports.contains(name)
    }

    /// Install the ref-bindable import set (from script codegen).
    pub fn set_ref_bindable_imports(&mut self, names: rustc_hash::FxHashSet<String>) {
        self.ref_bindable_imports = names;
    }

    /// Set the vapor mode flag.
    #[inline]
    pub fn set_vapor(&mut self, vapor: bool) {
        self.is_vapor = vapor;
    }

    /// Set the TSX mode flag. When true, unresolved bindings use
    /// `___VERTER___instance.` prefix and refs have no `.value` suffix.
    #[inline]
    pub fn set_tsx(&mut self, tsx: bool) {
        self.is_tsx = tsx;
    }

    /// Look up the binding type for an identifier.
    #[inline]
    pub fn get(&self, ident: &str) -> Option<BindingType> {
        self.bindings.get(ident).copied()
    }

    #[inline]
    fn has_completion_prefix_match(&self, ident: &str) -> bool {
        !ident.is_empty()
            && self
                .bindings
                .keys()
                .any(|candidate| candidate.starts_with(ident))
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
    /// - **TSX mode**: props use `__props.`, known bindings are bare (no prefix),
    ///   unresolved identifiers use `___VERTER___instance.` (matches Vue's `_ctx.` behavior),
    ///   globals and keywords remain bare
    /// - **Vapor mode**: props use `$props.`; every other binding (setup, data,
    ///   options) and any unresolved identifier use `_ctx.` — official
    ///   `@vue/compiler-vapor`'s non-inline expression transform is the
    ///   binary `type === "props" ? "$props" : "_ctx"` (`compiler-vapor.cjs.js`),
    ///   not the richer `$setup.`/`$data.`/`$options.` table VDOM/SSR use
    /// - **SSR non-inline**: same table as VDOM non-inline below — official's
    ///   `processExpression` does not special-case SSR, and the SSR
    ///   `ssrRender` signature declares the matching `$props`/`$setup`/
    ///   `$data`/`$options` parameters whenever a script exists
    /// - **VDOM mode**:
    ///   - Props: `__props.` (inline) or `$props.` (standalone)
    ///   - Setup bindings: `""` (inline) or `$setup.` (standalone)
    ///   - Data/Options/Unresolved: `_ctx.`
    #[inline]
    pub fn resolve_prefix(&self, ident: &str) -> &'static str {
        if self.is_tsx {
            // TSX mode: props → __props., data/options → instance.,
            // setup bindings → bare, globals/keywords → bare,
            // unresolved → instance prefix (matches Vue's _ctx. behavior)
            return match self.bindings.get(ident) {
                Some(BindingType::PropsDestructured) => "",
                Some(bt) if bt.is_props() => "__props.",
                Some(BindingType::Data) | Some(BindingType::Options) => "___VERTER___instance.",
                Some(_) => "",
                None if self.has_completion_prefix_match(ident) => "",
                None if is_global(ident.as_bytes())
                    || is_keyword(ident.as_bytes())
                    || ident == "$event" =>
                {
                    ""
                }
                None => "___VERTER___instance.",
            };
        }
        if self.is_vapor {
            return match self.bindings.get(ident) {
                Some(bt) if bt.is_props() => "$props.",
                _ => "_ctx.",
            };
        }
        // SSR takes NO special branch here — official routes non-inline SSR
        // through the exact same table as non-inline VDOM below (see the
        // doc comment on `resolve_prefix` above).
        // Inline template mode: `$attrs`/`$slots` resolve to the setup-context
        // destructure (official `buildDestructureElements` injects them into
        // `setup(__props, { attrs: $attrs, slots: $slots })` on template use),
        // so they are referenced BARE — never `_ctx.$attrs` / `_ctx.$slots`.
        if self.is_inline && (ident == "$attrs" || ident == "$slots") {
            return "";
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
            Some(BindingType::Data) => {
                if self.is_inline {
                    // Inline mode: data properties are on the component proxy
                    "_ctx."
                } else {
                    "$data."
                }
            }
            Some(BindingType::Options) => {
                if self.is_inline {
                    "_ctx."
                } else {
                    "$options."
                }
            }
            // $event is the arrow function parameter in event handlers, not a ctx property
            None if ident == "$event" => "",
            None => "_ctx.",
            // unreachable but needed for exhaustiveness since we use guards above
            _ => "_ctx.",
        }
    }

    /// Resolve the accessor suffix for an identifier.
    ///
    /// Returns `.value` for `SetupRef` and `SetupMaybeRef` bindings in inline mode,
    /// empty string otherwise. Vapor mode and TSX mode never add `.value`.
    #[inline]
    pub fn resolve_suffix(&self, ident: &str) -> &'static str {
        if self.is_vapor || self.is_tsx || !self.is_inline {
            return "";
        }
        match self.bindings.get(ident) {
            Some(bt) if bt.needs_value_access() => ".value",
            _ => "",
        }
    }

    /// Resolve an ASSET reference (a component or directive tag name) to
    /// its fully-qualified access expression.
    ///
    /// Distinct from [`Self::resolve_prefix`]/[`Self::resolve_suffix`],
    /// which resolve a plain expression IDENTIFIER (event handler values,
    /// prop values, …) with dot access. Official Vue's `resolveSetupReference`
    /// (`@vue/compiler-core`) always uses COMPUTED/bracket access for an
    /// asset id in non-inline mode — `$setup["Name"]`, never `$setup.Name`
    /// — because asset names are not guaranteed to be valid JS identifiers
    /// and the compiler never special-cases "is this name a valid
    /// identifier" for this position. Inline mode still resolves to the
    /// bare const name (unprefixed), same as a plain identifier.
    ///
    /// Only the `setup`-kind binding case is covered (script-setup
    /// component imports/consts) — every other binding kind falls back to
    /// [`Self::resolve_prefix`]/[`Self::resolve_suffix`]'s existing dot
    /// form, unverified for this position but unchanged from prior
    /// behavior.
    pub fn resolve_asset_ref(&self, ident: &str) -> String {
        if !self.is_vapor && !self.is_tsx && !self.is_inline {
            if let Some(bt) = self.bindings.get(ident) {
                if bt.is_setup() {
                    let mut s = String::with_capacity(ident.len() + 11);
                    s.push_str("$setup[\"");
                    s.push_str(ident);
                    s.push_str("\"]");
                    return s;
                }
            }
        }
        let mut s = String::with_capacity(ident.len() + 16);
        s.push_str(self.resolve_prefix(ident));
        s.push_str(ident);
        s.push_str(self.resolve_suffix(ident));
        s
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
        // Globals and Vue's $event special variable are also left unchanged.
        if (is_kw && !self.bindings.contains_key(trimmed))
            || is_global(trimmed.as_bytes())
            || trimmed == "$event"
        {
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
                out.prepend_fmt(binding.pos, format_args!("{}: ", binding.name));
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
#[path = "binding_tests.rs"]
mod binding_tests;
