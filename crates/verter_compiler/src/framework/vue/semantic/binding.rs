//! Script bindings and the text a template identifier is emitted with.
//!
//! [`VueBindingTable`] holds every script binding of one component with the
//! class the script frontend reported. [`VueBindingView`] is the one table
//! that turns a binding class into the text written around a template
//! identifier (`$setup.`, `__props.`, `.value`, `_unref(…)`): every Vue
//! target asks it, and the IDE route's `BindingResolver` wraps it.

use std::cell::Cell;
use std::ops::Deref;

use rustc_hash::{FxHashMap, FxHashSet};

use super::origin::{FrameworkOrigin, FrameworkOriginTable};
use crate::template::oxc::scope::{LexicalScopeId, LexicalScopes};
use crate::types::{BindingType, ReactivityLevel};
use crate::utils::oxc::bindings::keywords::{is_global, is_keyword};
use crate::utils::oxc::bindings::BindingExtractionResult;

/// The prefix of an identifier read through Vue's `unref` helper. A target
/// that emits it imports `unref` as `_unref`.
pub const UNREF_PREFIX: &str = "_unref(";

/// [`UNREF_PREFIX`] for the constructor of a `new` expression, parenthesised
/// so that `new` applies to the unwrapped value: `new (_unref(X))()`.
const CONSTRUCTED_UNREF_PREFIX: &str = "(_unref(";

/// One script binding as the compiler sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueBindingFact {
    /// The class every consumer reads.
    pub binding: BindingType,
    /// The class the script frontend reported.
    pub reported: BindingType,
    /// Origin of the initializer's callee, when the binding is a call.
    pub callee_origin: FrameworkOrigin,
}

/// Every script binding of the component, by name.
#[derive(Debug, Clone, Default)]
pub struct VueBindingTable<'c> {
    facts: FxHashMap<&'c str, VueBindingFact>,
}

impl<'c> VueBindingTable<'c> {
    /// The bindings the script frontend reported, each with its reported
    /// class and no proven callee origin.
    pub fn from_reported(reported: impl IntoIterator<Item = (&'c str, BindingType)>) -> Self {
        let facts = reported
            .into_iter()
            .map(|(name, binding)| {
                (
                    name,
                    VueBindingFact {
                        binding,
                        reported: binding,
                        callee_origin: FrameworkOrigin::Unknown,
                    },
                )
            })
            .collect();
        Self { facts }
    }

    pub fn get(&self, name: &str) -> Option<VueBindingFact> {
        self.facts.get(name).copied()
    }

    pub fn binding(&self, name: &str) -> Option<BindingType> {
        self.facts.get(name).map(|fact| fact.binding)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.facts.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'c str, VueBindingFact)> + '_ {
        self.facts.iter().map(|(name, fact)| (*name, *fact))
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Record, for every binding whose initializer is a call, where the
    /// callee comes from. The class stays the one the script frontend
    /// reported: the frontend's classifier is the only one, and this table
    /// keeps only the provenance beside it (the callee's module specifier
    /// and imported name, answered by [`FrameworkOriginTable::callee_provenance`]).
    pub fn record_callee_origins(&mut self, origins: &FrameworkOriginTable) {
        for (name, fact) in &mut self.facts {
            if let Some(callee) = origins
                .view()
                .initializer_of(name)
                .and_then(|initializer| initializer.callee)
            {
                fact.callee_origin = origins.callee_origin(callee);
            }
        }
    }
}

/// How a template position uses an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingAccess {
    /// A read. In inline mode a binding that may or may not hold a ref
    /// (`setup-maybe-ref`, `setup-let`) is read through `_unref(…)`, as Vue
    /// does.
    Read,
    /// A read as the constructor of a `new` expression (`X` in `new X()`
    /// and `new X.Y()`). The text is a read's, parenthesised when it goes
    /// through `_unref`: `new (_unref(X))()`.
    Construct,
    /// A position that may be written: an assignment target, an update
    /// operand, the target of `v-model`. A binding that may hold a ref is
    /// written through `.value`; a `let` binding is written bare (the
    /// `isRef` guard Vue puts around that write is the target's to emit).
    /// The text is valid in a read position too.
    Write,
}

/// The view's table, borrowed from a snapshot or owned by a wrapper. A
/// plain enum rather than `Cow`, so the view stays covariant in its
/// lifetime.
#[derive(Debug, Clone)]
enum TableRef<'c> {
    Borrowed(&'c VueBindingTable<'c>),
    Owned(VueBindingTable<'c>),
}

impl<'c> Deref for TableRef<'c> {
    type Target = VueBindingTable<'c>;

    fn deref(&self) -> &Self::Target {
        match self {
            TableRef::Borrowed(table) => table,
            TableRef::Owned(table) => table,
        }
    }
}

/// How template identifiers are rewritten for one target run: the access
/// prefix and suffix of each binding, with the template scopes that shadow
/// them.
#[derive(Debug, Clone)]
pub struct VueBindingView<'c> {
    table: TableRef<'c>,
    inline: bool,
    fine_grained: bool,
    const_props: Option<FxHashSet<&'c str>>,
    ref_bindable_imports: FxHashSet<String>,
    /// The template's lexical scopes, the one producer of template-local
    /// names; `None` until a target attaches them.
    template_scopes: Option<&'c LexicalScopes<'c>>,
    /// The template scopes entered, innermost last.
    local_scopes: Vec<LexicalScopeId>,
    /// Whether any text this view produced reads through `_unref`; the
    /// target reads it once when it lists its imports.
    unref_read: Cell<bool>,
}

impl<'c> VueBindingView<'c> {
    pub fn new(
        table: &'c VueBindingTable<'c>,
        inline: bool,
        fine_grained: bool,
        const_props: Option<FxHashSet<&'c str>>,
        ref_bindable_imports: FxHashSet<String>,
    ) -> Self {
        Self {
            table: TableRef::Borrowed(table),
            inline,
            fine_grained,
            const_props,
            ref_bindable_imports,
            template_scopes: None,
            local_scopes: Vec::new(),
            unref_read: Cell::new(false),
        }
    }

