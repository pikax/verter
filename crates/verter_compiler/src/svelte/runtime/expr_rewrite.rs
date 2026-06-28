//! The FALLIBLE AST-backed Svelte client EXPRESSION rewriter (the two-pass core).
//!
//! This is the EMISSION-grade rewriter the client backend drives: a real walk of
//! the OXC expression AST that resolves each identifier scope-awarely against the
//! binding table and rewrites reads (`$.get`) / writes (`$.set` / `$.update`) /
//! prop reads, REFUSING (a typed `Err`) on a parse failure or an unsupported form
//! (an `await`, a destructuring write to a signal, a TS-wrapped reactive write
//! target) — never verbatim output.
//!
//! Two passes:
//! 1. [`BindingOccurrenceCollector`] — a COMPLETE scope-aware walk recording every
//!    binding-bearing read / reassign / update as a TYPED [`Occurrence`] plus the
//!    first unsupported expression form. Complete-by-construction: every override
//!    delegates to `walk::walk_*`, so no subtree is dropped.
//! 2. [`RewritePlanner`] — turns the occurrences into [`CodeTransform`] edits, or a
//!    refusal. A post-pass invariant asserts no resolved signal/prop occurrence was
//!    left without a rewrite decision.
//!
//! Assignment / update targets lower through the typed [`ClientLvalue`] classifier,
//! so a TS-wrapped / private-field / computed-member / destructuring target is
//! handled STRUCTURALLY (never silently dropped, never mis-rewritten).
//!
//! It also owns the `$props()` READ-FORM vocabulary ([`PropRead`] / [`PropReads`])
//! — the rewriter consumes it for a prop read, and the declaration lowering in
//! [`super::expr_emit`] produces it (`collect_prop_reads`).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentOperator, AssignmentTarget,
    BlockStatement, CatchClause, Expression, ForInStatement, ForOfStatement, ForStatement,
    Function, IdentifierReference, Program, SimpleAssignmentTarget, Statement, UpdateExpression,
    UpdateOperator, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, SourceType};

use super::expr::{
    arrow_scope_names, block_scope_names, collect_pattern_names, expr_is_proxiable, for_left_names,
    function_scope_names, BindingRuntimeKind, BindingTable, ProxyInit, ScopeGraph, ScopeId,
};
use super::unsupported::UnsupportedSvelteRuntimeSurface;
use crate::code_transform::CodeTransform;
use rustc_hash::FxHashMap;
use verter_span::Span as VerterSpan;

/// A rewritten expression — the emitted client form of a template / handler /
/// initializer expression. A thin wrapper around the emitted JS string; the
/// CodeTransform that produced it owned the source-map authority, which is a
/// deferral-ledger follow-up surface (the emitted runtime module carries no
/// source map yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewrittenExpr {
    /// The emitted JS text.
    pub text: String,
}

/// The typed classification of an assignment / update TARGET — the structural
/// lvalue shape the rewriter consults to decide how (or whether) a write lowers.
///
/// This replaces a hand-enumerated target-kind switch: every assignment / update
/// target lowers through this classifier, so a TS-wrapped (`(x as T) = …` /
/// `x! = …`), private-field (`o.#x = …`), computed-member, or destructuring
/// (`{ x } = …` / `[x] = …`) target is handled STRUCTURALLY (never silently
/// dropped, never mis-rewritten).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientLvalue {
    /// A bare-identifier target that resolves to a reactive SIGNAL (`count = …` →
    /// `$.set(count, …)`).
    SignalIdent {
        /// The signal binding name.
        name: String,
    },
    /// A bare-identifier target that is NOT a signal (a plain local / global) —
    /// the assignment passes through unchanged.
    PlainIdent,
    /// A member target (`o.a = …` / `o[i] = …`) — a deep write the rewriter passes
    /// through (a `BareProxy` / `StateProxy` member write stays plain member
    /// access; the object base is recursed for any nested signal read).
    Member,
    /// A target shape that COULD denote a reactive write but is outside the
    /// supported lvalue subset (a TS-wrapped or otherwise non-plain identifier that
    /// resolves to a signal) — the rewriter FAILS CLOSED rather than dropping the
    /// rewrite (which would leave a raw write on the signal var).
    UnsupportedReactiveTarget,
    /// A destructuring assignment target (`{ x } = …` / `[x] = …`) — the official
    /// compiler lowers it through a destructure closure; that lowering is a
    /// deferral-ledger follow-up, so the rewriter FAILS CLOSED.
    UnsupportedTarget,
}

/// The role a rewritten expression plays. The single value-position role; the
/// enum exists so a future statement / pattern role can be added without changing
/// the call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteRole {
    /// A value-position expression (an interpolation, a handler body, a getter).
    Value,
}

/// How a `$props()` member is READ at a reference site (the official two forms).
///
/// The READ is keyed on the LOCAL binding name (`let { foo: bar }` → `bar`), but a
/// no-default prop reads off the props object by its SOURCE key (`$$props.foo`,
/// NOT `$$props.bar`) — matching the official compiler. A default-bearing prop is
/// declared `let bar = $.prop($$props, 'foo', …)` and read as the local getter
/// `bar()`, so the source key lives in the declaration, not the read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropRead {
    /// A default-bearing prop is declared via `$.prop(...)` and READ as a getter
    /// call (`<local>()`).
    Getter,
    /// A no-default prop is NOT declared; it is READ directly off the props object
    /// by its SOURCE key (`$$props.<source_key>`).
    PropsMember {
        /// The SOURCE prop key (the destructure key), which may differ from the
        /// local binding name under aliasing (`let { foo: bar }` → `foo`).
        source_key: String,
    },
}

