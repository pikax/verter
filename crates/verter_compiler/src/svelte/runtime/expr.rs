//! OXC-backed expression reparse + binding/scope ANALYSIS.
//!
//! The Svelte parser stores script bodies and every template expression as
//! `Span`s, never parsed expressions, so the runtime IR must REPARSE them with
//! OXC. This module owns ALL of that reparse: the instance + module script
//! bodies, every template expression, and the each / snippet / await / declaration
//! patterns. It performs ANALYSIS ONLY — it builds the [`ScopeGraph`], the
//! [`BindingTable`], and the per-expression [`ExprArena`] entries; it NEVER
//! rewrites a read into a `$.get` string or emits any JS (that is the backend's
//! job).
//!
//! The binding/scope model is the primary correctness surface. An identifier is
//! treated as a rune/signal binding ONLY when [`ScopeGraph::resolve`] finds the
//! nearest binding in scope to BE the rune declaration — a shadowing local
//! (a nested-function parameter, an each-as binding of the same name, a snippet
//! parameter, an await-then binding, a `{@const}` introduction, a declaration-tag
//! local) is its own binding and must NOT be classified as the signal.
//!
//! Both the script use-collector ([`ScriptUseCollector`]) and the
//! template-expression reference collector model the FULL nested lexical-scope
//! stack — function/arrow parameters AND nested `let`/`const`/`var`, `catch`
//! parameters, `for`-loop bindings, and nested function declarations — so a write
//! to an inner local of the same name as an outer rune (`() => { let count = 0;
//! count++; }`, `catch (n) { n = 5 }`, `for (let i …)`) is NOT attributed to the
//! outer signal, and an inner local of the same name is NOT reported as a free
//! reference of the enclosing expression.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, BindingPattern,
    BlockStatement, CallExpression, CatchClause, ChainElement, Expression, ForInStatement,
    ForOfStatement, ForStatement, Function, FunctionType, IdentifierReference, Program,
    SimpleAssignmentTarget, Statement, UpdateExpression, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::FxHashMap;

use super::ir::BindingId;

/// A lexical scope id into the [`ScopeGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// The runtime lowering classification of a binding.
///
/// This is the SEMANTIC classification a binding read/write rewrite consults —
/// it names WHAT a binding is, not the `$.`-call a backend emits for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingRuntimeKind {
    /// A plain local (`let x = …`) with no reactive treatment.
    PlainLocal,
    /// A `$state` reactive signal cell. `raw` = a `$state.raw(…)` signal (no deep
    /// proxy).
    StateSignal {
        /// Whether this is a `$state.raw` signal.
        raw: bool,
    },
    /// A bare `$.proxy(…)` value: an object/array `$state` deep-mutated but never
    /// reassigned. Reads/writes are PLAIN member access, NOT a signal.
    BareProxy,
    /// A `$.state($.proxy(…))` value: an object/array `$state` that is reassigned
    /// (the binding itself is reactive).
    StateProxy,
    /// A `$derived` / `$derived.by` memo.
    Derived,
    /// A `$props()` destructured prop.
    Prop,
    /// A `$bindable()` prop.
    BindableProp,
    /// An `{#each}` item / index binding — a SIGNAL read.
    EachSignal,
    /// An `{#await … then x}` / `{:catch e}` binding — a SIGNAL read.
    AwaitSignal,
    /// A `{@const}` block-local derived binding.
    LegacyConstDerived,
    /// A `{const …}` / `{let …}` declaration-tag local — INERT.
    TemplateDeclLocal,
    /// A `{#snippet name(...)}` NAME binding (callable by siblings via
    /// `{@render name(...)}`).
    SnippetName,
    /// A `{#snippet}` parameter — INERT.
    SnippetParam,
    /// A module-script binding.
    ModuleBinding,
    /// A default import of a `.svelte` component module (`import Child from
    /// './Child.svelte'`) — a NON-REACTIVE value binding. The imported local is a
    /// component callee (`Child($$anchor, …)`); a read emits the bare name, NEVER
    /// `$.get`.
    ComponentImport,
}

/// The declared `$state` rune flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRuneKind {
    /// `$state(…)`.
    State,
    /// `$state.raw(…)`.
    Raw,
}

/// The set of WRITE uses observed for a binding.
///
/// A `$state` binding's `$.state(…)`-wrapper lowering is decided ENTIRELY by
/// whether the binding identifier is REASSIGNED — verified against the pinned
/// compiler (`is_state_source` reduces to `reassigned` in runes non-dev). Reads
/// do not enter the wrapper decision. A `deep_mutated` flag is retained as a
/// neutral fact (member writes / mutating method calls) for diagnostics and
/// future SSR/legacy analysis, but it does NOT drive `$state` declaration
/// lowering — the proxy decision is `should_proxy(initializer)`, independent of
/// mutation (an object/array/call `$state` is proxied even when never mutated).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BindingUseSet {
    /// The binding identifier is reassigned (`x = …` / `x += …` / `x++` / a
    /// destructuring-assignment target `({x} = …)` / `[x] = …`).
    pub reassigned: bool,
    /// A member of the binding's value is mutated (`x.a = …` / `x.a++` / a
    /// mutating method call `x.push(…)`). Neutral fact only — NOT a lowering
    /// determinant in runes mode.
    pub deep_mutated: bool,
}

/// The full `$state` lowering classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateClassification {
    /// The declared rune flavour.
    pub declared: StateRuneKind,
    /// Whether the initializer is PROXIABLE (`should_proxy(init)`) — the official
    /// `svelte@5.56.3` predicate over the initializer SHAPE alone. Independent of
    /// reads/writes/mutations.
    pub proxiable: bool,
    /// The observed uses.
    pub uses: BindingUseSet,
    /// The resulting lowering decision.
    pub lowering: StateLowering,
}

/// The resolved `$state` lowering decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateLowering {
    /// `let x = …;` — never reactively read.
    PlainLet,
    /// `let x = $.state(prim);` — a reactive primitive.
    StateSignal,
    /// `let o = $.proxy({…});` — an object/array deep-mutated, never reassigned.
    BareProxy,
    /// `let o = $.state($.proxy({…}));` — an object/array reassigned.
    StateProxy,
    /// `let o = $.state({…});` — a reassigned `$state.raw` (no proxy).
    RawStateSignal,
}

/// Compute the `$state` lowering from the declared flavour, whether the
/// initializer is PROXIABLE, and whether the binding is REASSIGNED.
///
/// The rule is derived empirically from `svelte@5.56.3` (`generate: 'client'`,
/// runes, non-dev) and confirmed against the compiler source
/// (`is_state_source` / `should_proxy`):
///
/// - The `$.state(…)` SIGNAL wrapper is gated on `reassigned` ALONE
///   (`is_state_source` reduces to `binding.reassigned` in runes non-dev). A
///   deep mutation (`o.a++`, `arr.push(…)`) does NOT make a binding a signal.
/// - The `$.proxy(…)` wrapper is gated on `proxiable = should_proxy(init)` — the
///   initializer SHAPE alone, INDEPENDENT of reads/writes/mutations. An
///   object/array/call/member `$state` is proxied even when never written; a
///   literal/template-literal/arrow/unary/binary (or an identifier resolving to
///   one) is not.
/// - `$state.raw` NEVER proxies.
///
/// The resulting five-way decision:
///
/// - `$state.raw` + reassigned        → `RawStateSignal` (`$.state(value)`).
/// - `$state.raw` + not reassigned    → `PlainLet`.
/// - `$state` + proxiable + reassigned     → `StateProxy` (`$.state($.proxy(…))`).
/// - `$state` + proxiable + not reassigned → `BareProxy` (`let o = $.proxy(…)`).
/// - `$state` + not proxiable + reassigned     → `StateSignal` (`$.state(…)`).
/// - `$state` + not proxiable + not reassigned → `PlainLet`.
#[must_use]
pub fn classify_state_lowering(
    declared: StateRuneKind,
    proxiable: bool,
    uses: BindingUseSet,
) -> StateLowering {
    match declared {
        StateRuneKind::Raw => {
            // `$state.raw` never proxies; it is a bare signal only when reassigned,
            // otherwise an unwrapped plain `let`.
            if uses.reassigned {
                StateLowering::RawStateSignal
            } else {
                StateLowering::PlainLet
            }
        }
        StateRuneKind::State => {
            if proxiable {
                // Proxiable inits are ALWAYS `$.proxy(…)`; the `$.state(…)` wrapper
                // is added only when the binding is reassigned.
                if uses.reassigned {
                    StateLowering::StateProxy
                } else {
                    StateLowering::BareProxy
                }
            } else if uses.reassigned {
                // A non-proxiable (primitive) init that is reassigned is a bare
                // signal.
                StateLowering::StateSignal
            } else {
                // A non-proxiable init that is never reassigned stays a plain `let`.
                StateLowering::PlainLet
            }
        }
    }
}