    /// A view that owns its table, for a caller that holds the bindings
    /// itself.
    pub(crate) fn owning(
        table: VueBindingTable<'c>,
        inline: bool,
        const_props: Option<FxHashSet<&'c str>>,
    ) -> Self {
        Self {
            table: TableRef::Owned(table),
            inline,
            fine_grained: false,
            const_props,
            ref_bindable_imports: FxHashSet::default(),
            template_scopes: None,
            local_scopes: Vec::new(),
            unref_read: Cell::new(false),
        }
    }

    /// Attach the template's lexical scopes, so that the names a template
    /// scope declares shadow the script bindings.
    pub fn with_template_scopes(mut self, scopes: &'c LexicalScopes<'c>) -> Self {
        self.template_scopes = Some(scopes);
        self
    }

    pub(crate) fn set_fine_grained(&mut self, fine_grained: bool) {
        self.fine_grained = fine_grained;
    }

    pub(crate) fn set_ref_bindable_imports(&mut self, names: FxHashSet<String>) {
        self.ref_bindable_imports = names;
    }

    pub fn table(&self) -> &VueBindingTable<'c> {
        &self.table
    }

    pub fn is_inline(&self) -> bool {
        self.inline
    }

    pub fn is_fine_grained(&self) -> bool {
        self.fine_grained
    }

    pub fn get(&self, ident: &str) -> Option<BindingType> {
        self.table.binding(ident)
    }

    /// Enter a template scope (a `v-for` or `v-slot` frame of the attached
    /// [`LexicalScopes`]). The scope's names, and its parents', shadow the
    /// script bindings until [`Self::pop_local_scope`].
    pub fn push_local_scope(&mut self, scope: LexicalScopeId) {
        verter_debug_assert!(
            self.template_scopes.is_some(),
            "a template scope was entered with no template scopes attached"
        );
        self.local_scopes.push(scope);
    }

    /// Leave the innermost entered template scope.
    pub fn pop_local_scope(&mut self) {
        self.local_scopes.pop();
    }

    /// Whether a template scope entered here declares `name`.
    pub fn resolve_local(&self, name: &str) -> bool {
        match (self.template_scopes, self.local_scopes.last()) {
            (Some(scopes), Some(&scope)) => scopes.declares(scope, name),
            _ => false,
        }
    }

    /// Whether any identifier this view resolved is read through `_unref`,
    /// so the target must import `unref` as `_unref`.
    pub fn uses_unref(&self) -> bool {
        self.unref_read.get()
    }