/// The per-name `$props()` read forms a component's instance script established.
/// An empty map (no props) is the common case.
pub type PropReads = rustc_hash::FxHashMap<String, PropRead>;

/// Rewrite a template / handler EXPRESSION to its emitted client form.
///
/// Walks the OXC AST of `source` (the expression text), resolving each identifier
/// scope-awarely against `scopes` rooted at `scope` (with a LOCAL shadow stack for
/// the expression's own arrow/fn params + nested lets), and rewrites:
///
/// - a signal read (`StateSignal` / `StateProxy` / `Derived` / each / await /
///   `{@const}`) → `$.get(x)`;
/// - a `BareProxy` read → PLAIN access (`o`, `o.a`) — never `$.get`;
/// - a signal reassign `x = rhs` → `$.set(x, rhs)` (`$.set(x, rhs, true)` for a
///   `StateProxy`); a compound `x += y` → `$.set(x, $.get(x) + y)`; `x++` →
///   `$.update(x)`, `x--` → `$.update(x, -1)`;
/// - a `BareProxy` member write (`o.a++`, `o.push(...)`) → PLAIN (never `$.set`).
///
/// A shadowing local of the same name as a signal is its own binding and is left
/// untouched. An expression that does NOT parse, or that uses a form outside the
/// supported subset (a destructuring write target, an `await`, a TS-wrapped
/// reactive write target), is a REFUSAL: it returns `Err(UnsupportedSvelteRuntimeSurface)`,
/// NEVER verbatim output.
pub fn rewrite_expression(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    role: RewriteRole,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    rewrite_expression_with_props(source, scope, bindings, scopes, &PropReads::default(), role)
}

/// Like [`rewrite_expression`] but with the component's `$props()` read forms, so
/// a default-bearing prop reads as `name()` and a no-default prop as
/// `$$props.name`.
pub fn rewrite_expression_with_props(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    role: RewriteRole,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    let _ = role;
    rewrite_expression_full(
        source,
        scope,
        bindings,
        scopes,
        prop_reads,
        &ProxyInitMap::default(),
    )
}

/// The per-script one-hop proxy-init map (`identifier name → ProxyInit`), the
/// scope-aware input the `should_proxy` follow consults to decide the trailing
/// `, true` on a `$.set(o, rhs[, true])`. An empty map means "no follow data" —
/// every identifier RHS defaults to proxiable (the official predicate's
/// default-true), matching the prior behaviour for call sites that have no script
/// context.
pub type ProxyInitMap = FxHashMap<String, ProxyInit>;

/// The SOURCE DIALECT a rewrite parses + lowers in. The two surfaces differ ONLY in
/// the parser source type and whether the TypeScript-strip pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprDialect {
    /// TS-lenient (`SourceType::tsx()`) parse, followed by the TypeScript-strip pass —
    /// the default for template expressions, event handlers, lvalue thunks, and
    /// instance-script `function` declarations (a `<script>` may carry TS syntax that
    /// lowers stripped).
    Tsx,
    /// PLAIN Svelte JS (`SourceType::mjs()`) parse, with NO TypeScript-strip pass — the
    /// FUNCTION-PAIR bind element lane ONLY. Official svelte@5.56.3 parses a binding
    /// expression as plain JS (any TS construct is a parse error, refused upstream by
    /// `parse_plain_svelte_function_pair`), so a valid-JS element that LOOKS like TS to
    /// the TSX parser — e.g. ``tag<string>`x` `` (a relational compare, NOT a tagged
    /// template with type arguments) — must be parsed as plain JS and NOT TS-stripped,
    /// or the strip corrupts it (``tag`x` ``). Scoped to function-pair elements; do not
    /// route other expression callers through this dialect.
    PlainJs,
}

/// Like [`rewrite_expression_with_props`] but ALSO threading the per-script
/// one-hop proxy-init map, so a `$.set(o, prim[, true])` reassignment matches the
/// official scope-aware `should_proxy(rhs)` (a one-hop identifier follow to a
/// non-proxiable primitive initializer does NOT proxy). Call sites that operate in
/// a script scope build the map once via [`collect_proxy_inits`](super::state_scan::collect_proxy_inits)
/// and pass it.
///
/// FALLIBLE: a parse failure or an unsupported expression form (a destructuring
/// write target, an `await`, a TS-wrapped reactive write target) returns
/// `Err(UnsupportedSvelteRuntimeSurface)` — never verbatim output.
pub fn rewrite_expression_full(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    rewrite_expression_dialect(
        source,
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::Tsx,
    )
}