/// A binding's analysis row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    /// The binding's source name.
    pub name: String,
    /// The lexical scope that introduces the binding.
    pub scope: ScopeId,
    /// The binding's runtime classification.
    pub kind: BindingRuntimeKind,
    /// The full `$state` classification, present only for `$state` bindings.
    pub state: Option<StateClassification>,
}

/// The binding table — every classified binding, addressable by [`BindingId`].
#[derive(Debug, Default, Clone)]
pub struct BindingTable {
    bindings: Vec<BindingInfo>,
}

impl BindingTable {
    /// A fresh empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a binding row, returning its id.
    pub fn push(&mut self, info: BindingInfo) -> BindingId {
        let id = BindingId(self.bindings.len() as u32);
        self.bindings.push(info);
        id
    }

    /// Look up a binding row by id.
    #[must_use]
    pub fn get(&self, id: BindingId) -> &BindingInfo {
        &self.bindings[id.0 as usize]
    }

    /// Mutably look up a binding row by id (used to finalize a `$state` binding's
    /// classification after template writes are attributed).
    pub fn get_mut(&mut self, id: BindingId) -> &mut BindingInfo {
        &mut self.bindings[id.0 as usize]
    }

    /// All binding rows.
    #[must_use]
    pub fn all(&self) -> &[BindingInfo] {
        &self.bindings
    }

    /// The runtime kind of the binding that resolves `name` in `scope` (the
    /// nearest binding up the scope chain), or `None` when `name` is free.
    ///
    /// This is the SCOPE-AWARE resolution the read/write rewrite consults: a
    /// shadowing local of the same name returns ITS kind, not the outer
    /// signal's.
    #[must_use]
    pub fn resolve_kind(
        &self,
        graph: &ScopeGraph,
        scope: ScopeId,
        name: &str,
    ) -> Option<BindingRuntimeKind> {
        graph.resolve(self, scope, name).map(|id| self.get(id).kind)
    }
}

/// A single lexical scope: its parent (for the scope chain) and the bindings it
/// directly introduces (`name → BindingId`).
#[derive(Debug, Clone)]
pub struct Scope {
    /// The enclosing scope, or `None` for the root.
    pub parent: Option<ScopeId>,
    /// The bindings introduced directly in this scope.
    pub bindings: FxHashMap<String, BindingId>,
}

/// The lexical scope graph: a parent-linked arena of scopes covering the script
/// scope, expression-local scopes (nested function/arrow params + bodies), and
/// the template block scopes (`{#each}` / `{#await}` / `{#snippet}` bodies).
#[derive(Debug, Default, Clone)]
pub struct ScopeGraph {
    scopes: Vec<Scope>,
}

impl ScopeGraph {
    /// A fresh graph with a single root scope. Returns the graph and its root id.
    #[must_use]
    pub fn with_root() -> (Self, ScopeId) {
        let mut graph = Self::default();
        let root = graph.push_scope(None);
        (graph, root)
    }

    /// Push a child scope under `parent`, returning its id.
    pub fn push_scope(&mut self, parent: Option<ScopeId>) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope {
            parent,
            bindings: FxHashMap::default(),
        });
        id
    }

    /// Bind `name` to `binding` in `scope` (a same-name binding in this scope is
    /// overwritten — the latest declaration wins within a single scope, matching
    /// JS shadowing within a scope).
    pub fn declare(&mut self, scope: ScopeId, name: &str, binding: BindingId) {
        self.scopes[scope.0 as usize]
            .bindings
            .insert(name.to_string(), binding);
    }

    /// The parent of `scope`.
    #[must_use]
    pub fn parent(&self, scope: ScopeId) -> Option<ScopeId> {
        self.scopes[scope.0 as usize].parent
    }

    /// Resolve `name` starting at `scope`, walking up the parent chain to the
    /// NEAREST binding. Returns the binding id of the nearest declaration, or
    /// `None` when `name` is free in the chain.
    ///
    /// The `table` parameter is accepted for symmetry with
    /// [`BindingTable::resolve_kind`]; resolution itself reads only the scope
    /// graph.
    #[must_use]
    pub fn resolve(&self, _table: &BindingTable, scope: ScopeId, name: &str) -> Option<BindingId> {
        let mut current = Some(scope);
        while let Some(s) = current {
            if let Some(&id) = self.scopes[s.0 as usize].bindings.get(name) {
                return Some(id);
            }
            current = self.scopes[s.0 as usize].parent;
        }
        None
    }

    /// The number of scopes in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    /// Whether the graph is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One reparsed, scope-annotated template expression.
#[derive(Debug, Clone)]
pub struct AnalyzedExpr<'a> {
    /// The raw expression source text (borrowed from the component source).
    pub source: &'a str,
    /// The lexical scope the expression is evaluated in.
    pub scope: ScopeId,
    /// The free identifier references in the expression, in source order, paired
    /// with whether each is an assignment TARGET (a write) vs a read.
    pub references: Vec<ExprReference>,
    /// When the expression is a DIRECT, non-optional, ZERO-ARGUMENT call on a plain
    /// IDENTIFIER callee (`render()`) — peeling through transparent
    /// `ParenthesizedExpression`s — the callee identifier name. `None` for every other
    /// shape (`render(x)`, `render?.()`, `obj.render()`, `new Foo()`, a bare identifier,
    /// a template). Drives the `{@html render()}` thunk-elision decision (the official
    /// compiler elides the `() => …` thunk to the bare callee ONLY when the callee
    /// rewrites unchanged). Harvested from the SAME parse that collects `references`
    /// (no second reparse, no synthesize-then-reparse).
    pub direct_zero_arg_call_callee: Option<String>,
    /// Whether the transparent-paren-unwrapped root is a `SequenceExpression`. The value
    /// printer re-wraps EXACTLY a top-level sequence in one paren pair (a bare `a, b` becomes
    /// `(a, b)` so it stays one value rather than splitting into positional arguments / object
    /// entries) — the one BEHAVIORAL value-position transform. Every other unwrapped root is
    /// emitted source-preserving (author parens kept verbatim).
    pub unwrapped_is_sequence: bool,
    /// The KIND of the transparent-paren-unwrapped root expression, for the `class={…}`
    /// `$.clsx` decision (the official `Attribute.js` rule reads the UNWRAPPED node type:
    /// a literal / template / binary value emits RAW, every other kind wraps in `$.clsx`).
    /// Computed on the same transparent-paren-unwrapped root.
    pub unwrapped_root_kind: UnwrappedRootKind,
    /// The ONE owned `bind:` target fact (classification / sequence presence / TS-wrapper
    /// validity / root identifier / plain-JS function-pair slices), computed ONCE from the
    /// SAME parse that produced this expression — the SINGLE authority every bind consumer
    /// reads instead of re-parsing the expression per consumer. Empty (default) for a
    /// non-bind expression (no `bind:` consumer reads it).
    pub bind_target: BindTargetFact,
}

/// The KIND of a value expression's transparent-paren-unwrapped root, restricted to the
/// distinction the `class={…}` `$.clsx` decision needs: the three official no-clsx-wrap
/// node kinds (a `Literal` family member, a `TemplateLiteral`, a `BinaryExpression`) vs
/// everything else. The official `Attribute.js` rule computes `needs_clsx` from the node
/// type of the value expression AFTER transparent author parens are removed — so a
/// parenthesized literal / template / binary stays unwrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnwrappedRootKind {
    /// A literal (string / numeric / boolean / null / bigint / regexp) — no `$.clsx` wrap.
    Literal,
    /// A `TemplateLiteral` (`` `a${b}c` ``) — no `$.clsx` wrap.
    TemplateLiteral,
    /// A `BinaryExpression` (`a + b`) — no `$.clsx` wrap.
    BinaryExpression,
    /// Every other expression kind (identifier / member / call / conditional / logical /
    /// object / array / sequence / unary / `new` / …) — wrapped in `$.clsx`.
    Other,
}

impl<'a> AnalyzedExpr<'a> {
    /// Build an analyzed expression from the per-parse [`ExprAnalysisFacts`].
    pub(crate) fn interned(source: &'a str, scope: ScopeId, facts: ExprAnalysisFacts) -> Self {
        Self {
            source,
            scope,
            references: facts.references,
            direct_zero_arg_call_callee: facts.direct_zero_arg_call_callee,
            unwrapped_is_sequence: facts.unwrapped_is_sequence,
            unwrapped_root_kind: facts.unwrapped_root_kind,
            bind_target: facts.bind_target,
        }
    }

