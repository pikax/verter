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
    /// SSR mode: non-inline `ssrRender(_ctx, _push, _parent, _attrs)` has no
    /// `$setup`/`$props`/`$data`/`$options` parameters. Setup state, props,
    /// data, and options are reached through the instance proxy as `_ctx.*`
    /// (matching Vue's non-inline `@vue/compiler-ssr` output). Free `$setup`
    /// references in that signature are a runtime `ReferenceError`.
    is_ssr: bool,
    /// TSX mode: props use `__props.`, known bindings are bare, unresolved use
    /// `___VERTER___instance.` for instance property access. No `.value` suffix.
    /// Block scope `shallowUnwrapRef` handles unwrapping.
    is_tsx: bool,
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
            is_ssr: false,
            is_tsx: false,
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
            is_ssr: false,
            is_tsx: false,
            const_props,
        }
    }

    /// Set the vapor mode flag.
    #[inline]
    pub fn set_vapor(&mut self, vapor: bool) {
        self.is_vapor = vapor;
    }

    /// Set the SSR mode flag. Non-inline SSR binds through `_ctx.` only.
    #[inline]
    pub fn set_ssr(&mut self, ssr: bool) {
        self.is_ssr = ssr;
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
    /// - **Vapor mode**: all bindings use `_ctx.` (matching Vue's official vapor compiler)
    /// - **SSR non-inline**: setup/props/data/options all use `_ctx.` — the
    ///   `ssrRender(_ctx, _push, _parent, _attrs)` signature has no `$setup`
    ///   parameter, so `$setup.*` would be a free reference / runtime error
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
            return "_ctx.";
        }
        // Non-inline SSR: instance proxy only. Inline SSR still uses bare /
        // __props. because the render closes over setup locals.
        if self.is_ssr && !self.is_inline {
            return match self.bindings.get(ident) {
                Some(BindingType::PropsDestructured) => "",
                Some(_) => "_ctx.",
                None if ident == "$event" => "",
                None => "_ctx.",
            };
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