    /// The `_unref` prefix for a read with `access`, recording that the
    /// view produced one.
    fn unref_prefix(&self, access: BindingAccess) -> &'static str {
        self.unref_read.set(true);
        if access == BindingAccess::Construct {
            CONSTRUCTED_UNREF_PREFIX
        } else {
            UNREF_PREFIX
        }
    }

    /// Whether `name` is a user import Vue marks `setup-maybe-ref`: an
    /// inline template `ref="name"` binds `ref_key`/`ref: name`.
    pub fn is_ref_bindable_import(&self, name: &str) -> bool {
        self.ref_bindable_imports.contains(name)
    }

    /// Text emitted before an identifier read (`_ctx.`, `$setup.`,
    /// `__props.`, `_unref(`, …).
    pub fn resolve_prefix(&self, ident: &str) -> &'static str {
        self.prefix(ident, BindingAccess::Read)
    }

    /// Text emitted after an identifier read (`.value`, `)` or nothing).
    pub fn resolve_suffix(&self, ident: &str) -> &'static str {
        self.suffix(ident, BindingAccess::Read)
    }

    /// Text emitted before an identifier used with `access`.
    ///
    /// - A name a template scope declares is bare.
    /// - **Fine-grained** (Vapor): props use `$props.`; every other binding
    ///   and an unresolved identifier use `_ctx.`. Vue's non-inline Vapor
    ///   expression transform is the binary `type === "props" ? "$props" :
    ///   "_ctx"`, not the richer table below.
    /// - **Client tree and server** (the same table: Vue's
    ///   `processExpression` prefixes from `context.inline` only):
    ///   - `$attrs` / `$slots` inline are the setup-context destructure,
    ///     referenced bare;
    ///   - props: `__props.` (inline) or `$props.`;
    ///   - setup bindings: bare (inline) or `$setup.`; an inline read of a
    ///     `setup-maybe-ref` or `setup-let` binding is `_unref(`, and
    ///     `(_unref(` when it is a `new` constructor;
    ///   - data / options: `_ctx.` (inline) or `$data.` / `$options.`;
    ///   - `$event` (the handler parameter) is bare; anything else is
    ///     `_ctx.`.
    pub fn prefix(&self, ident: &str, access: BindingAccess) -> &'static str {
        if self.resolve_local(ident) {
            return "";
        }
        if self.fine_grained {
            return match self.table.binding(ident) {
                Some(bt) if bt.is_props() => "$props.",
                _ => "_ctx.",
            };
        }
        if self.inline && (ident == "$attrs" || ident == "$slots") {
            return "";
        }
        match self.table.binding(ident) {
            Some(bt) if bt.is_props() => {
                if self.inline {
                    "__props."
                } else {
                    "$props."
                }
            }
            Some(bt) if bt.is_setup() => {
                if !self.inline {
                    "$setup."
                } else if access != BindingAccess::Write && reads_through_unref(bt) {
                    self.unref_prefix(access)
                } else {
                    ""
                }
            }
            Some(BindingType::Data) => {
                if self.inline {
                    "_ctx."
                } else {
                    "$data."
                }
            }
            Some(BindingType::Options) => {
                if self.inline {
                    "_ctx."
                } else {
                    "$options."
                }
            }
            None if ident == "$event" => "",
            _ => "_ctx.",
        }
    }

    /// Text emitted after an identifier used with `access`: in inline mode
    /// `.value` for a `setup-ref` (and for a `setup-maybe-ref` written),
    /// `)` closing an `_unref(` read (`))` for a `new` constructor);
    /// nothing otherwise. Fine-grained output never adds one.
    pub fn suffix(&self, ident: &str, access: BindingAccess) -> &'static str {
        if self.fine_grained || !self.inline || self.resolve_local(ident) {
            return "";
        }
        match self.table.binding(ident) {
            Some(bt) if access != BindingAccess::Write && reads_through_unref(bt) => {
                if access == BindingAccess::Construct {
                    "))"
                } else {
                    ")"
                }
            }
            Some(bt) if bt.needs_value_access() => ".value",
            _ => "",
        }
    }

    /// How a component or directive name is referenced when it is a
    /// binding (an asset read).
    pub fn resolve_asset_ref(&self, ident: &str) -> String {
        self.asset_ref(ident, BindingAccess::Read)
    }

    /// Resolve an asset reference (a component or directive tag name).
    ///
    /// Distinct from [`Self::prefix`]/[`Self::suffix`], which resolve a plain
    /// expression identifier with dot access. Vue's `resolveSetupReference`
    /// (`@vue/compiler-core`) always uses computed access for an asset in
    /// non-inline mode (`$setup["Name"]`, never `$setup.Name`), because an
    /// asset name need not be a valid identifier. Inline, a read of a binding
    /// that may hold a ref (`setup-let`, `setup-ref`, `setup-maybe-ref`) is
    /// `_unref(Name)`. Every other binding kind uses the identifier form.
    pub fn asset_ref(&self, ident: &str, access: BindingAccess) -> String {
        if !self.fine_grained {
            if let Some(bt) = self.table.binding(ident) {
                if bt.is_setup() && !self.inline {
                    return concat3("$setup[\"", ident, "\"]");
                }
                if bt.is_setup()
                    && access != BindingAccess::Write
                    && (bt == BindingType::SetupRef || reads_through_unref(bt))
                {
                    self.unref_read.set(true);
                    return concat3(UNREF_PREFIX, ident, ")");
                }
            }
        }
        concat3(
            self.prefix(ident, access),
            ident,
            self.suffix(ident, access),
        )
    }

    /// A bare identifier read with its prefix and suffix applied; any other
    /// expression is returned trimmed and unchanged.
    pub fn resolve_simple_expr(&self, expr: &str) -> String {
        self.simple_expr(expr, BindingAccess::Read)
    }

    /// A bare identifier used with `access`, with its prefix and suffix
    /// applied. See [`resolve_simple_expr_with`].
    pub fn simple_expr(&self, expr: &str, access: BindingAccess) -> String {
        resolve_simple_expr_with(
            expr,
            |ident| self.table.contains(ident),
            |ident| (self.prefix(ident, access), self.suffix(ident, access)),
        )
    }

    /// Whether every free identifier of an expression is a constant prop.
    ///
    /// True ONLY when cross-file `const_props` data is available AND every
    /// non-ignored identifier is either a const prop or a static setup
    /// binding. Without `const_props` data it is false, matching Vue's
    /// compiler, which never elides bound props from `dynamicProps`.
    ///
    /// Used to skip adding props to the client tree's `dynamicProps` when
    /// cross-file analysis proves the prop value cannot change across
    /// re-renders.
    pub fn all_bindings_const_props(&self, bindings: Option<&BindingExtractionResult<'_>>) -> bool {
        let Some(const_props) = &self.const_props else {
            return false;
        };
        let Some(bindings) = bindings else {
            return false;
        };
        let names = bindings.non_ignored_binding_names();
        if names.is_empty() {
            // A pure literal is static, but Vue still lists the prop in
            // `dynamicProps`, so stay compatible.
            return false;
        }
        names.iter().all(|name| match self.table.binding(name) {
            Some(bt) if bt.is_props() => const_props.contains(*name),
            Some(bt) => bt.reactivity_level() == ReactivityLevel::Static,
            None => false,
        })
    }
}