    /// Build the analyzed expression for a TORN parse (the fragment did not parse cleanly):
    /// no references, treated as a non-sequence root with an unknown root kind and an empty
    /// bind-target fact (a torn bind target fails closed downstream).
    pub(crate) fn torn(source: &'a str, scope: ScopeId) -> Self {
        Self {
            source,
            scope,
            references: Vec::new(),
            direct_zero_arg_call_callee: None,
            unwrapped_is_sequence: false,
            unwrapped_root_kind: UnwrappedRootKind::Other,
            bind_target: BindTargetFact::default(),
        }
    }
}

/// One FREE identifier reference inside an analyzed expression (an
/// expression-local binding — a nested arrow/function param or a nested local —
/// is excluded). The reference kind drives WRITE attribution during `$state`
/// classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprReference {
    /// The referenced identifier name.
    pub name: String,
    /// How the identifier is referenced.
    pub kind: ExprRefKind,
    /// Whether the reference appears INSIDE a nested function / arrow body (a DEFERRED
    /// read, executed only when the function is called) vs SYNCHRONOUSLY at the
    /// expression's top level. The official `metadata.expression.has_state` counts only
    /// SYNCHRONOUS reactive reads — `onclick={() => x}` is a plain prop init (`has_state =
    /// false`), while `b={x}` / `depth={depth - 1}` are reactive (a getter / derived).
    pub in_function: bool,
}

/// How a free identifier is referenced inside a template expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprRefKind {
    /// A read (every non-write position).
    Read,
    /// A reassignment of the binding itself (`x = …` / `x += …` / `x++`).
    Reassign,
    /// A deep mutation of the binding's value (`x.a = …` / `x.a++`).
    DeepMutate,
}

impl ExprReference {
    /// Whether the reference is a write (a reassignment or a deep mutation).
    #[must_use]
    pub fn is_write(&self) -> bool {
        matches!(self.kind, ExprRefKind::Reassign | ExprRefKind::DeepMutate)
    }
}

/// The arena of analyzed template expressions, indexed by `ExprId`.
#[derive(Debug, Default)]
pub struct ExprArena<'a> {
    exprs: Vec<AnalyzedExpr<'a>>,
}

impl<'a> ExprArena<'a> {
    /// A fresh empty arena.
    #[must_use]
    pub fn new() -> Self {
        Self { exprs: Vec::new() }
    }

    /// Push an analyzed expression, returning its `ExprId`.
    pub fn push(&mut self, expr: AnalyzedExpr<'a>) -> super::ir::ExprId {
        let id = super::ir::ExprId(self.exprs.len() as u32);
        self.exprs.push(expr);
        id
    }

    /// Look up an analyzed expression by id.
    #[must_use]
    pub fn get(&self, id: super::ir::ExprId) -> &AnalyzedExpr<'a> {
        &self.exprs[id.0 as usize]
    }

    /// All analyzed expressions, in interning order.
    #[must_use]
    pub fn all(&self) -> &[AnalyzedExpr<'a>] {
        &self.exprs
    }

    /// The number of analyzed expressions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.exprs.len()
    }

    /// Whether the arena is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exprs.is_empty()
    }
}

/// The instance + module script analysis.
///
/// The reactivity mode is NOT carried here — it is owned solely by
/// [`ComponentIr::mode`](super::ir::ComponentIr::mode).
#[derive(Debug)]
pub struct ScriptAnalysis<'a> {
    /// The instance-script body source, if present.
    pub instance_source: Option<&'a str>,
    /// The module-script body source, if present.
    pub module_source: Option<&'a str>,
}

/// Reparse `text` as a TSX module with OXC, returning the parsed program (and the
/// owning parse result) for analysis. A fragment that does not parse cleanly
/// yields `None` (the caller fails open or records a diagnostic).
///
/// This mirrors the SINGLE reparse pattern the IDE scanners use
/// (`oxc_parser::Parser::new(&alloc, text, SourceType::tsx()).parse()`), so the
/// runtime analysis flows through the same grammar-correct front-end rather than
/// a parallel tokenizer.
pub fn reparse_module<'a>(alloc: &'a Allocator, text: &str) -> Option<Program<'a>> {
    let source_type = SourceType::tsx();
    let parsed = Parser::new(alloc, alloc.alloc_str(text), source_type).parse();
    // A panic OR a non-empty error set means the AST is partial / unreliable —
    // never feed it into rune / mode / state analysis. The caller fails open or
    // records a diagnostic rather than analyzing a torn parse.
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    Some(parsed.program)
}

pub use super::bind_target::{BindTargetFact, BindTargetKind};

/// Whether a callee expression is the `$state(…)` rune (vs `$state.raw` / a
/// shadowing local). Returns the declared flavour when it is a `$state` family
/// call.
#[must_use]
pub fn state_rune_call(call: &CallExpression<'_>) -> Option<StateRuneKind> {
    match &call.callee {
        // `$state(...)`.
        Expression::Identifier(id) if id.name.as_str() == "$state" => Some(StateRuneKind::State),
        // `$state.raw(...)`.
        Expression::StaticMemberExpression(m) => {
            if let Expression::Identifier(obj) = &m.object {
                if obj.name.as_str() == "$state" && m.property.name.as_str() == "raw" {
                    return Some(StateRuneKind::Raw);
                }
            }
            None
        }
        _ => None,
    }
}

/// Whether a CALLEE expression is the bare `$props` rune (`$props()` — NOT a
/// `$props.id` member access, NOT a shadowing local). The SINGLE shared
/// `$props`-callee predicate every syntax-side pass consults (the rune scan, the
/// binding classifier, the declaration lowering) — there is no per-module fork.
#[must_use]
pub(super) fn is_props_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "$props")
}

/// Whether a CALLEE expression is the bare `$effect` rune (`$effect(...)` — NOT
/// `$effect.pre` / `$effect.root` / `$effect.tracking`, NOT a shadowing local). The
/// SINGLE shared `$effect`-callee predicate.
#[must_use]
pub(super) fn is_effect_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "$effect")
}

/// Whether a CALLEE expression is a `$derived(...)` or `$derived.by(...)` rune — the
/// two callee forms that introduce a `Derived` memo binding (NOT a shadowing local).
/// The SINGLE shared `$derived`-callee predicate.
#[must_use]
pub(super) fn is_derived_callee(callee: &Expression<'_>) -> bool {
    match callee {
        // `$derived(...)`.
        Expression::Identifier(id) => id.name.as_str() == "$derived",
        // `$derived.by(...)`.
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(obj)
                if obj.name.as_str() == "$derived" && m.property.name.as_str() == "by")
        }
        _ => false,
    }
}

/// Whether an EXPRESSION is a `$bindable(...)` rune call — the default-value form
/// that marks a destructured `$props()` member as a BINDABLE prop. The SINGLE shared
/// `$bindable`-expression predicate.
#[must_use]
pub(super) fn is_bindable_call(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::CallExpression(call)
        if matches!(&call.callee, Expression::Identifier(id) if id.name.as_str() == "$bindable"))
}

/// Whether a `$state(…)` initializer's first argument is PROXIABLE — the
/// official `svelte@5.56.3` `should_proxy` predicate over the initializer SHAPE.
///
/// `should_proxy` is a NEGATIVE-LIST, default-TRUE predicate (NOT an
/// object/array/call whitelist): everything is proxiable EXCEPT a statically
/// non-proxiable expression — a literal, a template literal, an arrow / function
/// expression, a unary or binary expression, the identifier `undefined`, or an
/// identifier that resolves (ONE hop, via a non-reassigned scope binding whose
/// initializer is itself non-proxiable) to one of those. `$state()` with no
/// argument is `undefined` → not proxiable.
///
/// `scope_inits` maps an in-scope identifier NAME to its declarator initializer
/// (for the one-hop identifier follow) and the set of names that are reassigned
/// somewhere in the script (a reassigned intermediate blocks the follow, so it
/// stays proxiable). This mirrors `should_proxy(node, scope)` →
/// `should_proxy(binding.initial, null)`.
#[must_use]
pub fn init_is_proxiable(
    call: &CallExpression<'_>,
    scope_inits: &FxHashMap<String, ProxyInit>,
) -> bool {
    let Some(arg) = call.arguments.first() else {
        // `$state()` with no argument — `undefined`, not proxiable.
        return false;
    };
    let Some(expr) = arg.as_expression() else {
        // A spread argument is not a plain initializer — default to proxiable
        // (the official predicate's default-true for an unrecognised node).
        return true;
    };
    expr_is_proxiable(expr, Some(scope_inits))
}