/// Like [`rewrite_expression_full`] but in the PLAIN-JS ([`ExprDialect::PlainJs`])
/// dialect — the FUNCTION-PAIR bind element lane. The source is parsed as
/// `SourceType::mjs()` and the TypeScript-strip pass is OMITTED, so a valid-JS element
/// that the TSX parser would reinterpret as TS (e.g. ``tag<string>`x` `` — a relational
/// compare, not a tagged template with type arguments) is rewritten faithfully instead
/// of being corrupted by the strip. The signal-rewrite collection is identical (signals
/// are plain identifiers/calls, dialect-independent). Acceptance + element extraction is
/// the caller's responsibility via the shared
/// [`BindTargetFact::function_pair`](super::expr::BindTargetFact) slices, which already
/// refused any TS-bearing element upstream; this lane only rewrites a known-clean element.
pub fn rewrite_expression_plain_js(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    rewrite_expression_dialect(
        source,
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::PlainJs,
    )
}

/// The shared source-preserving expression-rewrite core, parameterised by source
/// [`ExprDialect`]. `Tsx` parses TS-lenient + strips TypeScript; `PlainJs` parses
/// `mjs` + omits the strip. Everything else (the two-pass signal-rewrite, the
/// CodeTransform composition, the inner-expression slice) is dialect-independent.
fn rewrite_expression_dialect(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
    dialect: ExprDialect,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    // Wrap the expression in `(…)` so an arrow / object literal / sequence parses
    // as a single expression statement. The single `(` prefix means an AST span
    // `s` indexes `wrapped[s..]` directly.
    let wrapped = format!("({source})");
    let source_type = match dialect {
        ExprDialect::Tsx => SourceType::tsx(),
        ExprDialect::PlainJs => SourceType::mjs(),
    };
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        // A fragment that does not parse is a refusal — never emit it verbatim.
        return Err(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
            span: VerterSpan::new(0, 0),
        });
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return Err(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
            span: VerterSpan::new(0, 0),
        });
    };
    let inner = match &stmt.expression {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };

    // Collect the binding-bearing read/write occurrences and plan them into the typed
    // signal-rewrite edits (the shared two-pass core; a refusal returns the typed surface).
    let edits = plan_signal_edits(
        inner,
        RewriteResolveCtx {
            bindings,
            scopes,
            outer_scope: scope,
            prop_reads,
            proxy_inits,
        },
        source,
    )?;

    let ct_alloc = Allocator::default();
    let mut ct = CodeTransform::new(&wrapped, &ct_alloc);
    // (1) Strip TypeScript syntax through the dedicated machinery (the §F rule —
    // `as`/`satisfies`/`!`/type assertions/instantiation type args), the SAME path
    // the statement lowering uses. The strip and the signal-read rewrites are
    // DISJOINT (a TS type span never overlaps a runtime read leaf), so they compose
    // on one transform. SKIPPED in the `PlainJs` dialect: the element parsed as plain
    // JS carries no TS nodes, and stripping would mis-handle a valid-JS form the TSX
    // parser reinterprets as TS (the ``tag<string>`x` `` trap).
    if dialect == ExprDialect::Tsx {
        crate::strip_types::typescript::strip_typescript_from_expression(
            inner, &mut ct, 0, &wrapped,
        );
    }
    // (2) Apply the signal read/write rewrites.
    for edit in &edits {
        match edit {
            Edit::Overwrite { start, end, text } => {
                ct.overwrite(*start, *end, text);
            }
            Edit::Append { at, text } => {
                ct.append_left(*at, text);
            }
        }
    }
    let built = ct.build_string();
    // Slice the rewritten INNER expression back out of the built wrapped string.
    // The leading `(` adds one byte; the build delta is uniform up to the inner
    // start because no edit precedes it, so the inner expression begins at byte 1.
    let body = built
        .strip_prefix('(')
        .and_then(|b| b.strip_suffix(')'))
        .unwrap_or(&built);
    Ok(RewrittenExpr {
        text: body.to_string(),
    })
}

/// Collect the binding-bearing read/write occurrences of one expression node and plan them
/// into the typed signal-rewrite [`Edit`]s — the two-pass core of the source-preserving
/// [`rewrite_expression_full`]. A refusal (an unsupported write target / `await` / TS-wrapped
/// reactive target, or a resolved occurrence left un-rewritten) returns the typed surface.
/// `source` is the original expression text (for the debug assertion message only).
fn plan_signal_edits(
    inner: &Expression<'_>,
    ctx: RewriteResolveCtx<'_>,
    source: &str,
) -> Result<Vec<Edit>, UnsupportedSvelteRuntimeSurface> {
    // Pass 1: a COMPLETE scope-aware AST walk records every binding-bearing occurrence
    // (read / reassign / update) as a TYPED occurrence plus any unsupported expression form.
    // The walk delegates to `walk::walk_*` after handling each node, so NO subtree is
    // dropped. Pass 2 turns the occurrences into CodeTransform edits or a refusal.
    let mut collector = BindingOccurrenceCollector {
        ctx,
        locals: Vec::new(),
        occurrences: Vec::new(),
        refusal: None,
    };
    collector.rewrite_expr(inner);
    if let Some(surface) = collector.refusal {
        return Err(surface);
    }
    let occurrences = collector.occurrences;

    // Pass 2 (RewritePlanner): every resolved signal/prop occurrence MUST carry a rewrite
    // decision (the post-pass invariant). Build the edits from the typed occurrences; a
    // `RewriteDecision::Refuse` returns the typed surface.
    let mut planner = RewritePlanner {
        edits: Vec::new(),
        refusal: None,
        unresolved: false,
    };
    planner.plan(&occurrences);
    if let Some(surface) = planner.refusal {
        return Err(surface);
    }
    // POST-PASS ASSERTION: no resolved signal/prop occurrence may remain without a rewrite
    // decision. The planner sets `unresolved` if it ever sees a `MustRewrite` occurrence it
    // did not turn into an edit — a structural safeguard against a silent un-rewritten
    // signal read slipping through.
    debug_assert!(
        !planner.unresolved,
        "rewrite planner left a resolved signal/prop occurrence un-rewritten in `{source}`"
    );
    if planner.unresolved {
        return Err(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
            span: VerterSpan::new(0, 0),
        });
    }
    Ok(planner.edits)
}

