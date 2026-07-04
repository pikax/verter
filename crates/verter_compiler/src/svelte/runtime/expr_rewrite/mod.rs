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
//! 1. [`BindingOccurrenceCollector`](plan::BindingOccurrenceCollector) — a COMPLETE scope-aware walk recording every
//!    binding-bearing read / reassign / update as a TYPED [`Occurrence`](plan::Occurrence) plus the
//!    first unsupported expression form. Complete-by-construction: every override
//!    delegates to `walk::walk_*`, so no subtree is dropped.
//! 2. [`RewritePlanner`](plan_planner::RewritePlanner) — turns the occurrences into [`CodeTransform`] edits, or a
//!    refusal. A post-pass invariant asserts no resolved signal/prop occurrence was
//!    left without a rewrite decision.
//!
//! Assignment / update targets lower through the typed [`ClientLvalue`] classifier,
//! so a TS-wrapped / private-field / computed-member / destructuring target is
//! handled STRUCTURALLY (never silently dropped, never mis-rewritten).
//!
//! It also owns the `$props()` READ-FORM vocabulary ([`PropRead`] / [`PropReads`])
//! — the rewriter consumes it for a prop read, and the unified
//! [`PropsDeclaratorPlan`](super::expr_emit::PropsDeclaratorPlan) in
//! [`super::expr_emit`] produces it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Statement};
use oxc_span::SourceType;

use self::plan::{plan_signal_edits, Edit, RewriteResolveCtx};
use super::expr::{BindingTable, ProxyInit, ScopeGraph, ScopeId};
use super::unsupported::UnsupportedSvelteRuntimeSurface;
use crate::code_transform::CodeTransform;
use rustc_hash::FxHashMap;
use verter_span::Span as VerterSpan;