/// The initializer fact for an in-scope identifier the one-hop proxy follow
/// needs: whether the name is reassigned anywhere (which blocks the follow), and
/// whether its declarator initializer is itself proxiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyInit {
    /// Whether the name is reassigned somewhere in the script.
    pub reassigned: bool,
    /// Whether the declarator initializer is proxiable (evaluated with NO further
    /// follow — matching `should_proxy(binding.initial, null)`).
    pub init_proxiable: bool,
    /// Whether the binding has an initializer that is followable (a `let`/`const`
    /// with an expression initializer, not a function/class/import/each/snippet).
    pub followable: bool,
}

/// The `should_proxy` predicate over an OXC expression. `scope_inits` enables the
/// ONE-hop identifier follow at the top level (passed `None` after a follow, so
/// the recursion never chains a second hop — matching the official compiler).
pub(crate) fn expr_is_proxiable(
    expr: &Expression<'_>,
    scope_inits: Option<&FxHashMap<String, ProxyInit>>,
) -> bool {
    match expr {
        // Statically non-proxiable expression forms (the official negative list).
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::UnaryExpression(_)
        | Expression::BinaryExpression(_) => false,
        Expression::Identifier(id) => {
            if id.name.as_str() == "undefined" {
                return false;
            }
            // The ONE-hop identifier follow: resolve to a non-reassigned scope
            // binding with a followable initializer and use ITS proxiability (no
            // further follow). Absent that, an identifier is proxiable.
            if let Some(inits) = scope_inits {
                if let Some(info) = inits.get(id.name.as_str()) {
                    if !info.reassigned && info.followable {
                        return info.init_proxiable;
                    }
                }
            }
            true
        }
        // Everything else (object / array / call / member / new / …) is proxiable.
        _ => true,
    }
}

/// A lexical-scope shadow stack.
///
/// Both the script use-collector and the template-expression reference collector
/// need the SAME lexical-scope model: an inner local (a `let`/`const`, a
/// function/arrow PARAMETER, a `catch` parameter, a `for`-loop binding, a nested
/// function declaration) SHADOWS an outer binding of the same name for reads,
/// writes, and reassignment marking. This stack holds one frame per active
/// lexical scope; a name is shadowed when any active frame declares it.
///
/// Modeling is conservative-correct for shadowing: a scope frame collects the
/// names declared DIRECTLY in that scope (params, the body's directly-declared
/// `let`/`const`/`var`/function-declaration ids — no descent into nested
/// scopes). `var` is collected on the introducing function/arrow/program frame
/// (function-scoped); `let`/`const` and the other forms on the block frame. The
/// frame is intentionally over-inclusive of `var` only on the current function
/// frame (never under-inclusive), so an inner `var` of the same name as an outer
/// binding still shadows.
#[derive(Default)]
pub(super) struct ShadowStack {
    frames: Vec<rustc_hash::FxHashSet<String>>,
}

impl ShadowStack {
    /// Whether `name` is shadowed by any active scope frame.
    pub(super) fn is_shadowed(&self, name: &str) -> bool {
        self.frames.iter().any(|f| f.contains(name))
    }

    /// Push a frame of names declared directly in a new scope.
    pub(super) fn push(&mut self, names: rustc_hash::FxHashSet<String>) {
        self.frames.push(names);
    }

    /// Pop the innermost frame.
    pub(super) fn pop(&mut self) {
        self.frames.pop();
    }
}

/// Collect the names a function/arrow PARAMETER list introduces.
fn param_names(params: &oxc_ast::ast::FormalParameters<'_>) -> rustc_hash::FxHashSet<String> {
    let mut names = Vec::new();
    for p in &params.items {
        collect_pattern_names(&p.pattern, &mut names);
    }
    if let Some(rest) = &params.rest {
        collect_pattern_names(&rest.rest.argument, &mut names);
    }
    names.into_iter().collect()
}

/// Collect the names a function BODY introduces in its own scope: the params, the
/// own id (a function EXPRESSION's recursion name), the body's directly-declared
/// `let`/`const`/function-declaration ids, plus every `var` hoisted anywhere
/// under the body (descending blocks / control flow but NOT nested functions).
pub(super) fn function_scope_names(func: &Function<'_>) -> rustc_hash::FxHashSet<String> {
    let mut names = param_names(&func.params);
    // A function EXPRESSION's own id is in scope inside its own body.
    if !matches!(func.r#type, FunctionType::FunctionDeclaration) {
        if let Some(id) = &func.id {
            names.insert(id.name.to_string());
        }
    }
    if let Some(body) = &func.body {
        collect_direct_decls(&body.statements, &mut names);
        collect_var_hoists(&body.statements, &mut names);
    }
    names
}

/// Collect an arrow's own-scope names (its params + its body's directly-declared
/// `let`/`const`/function-declaration ids + hoisted `var`s).
pub(super) fn arrow_scope_names(
    arrow: &ArrowFunctionExpression<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut names = param_names(&arrow.params);
    collect_direct_decls(&arrow.body.statements, &mut names);
    collect_var_hoists(&arrow.body.statements, &mut names);
    names
}

/// Collect a `{ … }` block scope's own names: the `let`/`const`/function-
/// declaration ids declared DIRECTLY in the block (no descent into nested blocks
/// or functions — `var` belongs to the enclosing function frame).
pub(super) fn block_scope_names(block: &BlockStatement<'_>) -> rustc_hash::FxHashSet<String> {
    let mut names = rustc_hash::FxHashSet::default();
    collect_direct_decls(&block.body, &mut names);
    names
}

/// Collect the `let`/`const`/function-declaration ids declared DIRECTLY in a
/// statement list (NOT descending nested blocks / functions / control flow). A
/// `var` is collected by [`collect_var_hoists`] on the function frame, not here.
pub(super) fn collect_direct_decls(
    stmts: &oxc_allocator::Vec<Statement<'_>>,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Statement::VariableDeclaration(v)
                if !matches!(v.kind, VariableDeclarationKind::Var) =>
            {
                for d in &v.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    out.extend(names);
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    out.insert(id.name.to_string());
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    out.insert(id.name.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Collect every `var` declared anywhere under a function body (descending blocks
/// / `for` / `if` / `catch` / `try`, since `var` hoists to the function scope) but
/// NOT into nested functions / arrows.
pub(super) fn collect_var_hoists(
    stmts: &oxc_allocator::Vec<Statement<'_>>,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    let mut scan = VarHoistScan { out };
    for stmt in stmts {
        scan.visit_statement(stmt);
    }
}

/// Scans `var` declarations under a function body without descending nested
/// function scopes.
struct VarHoistScan<'o> {
    out: &'o mut rustc_hash::FxHashSet<String>,
}

impl<'a> Visit<'a> for VarHoistScan<'_> {
    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        if matches!(it.kind, VariableDeclarationKind::Var) {
            for d in &it.declarations {
                let mut names = Vec::new();
                collect_pattern_names(&d.id, &mut names);
                self.out.extend(names);
            }
        }
    }
    // A nested function / arrow opens its own var scope — do not descend.
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}
}

/// Collect the BINDING NAMES introduced by a binding pattern (an identifier, an
/// object/array destructure, a rest, a default). The names are appended to `out`.
pub fn collect_pattern_names(pattern: &BindingPattern<'_>, out: &mut Vec<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.to_string()),
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_names(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_pattern_names(el, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_names(&assign.left, out);
        }
    }
}

/// Parse a binding-pattern text fragment (an each item, a snippet param list, an
/// await binding, a declaration-tag declarator) and collect its binding names.
///
/// The fragment is wrapped in `const [<text>] = null as any;` so a bare
/// identifier, a destructuring pattern, AND a comma-separated param list all
/// parse as one declarator's binding pattern (mirroring the IDE store-scan
/// pattern wrapper). A fragment that does not parse yields `Err(())` so the
/// caller can surface a diagnostic rather than silently dropping the names.
pub(crate) fn parse_pattern_names(pattern_text: &str) -> Result<Vec<String>, ()> {
    let alloc = Allocator::default();
    let wrapped = format!("const [{pattern_text}] = null as any;");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    let mut names = Vec::new();
    for stmt in &parsed.program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for d in &decl.declarations {
                collect_pattern_names(&d.id, &mut names);
            }
        }
    }
    Ok(names)
}