/// One mapped/unmapped edit the rewriter records over the wrapped expression
/// source. Edits are DISJOINT by construction (a structural rewrite never also
/// carries a leaf edit inside the span it fully overwrites), so they compose
/// cleanly on the `CodeTransform`.
enum Edit {
    /// Overwrite `[start, end)` with `text` (a signal-read leaf → `$.get(x)`, a
    /// prop read → `name()`, an assignment / update head). The text is inserted
    /// (unmapped synthesized scaffolding); surrounding source stays mapped.
    Overwrite { start: u32, end: u32, text: String },
    /// Append `text` after byte `at` (the closing `)` of an assignment / update
    /// wrap, placed after a sub-expression that keeps its own leaf edits).
    Append { at: u32, text: String },
}

/// The resolution context the rewriter consults (binding table, scope graph, prop
/// read forms, proxy-init map). Bundled so the per-occurrence resolution is one
/// borrow.
#[derive(Clone, Copy)]
struct RewriteResolveCtx<'s> {
    bindings: &'s BindingTable,
    scopes: &'s ScopeGraph,
    /// The outer (template / script) scope this expression evaluates in.
    outer_scope: ScopeId,
    /// The component's `$props()` read forms (per prop name).
    prop_reads: &'s PropReads,
    /// The per-script one-hop proxy-init map for the `should_proxy(rhs)` follow.
    proxy_inits: &'s ProxyInitMap,
}

/// One binding-bearing occurrence the [`BindingOccurrenceCollector`] records — a
/// READ, a REASSIGN, or an UPDATE that resolves (scope-awarely) to a rune binding,
/// already lowered to its TYPED rewrite decision. A non-binding occurrence (a
/// global / shadowed local read) is NOT recorded at all.
enum Occurrence {
    /// A read leaf that must be rewritten (`$.get(x)` for a signal, `name()` /
    /// `$$props.x` for a prop). Carries the read span + the emitted text.
    ReadRewrite { span: oxc_span::Span, text: String },
    /// A signal reassignment (`x = rhs` → `$.set(x, …)`, `x += y` → `$.set(x,
    /// $.get(x) + y)`). Carries the head-overwrite span + text and the trailing
    /// append (`)` / `, true)`).
    SignalReassign {
        head_span: oxc_span::Span,
        head_text: String,
        append_at: u32,
        append_text: String,
    },
    /// A signal update (`x++` → `$.update(x)`). Carries the whole-update span + the
    /// emitted text.
    SignalUpdate { span: oxc_span::Span, text: String },
}

/// Pass 1: the COMPLETE scope-aware occurrence collector. It walks the OXC
/// expression and records each binding-bearing READ / REASSIGN / UPDATE as a typed
/// [`Occurrence`] (already lowered to its rewrite decision), plus the FIRST
/// unsupported expression form it hits (`await`, a destructuring write target, a
/// TS-wrapped reactive write target) as a `refusal`.
///
/// COMPLETE BY CONSTRUCTION: every override DELEGATES to `walk::walk_*` after
/// handling its node, so the traversal reaches EVERY expression AND statement node
/// — no subtree is dropped. A LOCAL shadow stack (`locals`) models the expression's
/// own nested scopes so a shadowing local of a signal name is NOT recorded.
struct BindingOccurrenceCollector<'s> {
    ctx: RewriteResolveCtx<'s>,
    /// The active LOCAL shadow frames (innermost last).
    locals: Vec<rustc_hash::FxHashSet<String>>,
    /// The recorded binding occurrences, in walk order.
    occurrences: Vec<Occurrence>,
    /// The FIRST unsupported expression form found (a refusal), if any.
    refusal: Option<UnsupportedSvelteRuntimeSurface>,
}