mod plan;
mod plan_planner;
mod plan_render;

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
    /// A bare-identifier target that resolves to a `$props()` PROP SOURCE (a
    /// default-bearing or written prop, declared `let name = $.prop(...)`) — the
    /// write lowers through the getter/setter function (`name = v` → `name(v)`;
    /// `name += v` → `name(name() + v)`; `name++` → `$.update_prop(name)`).
    PropSetter {
        /// The prop-source local name (the `$.prop` getter/setter binding).
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

/// The role a rewritten expression plays — the POSITION the caller embeds the
/// rewritten text in. The user-effect runes are position-sensitive (official
/// `effect_invalid_placement`: `$effect(...)` / `$effect.pre(...)` are legal
/// ONLY as the expression of an `ExpressionStatement`), so the role decides
/// whether a top-level user-effect call is admissible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteRole {
    /// A value-position expression (an interpolation, a handler body, a getter,
    /// a declarator init). A top-level `$effect(...)` / `$effect.pre(...)` call
    /// REFUSES here (official rejects every value position).
    Value,
    /// The expression OF a statement (the instance-script effect-statement
    /// carrier): a top-level `$effect(...)` / `$effect.pre(...)` call is in its
    /// one official-legal position and lowers.
    Statement,
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
    /// A `$props()` REST / WHOLE-OBJECT capture binding (`let { …, ...rest } =
    /// $props()` / `let all = $props()`), declared `let <local> =
    /// $.rest_props($$props, rest_excludes)`. A BARE read stays the verbatim real
    /// local (`rest` / `all`); a MEMBER read is KEY-AWARE (owned by the member
    /// visit, not the identifier leaf): `<local>.KEY` de-localizes to `$$props.KEY`
    /// when KEY is NOT in `excludes`, else stays `<local>.KEY`.
    RestBinding {
        /// The exclude-key membership set (the fixed prefix + each non-rest source
        /// key) — the SHARED set the unified [`super::expr_emit::PropsDeclaratorPlan`]
        /// owns, carried as a cheap `Arc` clone so the hot member-visit exclude
        /// lookup is O(1) and never clones the key `Vec`. (The ORDERED exclude Vec
        /// for the emitted `new Set([…])` lives on the declarator plan / hoist, not
        /// this read form.)
        excludes: std::sync::Arc<rustc_hash::FxHashSet<String>>,
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
    rewrite_expression_dialect(
        source,
        "",
        scope,
        bindings,
        scopes,
        prop_reads,
        &ProxyInitMap::default(),
        ExprDialect::Tsx,
        role,
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
        "",
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::Tsx,
        RewriteRole::Value,
    )
}

/// Like [`rewrite_expression_full`] but in the STATEMENT role
/// ([`RewriteRole::Statement`]): `source` is the EXPRESSION OF a statement (the
/// instance-script effect-statement carrier), so a top-level `$effect(...)` /
/// `$effect.pre(...)` call is in its one official-legal position
/// (`effect_invalid_placement` is a DIRECT-parent rule) and lowers to
/// `$.user_effect` / `$.user_pre_effect`. `carrier_head_trivia` is the
/// carrier's pre-rendered transparent-wrapper head trivia (the wrapper parens
/// normalized away by the carrier slice) — re-emitted INSIDE the emitted
/// helper call, immediately after the opening paren, ahead of the head's own
/// trivia. Everything else — nested statement admission, the value-position
/// refusals, the signal rewrites — is identical to the value role.
pub fn rewrite_statement_expression_full(
    source: &str,
    carrier_head_trivia: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    rewrite_expression_dialect(
        source,
        carrier_head_trivia,
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::Tsx,
        RewriteRole::Statement,
    )
}

/// Like [`rewrite_expression_full`] (the VALUE role) but for the
/// instance-script effect-rune-init carrier: `source` is a carried
/// `$effect.root(fn)` / `$effect.tracking()` declarator-init payload whose
/// transparent wrapper parens the carrier slice normalized away, and
/// `carrier_head_trivia` is that peeled wrapper head's pre-rendered comment
/// trivia — re-emitted INSIDE the emitted helper call, immediately after the
/// opening paren (the canonical call-internal slot), ahead of the head's own
/// trivia. Everything else is identical to the value role.
pub fn rewrite_init_expression_full(
    source: &str,
    carrier_head_trivia: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
) -> Result<RewrittenExpr, UnsupportedSvelteRuntimeSurface> {
    rewrite_expression_dialect(
        source,
        carrier_head_trivia,
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::Tsx,
        RewriteRole::Value,
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
        "",
        scope,
        bindings,
        scopes,
        prop_reads,
        proxy_inits,
        ExprDialect::PlainJs,
        RewriteRole::Value,
    )
}

/// The shared source-preserving expression-rewrite core, parameterised by source
/// [`ExprDialect`] and [`RewriteRole`]. `Tsx` parses TS-lenient + strips
/// TypeScript; `PlainJs` parses `mjs` + omits the strip. The role decides
/// whether the TOP-LEVEL expression counts as a statement position (the
/// user-effect statement carrier) or a value position. `carrier_head_trivia`
/// is the instance-item carriers' pre-rendered transparent-wrapper head trivia
/// (empty on every other lane) — re-emitted inside the carried expression's
/// top-level family call head. Everything else (the two-pass signal-rewrite,
/// the CodeTransform composition, the inner-expression slice) is dialect- and
/// role-independent.
#[allow(clippy::too_many_arguments)]
fn rewrite_expression_dialect(
    source: &str,
    carrier_head_trivia: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    prop_reads: &PropReads,
    proxy_inits: &ProxyInitMap,
    dialect: ExprDialect,
    role: RewriteRole,
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
    // The wrapped source + the parse's comment table + the carrier's wrapper-head
    // trivia ride along so the invocation-head rewrites can preserve comment
    // trivia inside a replaced (or carrier-normalized) range.
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
        role,
        &wrapped,
        &parsed.program.comments,
        carrier_head_trivia,
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
    // (2) Apply the signal read/write rewrites + the elision drops.
    for edit in &edits {
        match edit {
            Edit::Overwrite { start, end, text } => {
                ct.overwrite(*start, *end, text);
            }
            Edit::Insert { at, text } => {
                // A pure insertion BEFORE `at` (the bindable mutation-wrap head):
                // `prepend_left` lands it before any overwrite starting at the
                // same byte (an empty-span `overwrite` would be a silent no-op).
                ct.prepend_left(*at, text);
            }
            Edit::Append { at, text } => {
                ct.append_left(*at, text);
            }
            Edit::Remove { start, end } => {
                ct.remove(*start, *end);
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