/// Parse a `let:`-directive alias value (`let:item={alias}`) and return the bare LOCAL name
/// IFF the value is a SINGLE bare binding identifier. The value is parsed (no text scan) as
/// `const <text> = null as any;`; only a top-level [`BindingPattern::BindingIdentifier`]
/// declarator id is a supported alias.
///
/// A destructuring pattern (`{ a }` / `[a]`, even a SINGLE-name one), a multi-declarator
/// list, or an unparseable value yields `None` so the caller fails CLOSED. The NODE KIND is
/// the discriminator, NOT a name count: a single-name destructure (`{ a }`) collects exactly
/// one name yet is an unsupported decomposition (it would otherwise lower as a one-name slot
/// rename), so the count alone cannot distinguish a bare-identifier alias from it.
pub(crate) fn parse_let_alias_identifier(value_text: &str) -> Option<String> {
    let alloc = Allocator::default();
    let wrapped = format!("const {value_text} = null as any;");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let mut decls = parsed.program.body.iter().filter_map(|stmt| match stmt {
        Statement::VariableDeclaration(decl) => Some(decl),
        _ => None,
    });
    let decl = decls.next()?;
    // Exactly ONE declarator whose id is a bare binding identifier — anything else (a
    // destructuring pattern, multiple declarators) is an unsupported alias form.
    if decls.next().is_some() || decl.declarations.len() != 1 {
        return None;
    }
    match &decl.declarations[0].id {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// One declarator parsed from a `{@const …}` / `{const …}` / `{let …}` tag's
/// inner text: its declared binding names (in source order) and the byte span of
/// its initializer expression RELATIVE to the inner text (i.e. `0` is the first
/// byte of the inner text). `init` is `None` for a bare declaration with no
/// initializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDeclarator {
    /// The declared binding names, in source order.
    pub names: Vec<String>,
    /// The initializer expression's `(start, end)` byte span relative to the
    /// inner text, if present.
    pub init: Option<(u32, u32)>,
}

/// The declarator keyword a declaration-tag inner text is wrapped with for the
/// OXC reparse. A `{let …}` tag wraps with `let` (so a no-initializer `{let x}`
/// is valid JS); a `{@const …}` / `{const …}` tag wraps with `const`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclaratorKeyword {
    /// Wrap with `const ` (`{@const}` / `{const}`).
    Const,
    /// Wrap with `let ` (`{let}` — may have no initializer).
    Let,
}

impl DeclaratorKeyword {
    /// The wrapper prefix (including the trailing space).
    fn prefix(self) -> &'static str {
        match self {
            Self::Const => "const ",
            Self::Let => "let ",
        }
    }
}

/// Parse a declaration-tag inner text (`{@const …}` / `{const …}` / `{let …}`,
/// the text AFTER the keyword) into its declarators via OXC.
///
/// This REPLACES the hand-rolled top-level-`=` byte splitter: the declared names
/// AND the initializer span both come from the OXC-parsed `VariableDeclarator`,
/// so a destructuring binding (`{a, b} = obj`) yields BOTH names and an
/// initializer expression positioned by the parser (no text scanning). The inner
/// text is parsed wrapped with the declarator `keyword` (`const `/`let `);
/// declarator/init spans are re-based to the inner text by subtracting the prefix
/// length. A fragment that does not parse yields `Err(())`.
///
/// The wrapper keyword matters: a no-initializer declaration tag (`{let x}`)
/// requires a `let` wrapper (`const x;` is invalid JS — `const` demands an
/// initializer — so a `const` wrapper would drop a valid `{let x}` tag).
pub(crate) fn parse_declarators(
    inner_text: &str,
    keyword: DeclaratorKeyword,
) -> Result<Vec<ParsedDeclarator>, ()> {
    let prefix = keyword.prefix();
    let alloc = Allocator::default();
    let wrapped = format!("{prefix}{inner_text};");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    let prefix_len = prefix.len() as u32;
    let mut out = Vec::new();
    for stmt in &parsed.program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let mut names = Vec::new();
            collect_pattern_names(&d.id, &mut names);
            let init = d.init.as_ref().map(|expr| {
                let span = expr.span();
                (
                    span.start.saturating_sub(prefix_len),
                    span.end.saturating_sub(prefix_len),
                )
            });
            out.push(ParsedDeclarator { names, init });
        }
    }
    Ok(out)
}

/// Parse a `{@debug …}` tag inner text into the `(start, end)` byte spans (relative
/// to the inner text) of its comma-separated debug arguments, via OXC — REJECTING a
/// non-identifier argument.
///
/// `{@debug a, b}` lowers to TWO debug expressions (`a` and `b`), NOT one
/// `SequenceExpression` — the official `DebugTag` walks `node.identifiers`
/// individually to build `console.log({ a: $.snapshot(a), b: $.snapshot(b) })`. The
/// split is structural (the parsed `SequenceExpression.expressions`), never a
/// top-level-comma byte scan. Official rejects ANY non-identifier argument with
/// `debug_tag_invalid_arguments` (the object key + snapshot must be a bare name), so a
/// member / call / binary / conditional / … argument returns `Err(())` here. An empty
/// inner (`{@debug}`) yields no arguments (the valid `console.log({})` form). A fragment
/// that does not parse also returns `Err(())`.
pub(crate) fn parse_debug_identifier_spans(inner_text: &str) -> Result<Vec<DebugIdentifier>, ()> {
    if inner_text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let alloc = Allocator::default();
    // No prefix is prepended, so the parsed spans are already relative to the inner
    // text (`a, b;` → the `ExpressionStatement`'s `SequenceExpression` elements).
    let wrapped = format!("{inner_text};");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    for stmt in &parsed.program.body {
        let Statement::ExpressionStatement(es) = stmt else {
            continue;
        };
        let args: Vec<&Expression> = match &es.expression {
            Expression::SequenceExpression(seq) => seq.expressions.iter().collect(),
            other => vec![other],
        };
        let mut idents = Vec::with_capacity(args.len());
        for arg in args {
            // Official `debug_tag_invalid_arguments`: every argument must be a bare
            // identifier reference, never an arbitrary expression (member / call /
            // binary / conditional / …). This is a STRUCTURAL check on the parsed node.
            let Expression::Identifier(id) = arg else {
                return Err(());
            };
            // The object key is the PARSED identifier NAME (the decoded
            // `IdentifierReference.name`), carried alongside its byte span — NOT a raw
            // source slice (which would keep a Unicode-escaped identifier's raw escape
            // bytes verbatim instead of the decoded name).
            let span = arg.span();
            idents.push(DebugIdentifier {
                name: id.name.as_str().to_string(),
                start: span.start,
                end: span.end,
            });
        }
        return Ok(idents);
    }
    Ok(Vec::new())
}

/// A parsed `{@debug}` argument: the decoded identifier NAME (the object key) plus its
/// byte span relative to the tag's inner text. The name is the typed fact the debug
/// projector keys on, so the emitted `{ <name>: $.snapshot(<expr>) }` object key is the
/// parsed identifier — never a re-sliced raw source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugIdentifier {
    /// The decoded identifier name (the debug object key).
    pub(crate) name: String,
    /// The byte-span start, relative to the `{@debug}` inner text.
    pub(crate) start: u32,
    /// The byte-span end, relative to the `{@debug}` inner text.
    pub(crate) end: u32,
}

/// The parsed shape of a `{@render …}` call's callee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderCalleeShape {
    /// A call whose callee is a plain identifier (after peeling transparent author
    /// parens), e.g. `row(1)`, `(row)(1)`, or the OPTIONAL `row?.(1)`. Carries the
    /// callee identifier name, whether the call is optional, and each argument's
    /// `(start, end)` byte span relative to the inner text.
    StaticName {
        /// The callee identifier name.
        name: String,
        /// Whether the trailing call is the optional `?.()` form.
        optional: bool,
        /// The argument expression spans, relative to the inner text.
        args: Vec<(u32, u32)>,
    },
    /// Anything else — an optional call (`expr?.()`), a member/computed callee, a
    /// non-call expression: the whole inner expression is the dynamic callee. The
    /// trailing call's ARGUMENT spans are still carried (relative to the inner
    /// text) so a prop/member/optional callee keeps its argument thunks.
    Dynamic {
        /// The argument expression spans, relative to the inner text (empty for a
        /// zero-arg call or a non-call expression).
        args: Vec<(u32, u32)>,
    },
    /// A `{@render …(…)}` call carrying a SPREAD argument (`{@render row(...xs)}`).
    /// Official `svelte@5.56.3` HARD-ERRORS on this (`render_tag_invalid_spread_argument`:
    /// "cannot use spread arguments in {@render ...} tags"), so it is the fail-closed
    /// signal — the caller refuses rather than degrading to dropped (un-thunk-able) args.
    /// Independent of the callee shape (a spread can ride a static-name, member, or
    /// optional callee), so it is its own arm: the refusal is about the ARGUMENT, the
    /// callee identity is irrelevant once a spread is present.
    SpreadArguments,
}