impl BindingOccurrenceCollector<'_> {
    /// Whether `name` is shadowed by a local frame inside the expression.
    fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|f| f.contains(name))
    }

    /// Resolve the runtime kind of `name` in the outer scope, unless it is locally
    /// shadowed (a shadowed name has no signal meaning here).
    fn signal_kind(&self, name: &str) -> Option<BindingRuntimeKind> {
        if self.is_local(name) {
            return None;
        }
        self.ctx
            .bindings
            .resolve_kind(self.ctx.scopes, self.ctx.outer_scope, name)
    }

    /// Whether `name` resolves to a reactive SIGNAL (read via `$.get`).
    fn is_signal(&self, name: &str) -> bool {
        matches!(self.signal_kind(name), Some(k) if is_signal_kind(k))
    }

    /// Whether `name` resolves to a plain runes `$state` cell — a `StateSignal`
    /// (primitive or proxy-backed) — for the `$.set(…, true)` proxy-RHS gate. A
    /// `Derived` / `Prop` / each / await / store binding is NOT a plain `$state`.
    fn is_runes_state(&self, name: &str) -> bool {
        matches!(
            self.signal_kind(name),
            Some(BindingRuntimeKind::StateSignal { raw: false } | BindingRuntimeKind::StateProxy)
        )
    }

    /// Record the first refusal (later refusals are ignored — the first surface is
    /// reported).
    fn refuse(&mut self, surface: UnsupportedSvelteRuntimeSurface) {
        if self.refusal.is_none() {
            self.refusal = Some(surface);
        }
    }

    /// Run the COMPLETE scope-aware walk over one expression node.
    fn rewrite_expr(&mut self, expr: &Expression<'_>) {
        self.visit_expression(expr);
    }

    /// Classify an assignment / update TARGET into its typed [`ClientLvalue`]. The
    /// classification is STRUCTURAL (the parsed OXC node), so a TS-wrapped
    /// (`(x as T)` / `x!`), private-field (`o.#x`), computed-member, or destructuring
    /// (`{ x }` / `[x]`) target is handled by its own arm — never silently dropped.
    fn classify_target(&self, target: &AssignmentTarget<'_>) -> ClientLvalue {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if self.is_signal(name) {
                    ClientLvalue::SignalIdent {
                        name: name.to_string(),
                    }
                } else {
                    ClientLvalue::PlainIdent
                }
            }
            // Member targets (`o.a` / `o[i]` / `o.#x`) are deep writes — plain
            // member access (a `BareProxy` / `StateProxy` member write stays plain).
            AssignmentTarget::StaticMemberExpression(_)
            | AssignmentTarget::ComputedMemberExpression(_)
            | AssignmentTarget::PrivateFieldExpression(_) => ClientLvalue::Member,
            // A TS-wrapped target (`(x as T) = …` / `x! = …` / `<T>x = …`) could
            // denote a reactive write but is outside the supported plain-identifier
            // subset — fail closed rather than drop the rewrite.
            AssignmentTarget::TSAsExpression(_)
            | AssignmentTarget::TSSatisfiesExpression(_)
            | AssignmentTarget::TSNonNullExpression(_)
            | AssignmentTarget::TSTypeAssertion(_) => ClientLvalue::UnsupportedReactiveTarget,
            // A destructuring assignment target (`{ x } = …` / `[x] = …`) — the
            // official compiler lowers it through a destructure closure; fail closed.
            AssignmentTarget::ArrayAssignmentTarget(_)
            | AssignmentTarget::ObjectAssignmentTarget(_) => ClientLvalue::UnsupportedTarget,
        }
    }

    /// Classify a SIMPLE assignment / update target (the `UpdateExpression`
    /// argument, which excludes destructuring patterns) into its typed
    /// [`ClientLvalue`].
    fn classify_simple_target(&self, target: &SimpleAssignmentTarget<'_>) -> ClientLvalue {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if self.is_signal(name) {
                    ClientLvalue::SignalIdent {
                        name: name.to_string(),
                    }
                } else {
                    ClientLvalue::PlainIdent
                }
            }
            SimpleAssignmentTarget::StaticMemberExpression(_)
            | SimpleAssignmentTarget::ComputedMemberExpression(_)
            | SimpleAssignmentTarget::PrivateFieldExpression(_) => ClientLvalue::Member,
            SimpleAssignmentTarget::TSAsExpression(_)
            | SimpleAssignmentTarget::TSSatisfiesExpression(_)
            | SimpleAssignmentTarget::TSNonNullExpression(_)
            | SimpleAssignmentTarget::TSTypeAssertion(_) => ClientLvalue::UnsupportedReactiveTarget,
        }
    }

    /// The rewritten READ text for a rune-binding identifier (a signal read →
    /// `$.get(name)`, a `$props()` member read → `name()` / `$$props.x`), or `None`
    /// for a non-signal / shadowed identifier (which stays the original source).
    fn read_rewrite_text(&self, name: &str) -> Option<String> {
        match self.signal_kind(name) {
            Some(k) if is_signal_kind(k) => Some(format!("$.get({name})")),
            Some(BindingRuntimeKind::Prop) => Some(match self.ctx.prop_reads.get(name) {
                // A default-bearing prop reads as a getter call `name()`.
                Some(PropRead::Getter) => format!("{name}()"),
                // A no-default prop reads off the props object by its SOURCE key. A
                // non-identifier-safe source key (`foo-bar`) reads via BRACKET access
                // (`$$props['foo-bar']`); an identifier-safe key reads via dotted
                // access (`$$props.foo`).
                Some(PropRead::PropsMember { source_key }) => props_member_access(source_key),
                // No recorded read form — a plain props member by name (the binding
                // name is always identifier-safe here).
                None => format!("$$props.{name}"),
            }),
            // A non-signal / shadowed identifier stays as the original source.
            _ => None,
        }
    }

    /// Record the read-leaf occurrence for a rune-binding identifier (a signal read
    /// → `$.get`, a `$props()` member read). A non-signal / shadowed identifier is
    /// NOT recorded.
    fn record_read_identifier(&mut self, id: &IdentifierReference<'_>) {
        if let Some(text) = self.read_rewrite_text(id.name.as_str()) {
            self.occurrences.push(Occurrence::ReadRewrite {
                span: id.span,
                text,
            });
        }
    }

    /// Record the read-leaf occurrence for a SHORTHAND object property whose value
    /// is a rune-binding identifier (`{ c }` with `c` a signal → `{ c: $.get(c) }`).
    ///
    /// A shorthand property's key and value share the same `c` span; rewriting the
    /// value to a bare `$.get(c)` would yield the INVALID `{ $.get(c) }` (a member
    /// expression is not a valid shorthand). Official EXPANDS the shorthand to the
    /// explicit `key: value` form, so the recorded rewrite over the value span
    /// carries the `key: ` prefix (`c: $.get(c)`). Returns `true` when it handled
    /// the property (a shorthand with a rewritten signal/prop value), so the caller
    /// skips the normal value descent (which would double-record the bare identifier).
    fn record_shorthand_property_read(&mut self, prop: &oxc_ast::ast::ObjectProperty<'_>) -> bool {
        if !prop.shorthand {
            return false;
        }
        // The shorthand value is a bare identifier reference; a shadowing local of
        // the same name is NOT rewritten (the scope-aware `read_rewrite_text` returns
        // `None` because `signal_kind` already excludes a locally-shadowed name).
        let Expression::Identifier(id) = &prop.value else {
            return false;
        };
        let key = match &prop.key {
            oxc_ast::ast::PropertyKey::StaticIdentifier(k) => k.name.as_str(),
            // A shorthand key is always a plain identifier; defensively bail for any
            // other key kind (no expansion, let the normal path handle it).
            _ => return false,
        };
        let Some(value_text) = self.read_rewrite_text(id.name.as_str()) else {
            return false;
        };
        self.occurrences.push(Occurrence::ReadRewrite {
            span: id.span,
            text: format!("{key}: {value_text}"),
        });
        true
    }

    /// Record an assignment occurrence (or a refusal), recursing the RHS.
    ///
    /// - A `SignalIdent` target: `x = rhs` → `$.set(x, rhs[, true])`; a compound
    ///   `x += y` → `$.set(x, $.get(x) + y)`. The trailing `, true` is gated on the
    ///   official `should_proxy(rhs)` for a non-coercive `=`/`||=`/`&&=`/`??=` to a
    ///   plain runes `$state`.
    /// - An `UnsupportedReactiveTarget` / `UnsupportedTarget`: a refusal (a
    ///   TS-wrapped reactive write / a destructuring write).
    /// - A `Member` / `PlainIdent`: recurse both sides (no head rewrite).
    fn record_assignment(&mut self, assign: &AssignmentExpression<'_>) {
        match self.classify_target(&assign.left) {
            ClientLvalue::SignalIdent { name } => {
                let left_start = assign.left.span().start;
                let rhs_start = assign.right.span().start;
                let rhs_end = assign.right.span().end;
                let trailing = if is_non_coercive_operator(assign.operator)
                    && self.is_runes_state(&name)
                    && self.rhs_should_proxy(&assign.right)
                {
                    ", true)"
                } else {
                    ")"
                };
                let head_text = match assign.operator {
                    AssignmentOperator::Assign => format!("$.set({name}, "),
                    op => {
                        let base = compound_base_operator(op);
                        format!("$.set({name}, $.get({name}) {base} ")
                    }
                };
                self.occurrences.push(Occurrence::SignalReassign {
                    head_span: oxc_span::Span::new(left_start, rhs_start),
                    head_text,
                    append_at: rhs_end,
                    append_text: trailing.to_string(),
                });
                // Recurse the RHS only (the head identifier is consumed above).
                self.visit_expression(&assign.right);
            }
            ClientLvalue::UnsupportedReactiveTarget | ClientLvalue::UnsupportedTarget => {
                // A TS-wrapped reactive write or a destructuring write — fail closed.
                self.refuse(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
                    span: VerterSpan::new(assign.span.start, assign.span.end),
                });
            }
            ClientLvalue::Member | ClientLvalue::PlainIdent => {
                // A member-rooted or non-signal target: recurse both sides (a
                // `BareProxy` member write stays plain; a member of a signal object
                // reads via a getter, handled by recursing into the target object).
                self.visit_assignment_target(&assign.left);
                self.visit_expression(&assign.right);
            }
        }
    }

    /// Whether the RHS of a reassignment is proxiable under the official
    /// `should_proxy` predicate, WITH the one-hop identifier follow resolved against
    /// the per-script proxy-init map (`o = primitiveVar` follows `primitiveVar` to
    /// its non-proxiable primitive init and does NOT proxy; `o = objVar` follows to
    /// a proxiable object init and DOES proxy). An empty map defaults to proxiable.
    fn rhs_should_proxy(&self, rhs: &Expression<'_>) -> bool {
        let inits = if self.ctx.proxy_inits.is_empty() {
            None
        } else {
            Some(self.ctx.proxy_inits)
        };
        expr_is_proxiable(rhs, inits)
    }

    /// Record an update occurrence (or a refusal), recursing the object base of a
    /// member target.
    ///
    /// - A `SignalIdent`: `x++` → `$.update(x)`, `x--` → `$.update(x, -1)`; `++x` →
    ///   `$.update_pre(x)`, `--x` → `$.update_pre(x, -1)` (the official prefix form).
    /// - An `UnsupportedReactiveTarget`: a refusal (a TS-wrapped update target).
    /// - A `Member` / `PlainIdent`: recurse the object base (a `BareProxy` member
    ///   update stays plain).
    fn record_update(&mut self, update: &UpdateExpression<'_>) {
        match self.classify_simple_target(&update.argument) {
            ClientLvalue::SignalIdent { name } => {
                let helper = if update.prefix {
                    "update_pre"
                } else {
                    "update"
                };
                let text = match update.operator {
                    UpdateOperator::Increment => format!("$.{helper}({name})"),
                    UpdateOperator::Decrement => format!("$.{helper}({name}, -1)"),
                };
                self.occurrences.push(Occurrence::SignalUpdate {
                    span: update.span,
                    text,
                });
            }
            ClientLvalue::UnsupportedReactiveTarget | ClientLvalue::UnsupportedTarget => {
                self.refuse(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
                    span: VerterSpan::new(update.span.start, update.span.end),
                });
            }
            ClientLvalue::Member | ClientLvalue::PlainIdent => {
                // A member target (`o.a++`) or non-signal: recurse the object base.
                match &update.argument {
                    SimpleAssignmentTarget::StaticMemberExpression(m) => {
                        self.visit_expression(&m.object);
                    }
                    SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                        self.visit_expression(&m.object);
                        self.visit_expression(&m.expression);
                    }
                    SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                        self.visit_expression(&m.object);
                    }
                    // A plain-identifier non-signal update has no sub-expression to
                    // recurse.
                    _ => {}
                }
            }
        }
    }
}

