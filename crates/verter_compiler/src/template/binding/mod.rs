//! The binding resolver of the legacy template emitters and the IDE route.
//!
//! The prefix and suffix table lives in
//! [`VueBindingView`](crate::framework::vue::semantic::VueBindingView); this
//! wrapper holds one and adds only what the view does not own: the TSX
//! accessor table of the IDE route (with its completion-prefix matching),
//! the loop-variable spellings the legacy Vapor emitter renames, and the
//! patch collection into a legacy [`EditRecorder`].

pub(crate) mod destructure;
pub mod prefixed_expr;

use rustc_hash::FxHashMap;

use crate::framework::vue::semantic::{
    resolve_simple_expr_with, BindingAccess, VueBindingTable, VueBindingView,
};
use crate::utils::oxc::bindings::keywords::{is_global, is_keyword};
use crate::utils::oxc::BindingExtractionResult;

use crate::code_transform::EditRecorder;

// BindingType and ReactivityLevel canonical definitions are in verter_parser::types.
// Re-exported here for backward compatibility.
pub use crate::framework::vue::semantic::is_simple_ident;
pub use verter_parser::types::{BindingType, ReactivityLevel};

/// Resolves identifiers to their accessor prefix and suffix.
///
/// The legacy emitters reuse one resolved text both as a value and as an
/// assignment target (`v-model`'s `"onUpdate:modelValue": $event => ((x) =
/// $event)`), so every position resolves with [`BindingAccess::Write`],
/// whose text is valid in both.
pub struct BindingResolver<'alloc> {
    view: VueBindingView<'alloc>,
    /// TSX mode: props use `__props.`, known bindings are bare, unresolved use
    /// `___VERTER___instance.` for instance property access. No `.value` suffix.
    /// Block scope `shallowUnwrapRef` handles unwrapping.
    is_tsx: bool,
    /// Stack of active v-for loop-variable rename maps — Vapor-only. Each
    /// entry maps a v-for's raw loop-variable name (the item, or a
    /// destructured sub-binding's leaf name) to its renamed accessor text
    /// (`_for_item{depth}.value`, `_for_key{depth}.value`, ...), mirroring
    /// official's real `context.withId(fn, idMap)` scoping (confirmed
    /// directly against the vendored rc.5 `@vue/compiler-vapor` source:
    /// `genFor`'s `itemVar = _for_item${depth}` + `buildDestructureIdMap`).
    /// The spelling belongs to the emitting target, so it stays here, beside
    /// the view. VDOM/SSR/IDE never push here — official's VDOM `genFor` does
    /// not rename loop variables at all, only Vapor does (`Two Template
    /// Codegen Paths`). Pushed/popped by Vapor's own `enter`/`leave` of a
    /// v-for element around the SAME extent as the loop body's own AST
    /// subtree — never touched by v-slot scoped-slot locals, which official
    /// does not rename either.
    for_scope_stack: Vec<FxHashMap<String, String>>,
}

impl<'alloc> BindingResolver<'alloc> {
    /// Create a new resolver from a binding map and inline mode flag.
    pub fn new(bindings: FxHashMap<&'alloc str, BindingType>, is_inline: bool) -> Self {
        Self::new_with_const_props(bindings, is_inline, None)
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
            view: VueBindingView::owning(
                VueBindingTable::from_reported(bindings),
                is_inline,
                const_props,
            ),
            is_tsx: false,
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
        self.view.is_ref_bindable_import(name)
    }

    /// Install the ref-bindable import set (from script codegen).
    pub fn set_ref_bindable_imports(&mut self, names: rustc_hash::FxHashSet<String>) {
        self.view.set_ref_bindable_imports(names);
    }

    /// Set the vapor mode flag.
    #[inline]
    pub fn set_vapor(&mut self, vapor: bool) {
        self.view.set_fine_grained(vapor);
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
        self.view.get(ident)
    }

    #[inline]
    fn has_completion_prefix_match(&self, ident: &str) -> bool {
        !ident.is_empty()
            && self
                .view
                .table()
                .iter()
                .any(|(candidate, _)| candidate.starts_with(ident))
    }

    /// Check if all non-ignored bindings in an expression are const props
    /// (cross-file override). See
    /// [`VueBindingView::all_bindings_const_props`].
    pub fn all_bindings_const_props(&self, bindings: Option<&BindingExtractionResult<'_>>) -> bool {
        self.view.all_bindings_const_props(bindings)
    }

    /// Whether this resolver is in inline mode.
    #[inline]
    #[allow(dead_code)]
    pub fn is_inline(&self) -> bool {
        self.view.is_inline()
    }

    /// Resolve the accessor prefix for an identifier.
    ///
    /// **TSX mode**: props use `__props.`, known bindings are bare (no
    /// prefix), unresolved identifiers use `___VERTER___instance.` (matches
    /// Vue's `_ctx.` behavior), globals and keywords remain bare. Every other
    /// mode is [`VueBindingView::prefix`] with [`BindingAccess::Write`].
    #[inline]
    pub fn resolve_prefix(&self, ident: &str) -> &'static str {
        if self.is_tsx {
            return match self.view.get(ident) {
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
        self.view.prefix(ident, BindingAccess::Write)
    }

    /// Resolve the accessor suffix for an identifier: `.value` for
    /// `SetupRef` and `SetupMaybeRef` bindings in inline mode, empty
    /// otherwise. Vapor mode and TSX mode never add `.value`.
    #[inline]
    pub fn resolve_suffix(&self, ident: &str) -> &'static str {
        if self.is_tsx {
            return "";
        }
        self.view.suffix(ident, BindingAccess::Write)
    }

    /// Resolve an ASSET reference (a component or directive tag name) to
    /// its fully-qualified access expression. See
    /// [`VueBindingView::asset_ref`].
    pub fn resolve_asset_ref(&self, ident: &str) -> String {
        if self.is_tsx {
            let prefix = self.resolve_prefix(ident);
            let mut s = String::with_capacity(prefix.len() + ident.len());
            s.push_str(prefix);
            s.push_str(ident);
            return s;
        }
        self.view.asset_ref(ident, BindingAccess::Write)
    }

    /// Resolve a simple identifier expression to its prefixed/suffixed form.
    ///
    /// If the expression is a simple identifier (no dots, brackets, operators),
    /// returns `prefix + ident + suffix`. Otherwise returns the expression unchanged.
    /// A keyword that is a registered binding (a prop named `class`) uses
    /// bracket notation: `$props["class"]`.
    pub fn resolve_simple_expr(&self, expr: &str) -> String {
        resolve_simple_expr_with(
            expr,
            |ident| self.view.table().contains(ident),
            |ident| (self.resolve_prefix(ident), self.resolve_suffix(ident)),
        )
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
        out: &mut EditRecorder<'alloc>,
    ) {
        prefixed_expr::collect_binding_patches(self, bindings, out);
    }
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod binding_tests;