/// Parse a `{@render …}` tag's inner text into its callee shape.
///
/// A call whose callee peels (through transparent author parens) to a plain
/// identifier — `row(1)`, `(row)(1)`, or the optional `row?.(1)` — yields
/// [`RenderCalleeShape::StaticName`] (the static-snippet-name candidate, carrying the
/// `optional` flag); ANYTHING else — a call-expression callee (`getSnippet()?.()`), a
/// member/computed callee (`obj.snip(x)`), or a non-call expression — yields
/// [`RenderCalleeShape::Dynamic`].
/// In BOTH call shapes the trailing call's ARGUMENT spans (relative to the inner
/// text) are carried, so a prop/member/optional callee keeps its argument thunks
/// (the official `$.snippet(node, callee, …args)` shape). This is purely structural
/// (no text matching): it inspects the OXC-parsed `CallExpression`. A fragment that
/// does not parse yields `Err(())`.
pub(crate) fn parse_render_call(inner_text: &str) -> Result<RenderCalleeShape, ()> {
    let alloc = Allocator::default();
    let wrapped = format!("({inner_text});");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    // The single wrapped expression statement.
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return Ok(RenderCalleeShape::Dynamic { args: Vec::new() });
    };
    // Peel ALL outer parenthesised layers — the synthetic wrapper paren plus any author
    // parens wrapping the WHOLE call — so the trailing-call peel and the per-argument
    // spread scan reach the call's top-level arguments. A single unwrap would leave an
    // author-parenthesised whole call (`(row(...xs))`, `((row(...xs)))`, `(row?.(...xs))`)
    // as a residual `ParenthesizedExpression` that matches neither a Call nor a Chain and
    // falls through to the empty-args dynamic shape, silently DROPPING a spread argument
    // the caller must instead fail closed on.
    let mut inner_expr = &stmt.expression;
    while let Expression::ParenthesizedExpression(p) = inner_expr {
        inner_expr = &p.expression;
    }
    // Peel the trailing call — a plain `CallExpression` or the `CallExpression`
    // inside a `ChainExpression` (an optional `fn?.()`). A non-call expression has
    // no arguments to carry (the dynamic callee is the whole expression).
    let call: &CallExpression = match inner_expr {
        Expression::CallExpression(call) => call,
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => call,
            _ => return Ok(RenderCalleeShape::Dynamic { args: Vec::new() }),
        },
        _ => return Ok(RenderCalleeShape::Dynamic { args: Vec::new() }),
    };
    // The wrapper prefix is a single `(`.
    let prefix_len = 1u32;
    let mut args = Vec::with_capacity(call.arguments.len());
    for arg in &call.arguments {
        let Some(expr) = arg.as_expression() else {
            // A spread argument (`row(...xs)`) is an official HARD ERROR
            // (`render_tag_invalid_spread_argument`). Signal the fail-closed shape so
            // the caller refuses, rather than silently dropping the (un-thunk-able)
            // spread and emitting a wrong-arity `$.snippet` call.
            return Ok(RenderCalleeShape::SpreadArguments);
        };
        let span = expr.span();
        args.push((
            span.start.saturating_sub(prefix_len),
            span.end.saturating_sub(prefix_len),
        ));
    }
    // A call whose callee is a plain identifier — after peeling TRANSPARENT author
    // parens (`(row)(1)` roots at `row`; estree/acorn has no ParenthesizedExpression
    // node, so official sees the bare identifier) — is the static snippet-name
    // candidate, for BOTH the plain and the optional (`row?.(…)`) call. A
    // member/computed/call callee stays the dynamic callee, carrying the same
    // argument thunks.
    let mut callee = &call.callee;
    while let Expression::ParenthesizedExpression(p) = callee {
        callee = &p.expression;
    }
    if let Expression::Identifier(ident) = callee {
        return Ok(RenderCalleeShape::StaticName {
            name: ident.name.to_string(),
            optional: call.optional,
            args,
        });
    }
    Ok(RenderCalleeShape::Dynamic { args })
}

/// A use-collector over a script body: it records, per tracked `$state` binding
/// name, whether the binding is reassigned (`x = …`) or deep-mutated (`x.a = …` /
/// `x.a++`). These WRITE facts are the lowering determinant (a never-written
/// `$state` collapses to a plain `let` — verified against the pinned compiler).
///
/// It tracks a real lexical-scope stack ([`ShadowStack`]) so a nested local of
/// the same name — a function/arrow PARAMETER, a `let`/`const`, a `catch`
/// parameter, a `for`-loop binding, or a nested function declaration — SHADOWS a
/// tracked binding (its uses are NOT attributed to the outer binding).
#[derive(Default)]
pub struct ScriptUseCollector {
    /// Per-binding-name use sets (only for names the caller seeded as tracked).
    uses: FxHashMap<String, BindingUseSet>,
    /// Names tracked at the script (top) scope.
    tracked: rustc_hash::FxHashSet<String>,
    /// The active lexical-scope shadow stack.
    scopes: ShadowStack,
}

impl ScriptUseCollector {
    /// Create a collector tracking the given top-scope binding names.
    #[must_use]
    pub fn tracking(names: &[String]) -> Self {
        let mut c = Self::default();
        for n in names {
            c.tracked.insert(n.clone());
            c.uses.insert(n.clone(), BindingUseSet::default());
        }
        c
    }

    /// The accumulated use set for a tracked binding name.
    #[must_use]
    pub fn use_set(&self, name: &str) -> BindingUseSet {
        self.uses.get(name).copied().unwrap_or_default()
    }

    /// Whether `name` is a tracked, non-shadowed binding (so its uses count).
    fn is_active_tracked(&self, name: &str) -> bool {
        self.tracked.contains(name) && !self.scopes.is_shadowed(name)
    }

    fn mark_reassigned(&mut self, name: &str) {
        if self.is_active_tracked(name) {
            if let Some(u) = self.uses.get_mut(name) {
                u.reassigned = true;
            }
        }
    }

    fn mark_deep_mutated(&mut self, name: &str) {
        if self.is_active_tracked(name) {
            if let Some(u) = self.uses.get_mut(name) {
                u.deep_mutated = true;
            }
        }
    }
}

impl<'a> Visit<'a> for ScriptUseCollector {
    fn visit_program(&mut self, it: &Program<'a>) {
        // The program (script) scope: its own top-level `let`/`const`/function
        // ids + hoisted `var`s. (The tracked $state names live here; pushing them
        // is harmless — `is_active_tracked` already gates on `tracked`.)
        let mut frame = rustc_hash::FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        // Do NOT shadow the tracked names with the program frame — they are the
        // bindings being classified, declared at this very scope.
        for n in &self.tracked {
            frame.remove(n);
        }
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        // The catch body's own `let`/`const` also live in (a child of) this scope;
        // fold them into the same frame (the body block does not re-push here —
        // walk_catch_clause visits the body block, which pushes its own frame, so
        // collecting them here too is safe over-inclusion for shadowing).
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        // A bare-identifier OR destructuring-assignment target is a REASSIGN of the
        // binding(s); a member-rooted target (`x.a = …`) is a DEEP MUTATION of the
        // binding's value.
        match &it.left {
            AssignmentTarget::StaticMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    self.mark_deep_mutated(obj.name.as_str());
                }
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    self.mark_deep_mutated(obj.name.as_str());
                }
            }
            other => {
                // Bare identifier and every destructuring-assignment target shape.
                let mut names = rustc_hash::FxHashSet::default();
                collect_reassigned_target_names(other, &mut names);
                for name in &names {
                    self.mark_reassigned(name);
                }
            }
        }
        // Visit the target itself so a NESTED write inside a computed-member key
        // (`a[b = 5] = 1` — the `b = 5`) or a default expression is still observed.
        // `mark_*` is driven only by the explicit arms above, so re-walking the
        // target adds no spurious reassign/mutation facts — it only descends into
        // sub-expressions that may themselves contain writes.
        walk::walk_assignment_target(self, &it.left);
        // The RHS is a read context — visit it normally.
        self.visit_expression(&it.right);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        match &it.argument {
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    self.mark_deep_mutated(obj.name.as_str());
                }
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    self.mark_deep_mutated(obj.name.as_str());
                }
            }
            // A bare-identifier OR a TS-WRAPPED identifier (`count!++` /
            // `(count as T)++`) update reassigns the inner identifier (the wrapper
            // is type-only and strips away).
            other => {
                if let Some(name) = simple_target_wrapped_ident(other) {
                    self.mark_reassigned(name);
                }
            }
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // A mutating method call on a binding (`items.push(…)`, `map.set(…)`) is a
        // DEEP MUTATION fact. It does NOT make a runes `$state` a signal (the proxy
        // decision is init-shape-driven), but it is a neutral fact retained for
        // diagnostics / future SSR-legacy analysis.
        if let Expression::StaticMemberExpression(m) = &it.callee {
            if let Expression::Identifier(obj) = &m.object {
                self.mark_deep_mutated(obj.name.as_str());
            }
        } else if let Expression::ComputedMemberExpression(m) = &it.callee {
            if let Expression::Identifier(obj) = &m.object {
                self.mark_deep_mutated(obj.name.as_str());
            }
        }
        walk::walk_call_expression(self, it);
    }
}