/// The EXHAUSTIVE-by-construction occurrence walk. By delegating every node it does
/// not specially handle to `walk::walk_*`, the traversal visits EVERY expression
/// AND statement node — so `switch` / `try` / `catch` / `finally` / `throw` /
/// `do-while` / `for-of` / `for-in` / labeled statements / class bodies /
/// `for`-init / default-parameter expressions are all reached structurally, with
/// no hand-enumerated kind list that can silently bail. The overrides are: (1) the
/// lexical-scope frame visitors (function / arrow / block / catch / for-family),
/// which push the SAME shadow frames the analysis-side collectors use; (2) the
/// rune-binding read / write / update visitors; (3) `visit_await_expression`, which
/// records a refusal (`await` is the 5j async surface); and (4) `visit_ts_type`, a
/// NO-OP so the walk never descends into a TYPE annotation (the `strip_typescript_*`
/// pass owns type-syntax removal on the same transform).
impl<'a> Visit<'a> for BindingOccurrenceCollector<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        // The wrapper program frame carries no shadowing names (the wrapped
        // expression has none at program scope); pushing it keeps the model uniform.
        self.locals.push(rustc_hash::FxHashSet::<String>::default());
        walk::walk_program(self, it);
        self.locals.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.locals.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.locals.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.locals.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.locals.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.locals.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.locals.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.locals.push(frame);
        walk::walk_catch_clause(self, it);
        self.locals.pop();
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
        self.locals.push(frame);
        walk::walk_for_statement(self, it);
        self.locals.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.locals.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.locals.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.locals.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.locals.pop();
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        // The head rewrite consumes the target + records the RHS recursion itself;
        // do NOT also `walk` (that would double-visit the RHS), EXCEPT when the
        // target is a Member/PlainIdent (then `record_assignment` recurses both
        // sides itself). Either way the recursion is owned by `record_assignment`.
        self.record_assignment(it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        // `record_update` consumes the target + recurses the object base of a member
        // target. Then DELEGATE to the generic walk so any OTHER sub-node reachable
        // from an UpdateExpression is still visited — the complete-by-construction
        // rule (the prior NON-delegating override was the drop bug). The signal /
        // member arms already recorded their occurrence above; the generic walk only
        // re-reaches an argument's sub-expressions (a computed-member key) the arm
        // did not, and a plain-identifier argument has no signal meaning to
        // double-record (it is not a signal).
        self.record_update(it);
        walk::walk_update_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
        // A SHORTHAND property whose value is a rune-binding identifier (`{ c }`)
        // must EXPAND to `{ c: $.get(c) }` — rewriting the shared key/value `c` span
        // to a bare `$.get(c)` would yield the INVALID `{ $.get(c) }`. When the
        // shorthand expansion is recorded, SKIP the normal value descent (it would
        // double-record the bare identifier over the same span). A non-shorthand /
        // non-signal property falls through to the generic walk unchanged.
        if self.record_shorthand_property_read(it) {
            // Still visit a computed KEY's sub-expressions (a shorthand has none, but
            // the complete-by-construction rule keeps the key reachable for safety).
            if it.computed {
                self.visit_property_key(&it.key);
            }
            return;
        }
        walk::walk_object_property(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // A READ-position reference: record a signal / prop leaf. Assignment / update
        // HEAD identifiers never reach here (their visitors consume the head before
        // recursing), so this is purely the read surface.
        self.record_read_identifier(it);
        walk::walk_identifier_reference(self, it);
    }

    fn visit_await_expression(&mut self, it: &oxc_ast::ast::AwaitExpression<'a>) {
        // An `await` expression is the experimental-async surface (5j) — fail closed
        // rather than emit a sync `$.template_effect(() => … await …)` (invalid).
        self.refuse(UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
            surface: "await",
            span: VerterSpan::new(it.span.start, it.span.end),
        });
        walk::walk_await_expression(self, it);
    }

    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {
        // A TYPE annotation carries no runtime read — the `strip_typescript_*` pass
        // removes the type syntax on the same `CodeTransform`. Do NOT descend (a
        // signal-named identifier in type position must never be rewritten, and a
        // double edit would conflict with the strip).
    }
}