/// Whether an inline read of a binding of class `binding` goes through
/// `unref`: the binding may or may not hold a ref.
#[inline]
fn reads_through_unref(binding: BindingType) -> bool {
    matches!(binding, BindingType::SetupMaybeRef | BindingType::SetupLet)
}

fn concat3(a: &str, b: &str, c: &str) -> String {
    let mut out = String::with_capacity(a.len() + b.len() + c.len());
    out.push_str(a);
    out.push_str(b);
    out.push_str(c);
    out
}

/// Resolve a simple identifier expression to its prefixed and suffixed
/// form, given how each identifier is accessed.
///
/// An expression that is not a simple identifier (no dots, brackets or
/// operators) is returned trimmed and unchanged. A keyword that is not a
/// binding (`true`), a global and `$event` are unchanged too. A keyword that
/// is a binding (a prop named `class`) uses bracket access, since
/// `$props.class` is a syntax error: `$props["class"]`.
pub(crate) fn resolve_simple_expr_with(
    expr: &str,
    is_binding: impl Fn(&str) -> bool,
    access: impl Fn(&str) -> (&'static str, &'static str),
) -> String {
    let trimmed = expr.trim();
    if !is_simple_ident(trimmed) {
        return trimmed.to_string();
    }

    let is_kw = is_keyword(trimmed.as_bytes());
    if (is_kw && !is_binding(trimmed)) || is_global(trimmed.as_bytes()) || trimmed == "$event" {
        return trimmed.to_string();
    }

    let (prefix, suffix) = access(trimmed);
    if is_kw && !prefix.is_empty() {
        // `$props.` becomes `$props["`.
        let base = prefix.trim_end_matches('.');
        let mut result = String::with_capacity(base.len() + 2 + trimmed.len() + 2 + suffix.len());
        result.push_str(base);
        result.push_str("[\"");
        result.push_str(trimmed);
        result.push_str("\"]");
        result.push_str(suffix);
        result
    } else {
        concat3(prefix, trimmed, suffix)
    }
}

/// Whether a string is a simple JavaScript identifier: `foo`, `_bar`,
/// `$baz`, `count123`; not a compound expression or a member access.
pub fn is_simple_ident(s: &str) -> bool {
    let bytes = s.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod binding_tests;