/// Peel TS-WRAPPER layers (`x!`, `x as T`, `x satisfies T`, `<T>x`) off a SIMPLE
/// assignment target and return the inner bare-identifier name, if it reduces to
/// one. A TS-wrapped update/assignment of an identifier (`count!++`) is still a
/// reassignment of that identifier (the type wrapper strips away), so the write
/// attribution must see through it — otherwise a `$state` written ONLY through a
/// wrapper is misclassified as never-reassigned. A member target reduces to `None`.
fn simple_target_wrapped_ident<'a>(target: &'a SimpleAssignmentTarget<'a>) -> Option<&'a str> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.as_str()),
        SimpleAssignmentTarget::TSAsExpression(e) => expr_wrapped_ident(&e.expression),
        SimpleAssignmentTarget::TSSatisfiesExpression(e) => expr_wrapped_ident(&e.expression),
        SimpleAssignmentTarget::TSNonNullExpression(e) => expr_wrapped_ident(&e.expression),
        SimpleAssignmentTarget::TSTypeAssertion(e) => expr_wrapped_ident(&e.expression),
        _ => None,
    }
}

/// Peel TS-wrapper / parenthesis layers off an EXPRESSION and return the inner
/// bare-identifier name, if it reduces to one.
fn expr_wrapped_ident<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::ParenthesizedExpression(p) => expr_wrapped_ident(&p.expression),
        Expression::TSAsExpression(e) => expr_wrapped_ident(&e.expression),
        Expression::TSSatisfiesExpression(e) => expr_wrapped_ident(&e.expression),
        Expression::TSNonNullExpression(e) => expr_wrapped_ident(&e.expression),
        Expression::TSTypeAssertion(e) => expr_wrapped_ident(&e.expression),
        _ => None,
    }
}

/// Collect the bare-identifier names that an assignment TARGET reassigns,
/// INCLUDING destructuring-assignment targets (`({ count } = …)`, `[a] = …`,
/// `[a, ...rest] = …`, `({ x: y } = …)`). A member-rooted target (`x.a = …`) is
/// a DEEP mutation, NOT a reassignment of the identifier, so its root object is
/// NOT collected here. The collected names are appended to `out`.
///
/// Destructuring-assignment reassignment is a `$state` lowering determinant —
/// `let count = $state(0); ({ count } = obj)` is a reassignment (verified against
/// the pinned compiler: it lowers `count` to `$.state(0)`), so the destructured
/// identifier targets MUST be attributed as reassignments, not dropped.
pub(crate) fn collect_reassigned_target_names(
    target: &AssignmentTarget<'_>,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    use oxc_ast::ast::AssignmentTargetProperty;
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            out.insert(id.name.to_string());
        }
        // A member-rooted target is a deep mutation, not a reassignment — skip.
        AssignmentTarget::StaticMemberExpression(_)
        | AssignmentTarget::ComputedMemberExpression(_)
        | AssignmentTarget::PrivateFieldExpression(_) => {}
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            for el in arr.elements.iter().flatten() {
                collect_maybe_default_target_names(el, out);
            }
            if let Some(rest) = &arr.rest {
                collect_reassigned_target_names(&rest.target, out);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            for prop in &obj.properties {
                match prop {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                        // `({ count } = …)` / `({ count = d } = …)` — the shorthand
                        // identifier IS the reassigned binding.
                        out.insert(p.binding.name.to_string());
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                        collect_maybe_default_target_names(&p.binding, out);
                    }
                }
            }
            if let Some(rest) = &obj.rest {
                collect_reassigned_target_names(&rest.target, out);
            }
        }
        // A `[a = d]` / `{x: y = d}` default wrapper is reached through
        // `collect_maybe_default_target_names`; it does not appear as a bare
        // top-level target.
        _ => {}
    }
}

/// Collect reassigned identifier names from an array element / object-property
/// value target, unwrapping a `= default` wrapper.
fn collect_maybe_default_target_names(
    target: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    use oxc_ast::ast::AssignmentTargetMaybeDefault;
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            collect_reassigned_target_names(&d.binding, out);
        }
        other => {
            if let Some(t) = other.as_assignment_target() {
                collect_reassigned_target_names(t, out);
            }
        }
    }
}

/// Collect the lexical names a `for-of` / `for-in` LEFT binds (a `for (const x of
/// …)` block-scopes `x`; a `var` left hoists to the enclosing function frame and
/// is handled there, so it is not collected here).
pub(super) fn for_left_names(
    left: &oxc_ast::ast::ForStatementLeft<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut frame = rustc_hash::FxHashSet::default();
    if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) = left {
        if !matches!(decl.kind, VariableDeclarationKind::Var) {
            for d in &decl.declarations {
                let mut names = Vec::new();
                collect_pattern_names(&d.id, &mut names);
                frame.extend(names);
            }
        }
    }
    frame
}

/// The per-expression facts harvested from ONE OXC parse of a template-expression
/// text: its free references, the direct-zero-arg-identifier-call callee fact, and the
/// two transparent-paren-unwrapped root facts (whether the root is a `SequenceExpression`
/// — the BEHAVIORAL sequence-wrap signal — and its KIND for the `class={…}` `$.clsx`
/// decision). Both root facts peel transparent outer parens internally to CLASSIFY the
/// root; neither slices an emitted-source span (the value printer is source-preserving).
pub(crate) struct ExprAnalysisFacts {
    /// The free identifier references (read vs write), in source order.
    pub references: Vec<ExprReference>,
    /// The callee name when the WHOLE expression is a direct, non-optional, zero-arg
    /// identifier call (peeling transparent parens); `None` otherwise.
    pub direct_zero_arg_call_callee: Option<String>,
    /// Whether the transparent-paren-unwrapped root is a `SequenceExpression` — the value
    /// printer re-wraps EXACTLY a top-level sequence in one paren pair (a bare `a, b` becomes
    /// `(a, b)` so it stays one value), the one BEHAVIORAL value-position transform.
    pub unwrapped_is_sequence: bool,
    /// The KIND of the transparent-paren-unwrapped root (for the `class={…}` `$.clsx`
    /// decision).
    pub unwrapped_root_kind: UnwrappedRootKind,
    /// The owned `bind:` target fact, derived from the SAME parsed expression (the
    /// structural fields reuse this parse; the function-pair plain-JS slices come from one
    /// optional `mjs` parse gated on sequence presence).
    pub bind_target: BindTargetFact,
}