/// Pass 2: the rewrite PLANNER. It consumes the typed occurrences pass 1 recorded
/// and emits the CodeTransform edits, OR records a refusal. A `MustRewrite`
/// occurrence that the planner cannot turn into an edit sets `unresolved` — the
/// post-pass invariant (no resolved signal/prop occurrence left un-rewritten).
struct RewritePlanner {
    /// The emitted edits (disjoint, applied in record order).
    edits: Vec<Edit>,
    /// The first refusal, if any.
    refusal: Option<UnsupportedSvelteRuntimeSurface>,
    /// Set when an occurrence that MUST rewrite was left without an edit (the
    /// post-pass safeguard).
    unresolved: bool,
}

impl RewritePlanner {
    /// Turn each occurrence into its edits. Every occurrence variant carries a
    /// concrete rewrite decision, so the planner always emits the edits (the
    /// `unresolved` flag stays false on the supported path) — it exists as the
    /// structural seam the post-pass invariant asserts against.
    fn plan(&mut self, occurrences: &[Occurrence]) {
        for occ in occurrences {
            match occ {
                Occurrence::ReadRewrite { span, text } => {
                    self.edits.push(Edit::Overwrite {
                        start: span.start,
                        end: span.end,
                        text: text.clone(),
                    });
                }
                Occurrence::SignalReassign {
                    head_span,
                    head_text,
                    append_at,
                    append_text,
                } => {
                    self.edits.push(Edit::Overwrite {
                        start: head_span.start,
                        end: head_span.end,
                        text: head_text.clone(),
                    });
                    self.edits.push(Edit::Append {
                        at: *append_at,
                        text: append_text.clone(),
                    });
                }
                Occurrence::SignalUpdate { span, text } => {
                    self.edits.push(Edit::Overwrite {
                        start: span.start,
                        end: span.end,
                        text: text.clone(),
                    });
                }
            }
        }
    }
}