/// Collect the FREE identifier references of a single template-expression text,
/// classifying each as a read or a write (assignment / update target), PLUS the
/// direct-zero-arg-identifier-call callee fact — both from ONE parse. The references
/// EXCLUDE identifiers bound LOCALLY inside the expression (nested arrow/function
/// params + nested locals), so a template expression's free references are the ones
/// whose meaning is decided by the enclosing template/script scope.
///
/// A fragment that does not parse cleanly yields `Err(())` so the caller can surface a
/// parse diagnostic rather than silently returning no references.
pub(crate) fn collect_expr_references(text: &str) -> Result<ExprAnalysisFacts, ()> {
    use oxc_ast::ast::{Expression, Statement};
    let alloc = Allocator::default();
    // Wrap as a parenthesised expression statement so a bare expression body
    // (`count + 1`, `() => count++`, `box.a`) parses as a module statement.
    let wrapped = format!("({text});");
    let parsed = Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    let mut collector = ExprReferenceCollector {
        refs: Vec::new(),
        local_frames: Vec::new(),
        fn_depth: 0,
    };
    collector.visit_program(&parsed.program);
    // The whole expression body (the `({text});` wrapper's lone expression statement) —
    // the SAME parsed program both downstream facts read (no second reparse).
    let body_expr = parsed.program.body.first().and_then(|stmt| match stmt {
        Statement::ExpressionStatement(s) => Some(&s.expression),
        _ => None,
    });
    // The direct-zero-arg-identifier-call fact: the WHOLE expression is a non-optional,
    // zero-argument `CallExpression` whose callee peels (through transparent parens) to a
    // plain identifier.
    let direct_zero_arg_call_callee = body_expr.and_then(|expr| {
        let mut e = expr;
        while let Expression::ParenthesizedExpression(p) = e {
            e = &p.expression;
        }
        match e {
            Expression::CallExpression(call) if !call.optional && call.arguments.is_empty() => {
                // The callee may itself be wrapped in transparent author parens
                // (`(render)()` / `((render))()` — OXC represents each `(…)` as a
                // `ParenthesizedExpression`). Peel them, the SAME transparent-paren peel
                // the whole-expression walk above does, before the identifier check, so a
                // paren-wrapped zero-arg call still harvests its callee name.
                let mut callee = &call.callee;
                while let Expression::ParenthesizedExpression(p) = callee {
                    callee = &p.expression;
                }
                match callee {
                    Expression::Identifier(id) => Some(id.name.to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    });
    // The transparent-paren-unwrapped root facts: peel every transparent outer
    // `ParenthesizedExpression` to the root operand, then CLASSIFY it — whether it is a
    // `SequenceExpression` (the BEHAVIORAL sequence-wrap signal) and its KIND (the `class={…}`
    // `$.clsx` decision). This is pure analysis: it never slices an emitted-source span (the
    // value printer keeps the author's parens verbatim, so no paren-removal slice is needed).
    let (unwrapped_is_sequence, unwrapped_root_kind) = match body_expr {
        Some(expr) => {
            let mut inner = expr;
            while let Expression::ParenthesizedExpression(p) = inner {
                inner = &p.expression;
            }
            (
                matches!(inner, Expression::SequenceExpression(_)),
                unwrapped_root_kind_of(inner),
            )
        }
        None => (false, UnwrappedRootKind::Other),
    };
    // The `bind:` target fact, derived from the SAME parsed `body_expr` (NO extra parse for
    // the structural fields). The plain-JS function-pair slices come from at most one
    // optional `mjs` parse INSIDE the constructor, gated on sequence presence — stored ONCE
    // here so no downstream bind consumer re-parses the expression. A non-bind expression
    // (no `body_expr`, or a non-bind shape) yields the empty default fact.
    let bind_target = body_expr
        .map(|expr| BindTargetFact::from_parsed_target(expr, &alloc, text))
        .unwrap_or_default();
    Ok(ExprAnalysisFacts {
        references: collector.refs,
        direct_zero_arg_call_callee,
        unwrapped_is_sequence,
        unwrapped_root_kind,
        bind_target,
    })
}

/// Classify a value expression's transparent-paren-unwrapped root into the
/// [`UnwrappedRootKind`] the `class={…}` `$.clsx` decision needs — the three official
/// no-clsx-wrap node kinds (`Literal` family / `TemplateLiteral` / `BinaryExpression`) vs
/// everything else.
fn unwrapped_root_kind_of(expr: &Expression) -> UnwrappedRootKind {
    match expr {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::StringLiteral(_) => UnwrappedRootKind::Literal,
        Expression::TemplateLiteral(_) => UnwrappedRootKind::TemplateLiteral,
        Expression::BinaryExpression(_) => UnwrappedRootKind::BinaryExpression,
        _ => UnwrappedRootKind::Other,
    }
}

/// Collects free identifier references (read vs write) from a wrapped expression,
/// excluding identifiers bound by ANY nested lexical scope inside the expression
/// — a function/arrow PARAMETER, a nested `let`/`const`/`var`, a `catch`
/// parameter, a `for`-loop binding, or a nested function declaration. It uses the
/// SAME lexical-scope model as the script use-collector ([`ShadowStack`] +
/// [`function_scope_names`] / [`arrow_scope_names`] / [`block_scope_names`] /
/// [`for_left_names`]), so an inner local of the same name as an outer signal is
/// NOT reported as a free reference (a write to it is NOT attributed to the outer
/// binding).
struct ExprReferenceCollector {
    refs: Vec<ExprReference>,
    local_frames: Vec<rustc_hash::FxHashSet<String>>,
    /// The current nested-function depth — incremented inside a `function` / arrow body
    /// ONLY (NOT a plain block / catch / for), so a reference's `in_function` flag marks a
    /// DEFERRED read (the official synchronous-`has_state` distinction).
    fn_depth: usize,
}

impl ExprReferenceCollector {
    fn is_local(&self, name: &str) -> bool {
        self.local_frames.iter().any(|f| f.contains(name))
    }

    /// Whether the collector is currently inside a nested function / arrow body.
    fn in_function(&self) -> bool {
        self.fn_depth > 0
    }
}

impl<'a> Visit<'a> for ExprReferenceCollector {
    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.local_frames.push(function_scope_names(it));
        self.fn_depth += 1;
        walk::walk_function(self, it, flags);
        self.fn_depth -= 1;
        self.local_frames.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.local_frames.push(arrow_scope_names(it));
        self.fn_depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.fn_depth -= 1;
        self.local_frames.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.local_frames.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.local_frames.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.local_frames.push(frame);
        walk::walk_catch_clause(self, it);
        self.local_frames.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.local_frames.push(frame);
        walk::walk_for_statement(self, it);
        self.local_frames.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.local_frames.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.local_frames.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.local_frames.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.local_frames.pop();
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        match &it.left {
            // A member-rooted target (`x.a = …` / `x[i] = …`) is a DEEP MUTATION
            // of the binding's value.
            AssignmentTarget::StaticMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    let name = obj.name.as_str();
                    if !self.is_local(name) {
                        let in_function = self.in_function();
                        self.refs.push(ExprReference {
                            name: name.to_string(),
                            kind: ExprRefKind::DeepMutate,
                            in_function,
                        });
                    }
                }
                // Visit any computed sub-expressions / object as reads below.
                walk::walk_assignment_target(self, &it.left);
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    let name = obj.name.as_str();
                    if !self.is_local(name) {
                        let in_function = self.in_function();
                        self.refs.push(ExprReference {
                            name: name.to_string(),
                            kind: ExprRefKind::DeepMutate,
                            in_function,
                        });
                    }
                }
                walk::walk_assignment_target(self, &it.left);
            }
            other => {
                // A bare-identifier OR destructuring-assignment target is a REASSIGN
                // of each named binding. `({ count } = obj)` / `[a] = arr` are
                // reassignments of `count` / `a`, not reads.
                let mut names = rustc_hash::FxHashSet::default();
                collect_reassigned_target_names(other, &mut names);
                let in_function = self.in_function();
                for name in &names {
                    if !self.is_local(name) {
                        self.refs.push(ExprReference {
                            name: name.clone(),
                            kind: ExprRefKind::Reassign,
                            in_function,
                        });
                    }
                }
                // Also visit the target for any computed sub-expressions (read
                // contexts inside the pattern).
                walk::walk_assignment_target(self, &it.left);
            }
        }
        self.visit_expression(&it.right);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // A method call on a binding (`obj.method(…)`) is a READ of the receiver, NOT a
        // write — even a genuinely-mutating method (`arr.push(…)`) is, statically, a read
        // (the mutation happens at runtime through the value/proxy). Official `svelte`
        // never classifies a `CallExpression` as a write: a `$props()` prop method-call
        // (`{p.toString()}`) compiles to a plain `$$props.p.toString()` READ, and a
        // `$state` lowering is driven by `should_proxy(init)` + reassignment ALONE (a
        // method call is irrelevant — see `classify_state_lowering`). So a method call
        // contributes only its receiver's READ reference (collected by the member-walk),
        // never a `DeepMutate` write fact; an OVER-classification here was wrongly
        // refusing a prop method-call in a template value as a "written prop". The
        // receiver's read + any computed-key / argument reads are collected by descending.
        walk::walk_call_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        match &it.argument {
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    let name = obj.name.as_str();
                    if !self.is_local(name) {
                        let in_function = self.in_function();
                        self.refs.push(ExprReference {
                            name: name.to_string(),
                            kind: ExprRefKind::DeepMutate,
                            in_function,
                        });
                    }
                }
                walk::walk_update_expression(self, it);
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                if let Expression::Identifier(obj) = &m.object {
                    let name = obj.name.as_str();
                    if !self.is_local(name) {
                        let in_function = self.in_function();
                        self.refs.push(ExprReference {
                            name: name.to_string(),
                            kind: ExprRefKind::DeepMutate,
                            in_function,
                        });
                    }
                }
                walk::walk_update_expression(self, it);
            }
            // A bare-identifier OR a TS-WRAPPED identifier (`count!++`) update is a
            // REASSIGN of the inner identifier (the type wrapper strips away).
            other => {
                if let Some(name) = simple_target_wrapped_ident(other) {
                    if !self.is_local(name) {
                        let in_function = self.in_function();
                        self.refs.push(ExprReference {
                            name: name.to_string(),
                            kind: ExprRefKind::Reassign,
                            in_function,
                        });
                    }
                }
                walk::walk_update_expression(self, it);
            }
        }
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let name = it.name.as_str();
        if !self.is_local(name) {
            let in_function = self.in_function();
            self.refs.push(ExprReference {
                name: name.to_string(),
                kind: ExprRefKind::Read,
                in_function,
            });
        }
        walk::walk_identifier_reference(self, it);
    }
}