/// The base operator of a compound assignment (`+=` → `+`, `*=` → `*`, …).
fn compound_base_operator(op: AssignmentOperator) -> &'static str {
    match op {
        AssignmentOperator::Addition => "+",
        AssignmentOperator::Subtraction => "-",
        AssignmentOperator::Multiplication => "*",
        AssignmentOperator::Division => "/",
        AssignmentOperator::Remainder => "%",
        AssignmentOperator::Exponential => "**",
        AssignmentOperator::ShiftLeft => "<<",
        AssignmentOperator::ShiftRight => ">>",
        AssignmentOperator::ShiftRightZeroFill => ">>>",
        AssignmentOperator::BitwiseOR => "|",
        AssignmentOperator::BitwiseXOR => "^",
        AssignmentOperator::BitwiseAnd => "&",
        AssignmentOperator::LogicalOr => "||",
        AssignmentOperator::LogicalAnd => "&&",
        AssignmentOperator::LogicalNullish => "??",
        AssignmentOperator::Assign => "=",
    }
}

/// Whether an assignment operator is NON-COERCIVE — the official
/// `is_non_coercive_operator` set (`=`, `||=`, `&&=`, `??=`). Only these gate the
/// proxy `, true` on a reassignment; a coercive compound (`+=`, `*=`, `<<=`, …)
/// always evaluates to a primitive and never proxies.
fn is_non_coercive_operator(op: AssignmentOperator) -> bool {
    matches!(
        op,
        AssignmentOperator::Assign
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalNullish
    )
}

/// Build the `$$props` member access for a no-default prop's SOURCE key. An
/// identifier-safe key reads via DOTTED access (`$$props.foo`); a key that is not
/// a valid JS identifier (`foo-bar`, a numeric key, a key with quotes) reads via
/// BRACKET access with a properly-escaped JS string literal (`$$props['foo-bar']`).
fn props_member_access(source_key: &str) -> String {
    if is_js_identifier(source_key) {
        format!("$$props.{source_key}")
    } else {
        format!("$$props[{}]", js_string_literal(source_key))
    }
}

/// Whether `name` is a valid plain JS identifier (so a `$$props.<name>` dotted
/// access is valid). A `$state`-style `$`/`_`-prefixed name qualifies; a `foo-bar`
/// / numeric-leading / empty name does not (it requires bracket access).
fn is_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render `value` as a single-quoted JS string literal, escaping backslash, the
/// single-quote delimiter, and the line terminators — so an arbitrary destructure
/// key (`foo-bar`, `it's`, a key with a newline) interpolates into emitted JS
/// SAFELY (no broken `'<key>'`).
pub(super) fn js_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Whether a binding kind is a reactive SIGNAL (read via `$.get`).
pub(super) fn is_signal_kind(kind: BindingRuntimeKind) -> bool {
    matches!(
        kind,
        BindingRuntimeKind::StateSignal { .. }
            | BindingRuntimeKind::StateProxy
            | BindingRuntimeKind::Derived
            | BindingRuntimeKind::EachSignal
            | BindingRuntimeKind::AwaitSignal
            | BindingRuntimeKind::LegacyConstDerived
    )
}
