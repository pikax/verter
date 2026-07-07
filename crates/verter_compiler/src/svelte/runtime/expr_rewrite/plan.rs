//! The TWO-PASS core of the fallible Svelte client expression rewriter
//! ([`crate::svelte::runtime::expr_rewrite`]).
//!
//! Pass 1 ([`BindingOccurrenceCollector`]) walks the parsed expression and
//! records every binding-bearing read / reassign / update as a typed
//! [`Occurrence`]; pass 2 ([`RewritePlanner`]) turns the occurrences into the
//! typed [`Edit`]s the caller applies to its `CodeTransform`.
//! [`plan_signal_edits`] is the single entry point. The leaf classification /
//! rendering helpers the passes consume (the `$state.snapshot` callee matcher,
//! the assignment-operator tables, the `$$props` member-access rendering) live
//! here as well.

use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentOperator, AssignmentTarget,
    BlockStatement, CallExpression, CatchClause, Comment, Expression, ForInStatement,
    ForOfStatement, ForStatement, Function, IdentifierReference, ParenthesizedExpression, Program,
    SimpleAssignmentTarget, Statement, UpdateExpression, UpdateOperator, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;

use super::super::expr::{
    arrow_scope_names, block_scope_names, call_internal_comment_trivia, collect_pattern_names,
    effect_family_call_fact, effect_family_expression_fact, expr_is_proxiable, for_left_names,
    function_scope_names, is_inspect_trace_call, is_signal_kind, peel_parens,
    statement_position_user_effect_span, BindingRuntimeKind, BindingTable, EffectFamilyCallKind,
    ScopeGraph, ScopeId,
};
use super::super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
use super::super::unsupported::UnsupportedSvelteRuntimeSurface;
use super::plan_planner::RewritePlanner;
use super::plan_render::{
    bare_member_rhs_verbatim_span, compound_base_operator, is_non_coercive_operator,
    props_member_access, state_snapshot_callee_span,
};
use super::{ClientLvalue, PropRead, PropReads, ProxyInitMap, RewriteRole};
use rustc_hash::FxHashMap;
use verter_span::Span as VerterSpan;

/// Collect the binding-bearing read/write occurrences of one expression node and plan them
/// into the typed signal-rewrite [`Edit`]s — the two-pass core of the source-preserving
/// [`rewrite_expression_full`](super::rewrite_expression_full). A refusal (an unsupported write target / `await` / TS-wrapped
/// reactive target, or a resolved occurrence left un-rewritten) returns the typed surface.
/// `source` is the original expression text (for the debug assertion message only);
/// `wrapped` is the parsed `({source})` text the AST spans index, and `comments` is
/// that parse's comment table (the invocation-head rewrites preserve comment trivia
/// inside a replaced range instead of swallowing it). `carrier_head_trivia` is the
/// pre-rendered wrapper-head trivia an instance-item carrier collected — re-emitted
/// inside the carried expression's TOP-LEVEL family call head (call-internal slot).
pub(super) fn plan_signal_edits(
    inner: &Expression<'_>,
    ctx: RewriteResolveCtx<'_>,
    source: &str,
    role: RewriteRole,
    wrapped: &str,
    comments: &[Comment],
    carrier_head_trivia: &str,
) -> Result<Vec<Edit>, UnsupportedSvelteRuntimeSurface> {
    // The carrier trivia targets the TOP-LEVEL family call of the carried
    // expression (each instance-item carrier slices exactly ONE family call —
    // classifier-proven — so its head rewrite is where the wrapper trivia
    // belongs). Resolved from the same parse the collector walks, so the
    // injection is span-exact; empty carrier trivia resolves no target.
    let carrier_trivia_target = if carrier_head_trivia.is_empty() {
        None
    } else {
        effect_family_expression_fact(inner).map(|fact| (fact.call_span.start, fact.call_span.end))
    };
    // Pass 1: a COMPLETE scope-aware AST walk records every binding-bearing occurrence
    // (read / reassign / update) as a TYPED occurrence plus any unsupported expression form.
    // The walk delegates to `walk::walk_*` after handling each node, so NO subtree is
    // dropped. Pass 2 turns the occurrences into CodeTransform edits or a refusal.
    let mut collector = BindingOccurrenceCollector {
        ctx,
        locals: Vec::new(),
        occurrences: Vec::new(),
        refusal: None,
        stmt_effect_spans: rustc_hash::FxHashSet::default(),
        wrapped,
        comments,
        carrier_head_trivia,
        carrier_trivia_target,
        wrapper_heads: FxHashMap::default(),
        member_write_target_spans: rustc_hash::FxHashSet::default(),
        member_assign_rhs_verbatim_spans: rustc_hash::FxHashSet::default(),
    };
    // In the STATEMENT role the top-level expression IS the expression of a
    // statement (the effect-statement carrier), so a top-level user-effect call
    // is pre-admitted exactly as a nested statement-position call would be by
    // `visit_expression_statement` (paren-transparent — official ESTree has no
    // paren nodes, so `($effect(fn));` is the same statement).
    if role == RewriteRole::Statement {
        if let Some(span) = statement_position_user_effect_span(inner) {
            collector.stmt_effect_spans.insert((span.start, span.end));
        }
    }
    collector.rewrite_expr(inner);
    if let Some(surface) = collector.refusal {
        return Err(surface);
    }
    let occurrences = collector.occurrences;

    // Pass 2 (RewritePlanner): every resolved signal/prop occurrence MUST carry a rewrite
    // decision (the post-pass invariant). Build the edits from the typed occurrences; a
    // `RewriteDecision::Refuse` returns the typed surface.
    let mut planner = RewritePlanner::new();
    planner.plan(&occurrences);
    if let Some(surface) = planner.take_refusal() {
        return Err(surface);
    }
    // POST-PASS ASSERTION: no resolved signal/prop occurrence may remain without a rewrite
    // decision. The planner sets `unresolved` if it ever sees a `MustRewrite` occurrence it
    // did not turn into an edit — a structural safeguard against a silent un-rewritten
    // signal read slipping through.
    debug_assert!(
        !planner.unresolved(),
        "rewrite planner left a resolved signal/prop occurrence un-rewritten in `{source}`"
    );
    if planner.unresolved() {
        return Err(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
            span: VerterSpan::new(0, 0),
        });
    }
    Ok(planner.into_edits())
}

/// One mapped/unmapped edit the rewriter records over the wrapped expression
/// source. Edits are DISJOINT by construction (a structural rewrite never also
/// carries a leaf edit inside the span it fully overwrites), so they compose
/// cleanly on the `CodeTransform`.
pub(super) enum Edit {
    /// Overwrite `[start, end)` with `text` (a signal-read leaf → `$.get(x)`, a
    /// prop read → `name()`, an assignment / update head). The text is inserted
    /// (unmapped synthesized scaffolding); surrounding source stays mapped.
    Overwrite { start: u32, end: u32, text: String },
    /// Insert `text` BEFORE byte `at` (the bindable mutation-wrap head
    /// `name(`, placed before a sub-expression that keeps its own leaf edits —
    /// an empty-span `Overwrite` would be a silent no-op).
    Insert { at: u32, text: String },
    /// Append `text` after byte `at` (the closing `)` of an assignment / update
    /// wrap, placed after a sub-expression that keeps its own leaf edits).
    Append { at: u32, text: String },
    /// Remove `[start, end)` entirely (a production-elided `$inspect.trace(...)`
    /// statement dropped in place, or a wrapper-gap comment relocated into a
    /// rewritten invocation head). The removed span carries no inner edits (the
    /// walk never descends into a dropped statement; a comment holds no AST
    /// node), so the removal composes disjointly with the leaf rewrites.
    Remove { start: u32, end: u32 },
}

/// The resolution context the rewriter consults (binding table, scope graph, prop
/// read forms, proxy-init map). Bundled so the per-occurrence resolution is one
/// borrow.
#[derive(Clone, Copy)]
pub(super) struct RewriteResolveCtx<'s> {
    pub(super) bindings: &'s BindingTable,
    pub(super) scopes: &'s ScopeGraph,
    /// The outer (template / script) scope this expression evaluates in.
    pub(super) outer_scope: ScopeId,
    /// The component's `$props()` read forms (per prop name).
    pub(super) prop_reads: &'s PropReads,
    /// The per-script one-hop proxy-init map for the `should_proxy(rhs)` follow.
    pub(super) proxy_inits: &'s ProxyInitMap,
}

/// One binding-bearing occurrence the [`BindingOccurrenceCollector`] records — a
/// READ, a REASSIGN, or an UPDATE that resolves (scope-awarely) to a rune binding,
/// already lowered to its TYPED rewrite decision. A non-binding occurrence (a
/// global / shadowed local read) is NOT recorded at all.
pub(super) enum Occurrence {
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
    /// A BINDABLE-prop member MUTATION wrapped in the setter with the mutation
    /// flag (`v.a++` → `v(v().a++, true)` — official interop with legacy parent
    /// bindings). Carries the pure-INSERT head position + text (`v(`) and the
    /// trailing append (`, true)`); the wrapped mutation keeps its own leaf
    /// edits (the base identifier read-rewrites to the getter).
    WrapCall {
        insert_at: u32,
        head_text: String,
        append_at: u32,
        append_text: String,
    },
    /// A production-ELIDED statement dropped in place — an unshadowed
    /// `$inspect.trace(...)` expression STATEMENT inside a lowered function /
    /// arrow body (official `dev:false` removes the call; the surrounding body is
    /// preserved). Carries the whole-statement span; the walk never descends into
    /// it, so the span holds no other occurrence.
    DropStatement { span: oxc_span::Span },
    /// A wrapper-GAP comment REMOVED from its source slot: the comment sits
    /// between a transparent author-paren `(` and the effect-family call the
    /// paren wraps (`(/*#__PURE__*/ $effect.root(fn))`), and the accepted
    /// invocation-head rewrite re-emits it INSIDE the emitted helper call —
    /// left in the gap it would sit call-leading before the rewritten helper
    /// (PURE-activating). The wrapper parens themselves STAY (a
    /// behavior-preserving redundant paren is waived); only the comment
    /// moves. Carries the comment span; the re-emission rides the head
    /// rewrite's replacement text.
    RelocatedWrapperComment { span: oxc_span::Span },
}

/// Pass 1: the COMPLETE scope-aware occurrence collector. It walks the OXC
/// expression and records each binding-bearing READ / REASSIGN / UPDATE as a typed
/// [`Occurrence`] (already lowered to its rewrite decision), plus the FIRST
/// unsupported expression form it hits (`await`, a destructuring write target, a
/// TS-wrapped reactive write target, a non-statement-position `$inspect`
/// reference) as a `refusal`. A statement-position `$inspect.trace(...)` inside a
/// lowered function / arrow body records a [`Occurrence::DropStatement`] instead
/// (the production elision).
///
/// COMPLETE BY CONSTRUCTION: every override DELEGATES to `walk::walk_*` after
/// handling its node, so the traversal reaches EVERY expression AND statement node
/// — no subtree is dropped (the ONE deliberate exception: a DROPPED
/// `$inspect.trace()` statement, whose whole span is removed so its subtree must
/// record nothing). A LOCAL shadow stack (`locals`) models the expression's own
/// nested scopes so a shadowing local of a signal name is NOT recorded.
pub(super) struct BindingOccurrenceCollector<'s> {
    pub(super) ctx: RewriteResolveCtx<'s>,
    /// The active LOCAL shadow frames (innermost last).
    locals: Vec<rustc_hash::FxHashSet<String>>,
    /// The recorded binding occurrences, in walk order.
    occurrences: Vec<Occurrence>,
    /// The FIRST unsupported expression form found (a refusal), if any.
    refusal: Option<UnsupportedSvelteRuntimeSurface>,
    /// The call spans of STATEMENT-POSITION `$effect(...)` / `$effect.pre(...)`
    /// calls — recorded by `visit_expression_statement` (and the statement-role
    /// entry seed) BEFORE the call visitor reaches the call. The user-effect
    /// members are statement-ONLY (official `effect_invalid_placement`); a
    /// family call whose span is NOT in this set is a value position and
    /// refuses.
    stmt_effect_spans: rustc_hash::FxHashSet<(u32, u32)>,
    /// The parsed `({source})` text the AST spans index — the slice source for
    /// re-emitting comment trivia a head rewrite would otherwise overwrite.
    wrapped: &'s str,
    /// The parse's comment table (spans index `wrapped`, delimiters included) —
    /// the trivia authority for the invocation-head rewrites.
    comments: &'s [Comment],
    /// The pre-rendered CALL-INTERNAL trivia an instance-item carrier collected
    /// from the transparent-wrapper head its slice normalized away
    /// (`(/*#__PURE__*/ $effect(fn));` — the wrapper parens stay outside the
    /// carried call span, but their interior head comments ride the carrier).
    /// Re-emitted inside the TOP-LEVEL family call's rewritten head, ahead of
    /// the head's own trivia (source order). Empty for every non-carrier entry.
    carrier_head_trivia: &'s str,
    /// The call span `(start, end)` the carrier trivia belongs to — the carried
    /// expression's top-level family call, resolved from the same parse the
    /// collector walks (span-exact injection). `None` when there is no carrier
    /// trivia.
    carrier_trivia_target: Option<(u32, u32)>,
    /// Per effect-family call span, the start byte of the OUTERMOST transparent
    /// author-paren wrapper around that call — recorded by
    /// `visit_parenthesized_expression` (the walk reaches an ancestor paren
    /// before the call it wraps, so the first insert is the outermost). The
    /// accepted head rewrite relocates every wrapper-GAP comment in
    /// `[wrapper_start, call_start)` into the emitted helper call (removal +
    /// call-internal re-emission); the parens themselves stay in place. A call
    /// with no wrapper has no entry (the overwhelmingly common case).
    wrapper_heads: FxHashMap<(u32, u32), u32>,
    /// The spans of the static-member expressions that are the DIRECT lvalue of
    /// an assignment (`rest.x = 1`) or update (`rest.x++`) — the WRITE-LIKE
    /// targets the `$props()` rest / whole-object member disposition keeps
    /// VERBATIM. De-localization (`rest.KEY` → `$$props.KEY`) is READ-only, so a
    /// direct member-write target stays local (oracle parity: the write mutates
    /// the rest proxy, NOT the raw `$$props` bag). Recorded top-down (by
    /// `record_assignment` / `record_update`) BEFORE the walk re-reaches the
    /// member. A `delete rest.x` argument and a `for (rest.x of …)` left are
    /// reference READS that DO de-localize (oracle-confirmed), so they are NOT
    /// recorded here.
    member_write_target_spans: rustc_hash::FxHashSet<(u32, u32)>,
    /// The spans of the static-member READS that are the ENTIRE right-hand side of
    /// a PLAIN `=` (`sink = rest.y`, paren-transparent) — kept VERBATIM alongside
    /// the write targets. Official svelte@5.56.3 (`Identifier.js`) de-localizes
    /// `rest.KEY` in every read position EXCEPT when its grand-parent is an
    /// Assignment/Update (a COARSE position guard, NOT a read/write split); `=`
    /// returns `right` unchanged, so a bare `rest.KEY` RHS stays local while `+=` /
    /// `??=` / `rest.y.z` de-localize. LHS-agnostic; predicate in `record_assignment`.
    member_assign_rhs_verbatim_spans: rustc_hash::FxHashSet<(u32, u32)>,
}

/// The end byte (EXCLUSIVE) of the opening `(` of an effect-family invocation
/// head — the first `(` token after the callee end, with comment trivia masked
/// through the parse's comment table (a `(` inside a comment is not the paren
/// token). The scan stops at `to` (the first argument start, or the call end
/// for a zero-arg form). EVERY accepted head — plain, member,
/// optional-receiver, optional-call — is rewritten THROUGH this byte, so
/// everything after it (arg-leading trivia, a zero-arg `()` interior, the
/// closing paren) survives in place, untouched and never duplicated. `None`
/// only when the range holds no paren token — unreachable for a parsed call
/// (the caller fails the family closed rather than emit a half-rewritten head).
fn head_open_paren_end(wrapped: &str, comments: &[Comment], from: u32, to: u32) -> Option<u32> {
    let bytes = wrapped.as_bytes();
    let mut i = from as usize;
    let end = (to as usize).min(bytes.len());
    while i < end {
        if let Some(c) = comments.iter().find(|c| c.span.start as usize == i) {
            i = (c.span.end as usize).max(i + 1);
            continue;
        }
        if bytes[i] == b'(' {
            return Some(i as u32 + 1);
        }
        i += 1;
    }
    None
}

impl BindingOccurrenceCollector<'_> {
    /// Whether `name` is shadowed by a local frame inside the expression.
    pub(super) fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|f| f.contains(name))
    }

    /// Resolve the runtime kind of `name` in the outer scope, unless it is locally
    /// shadowed (a shadowed name has no signal meaning here).
    pub(super) fn signal_kind(&self, name: &str) -> Option<BindingRuntimeKind> {
        if self.is_local(name) {
            return None;
        }
        self.ctx
            .bindings
            .resolve_kind(self.ctx.scopes, self.ctx.outer_scope, name)
    }

    /// Whether `name` resolves to a reactive SIGNAL (read via `$.get`).
    pub(super) fn is_signal(&self, name: &str) -> bool {
        matches!(self.signal_kind(name), Some(k) if is_signal_kind(k))
    }

    /// The exclude-key membership set when `name` (UNSHADOWED) resolves to the
    /// `$props()` rest / whole-object capture binding, else `None`. A local
    /// shadowing the rest name is its own binding (`None`), so its member reads
    /// are NOT key-rewritten. The set is the shared `Arc` the unified declarator
    /// plan owns — the O(1) hot-path exclude lookup.
    fn rest_binding_excludes(&self, name: &str) -> Option<&rustc_hash::FxHashSet<String>> {
        if self.is_local(name) {
            return None;
        }
        match self.ctx.prop_reads.get(name) {
            Some(PropRead::RestBinding { excludes }) => Some(excludes),
            _ => None,
        }
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

    /// The BINDABLE-prop-source ROOT name of a MEMBER write target (`v.a++` /
    /// `v.a.b = 1` — the leftmost identifier of the member chain, resolving to a
    /// `BindableProp` with a getter read), or `None`. A bindable member MUTATION
    /// wraps in the setter with the mutation flag (`v(v().a++, true)` — official
    /// interop with legacy parent bindings); a PLAIN-prop member mutation stays a
    /// raw member write over the rewritten getter base.
    // TODO(follow-up): this walk has no TS-wrapper arms (`TSNonNullExpression` /
    // `TSAsExpression` / …), so a TS-wrapped chain (`v!.a++`) would miss the
    // bindable wrap. Unreachable for that shape today: `member_write_lvalue`
    // fails every TS-wrapped prop-rooted write chain closed before the wrap
    // walk runs. When the `lang="ts"` lowering lands, peel the TS wrappers
    // here (mirroring `target_expr_root_ident`) so the wrap matches the
    // official TS-component emit. Owned by the script-completion block (5t).
    fn member_root_bindable_prop_source(&self, mut object: &Expression<'_>) -> Option<String> {
        loop {
            match object {
                Expression::StaticMemberExpression(m) => object = &m.object,
                Expression::ComputedMemberExpression(m) => object = &m.object,
                Expression::PrivateFieldExpression(m) => object = &m.object,
                Expression::ParenthesizedExpression(p) => object = &p.expression,
                Expression::Identifier(id) => {
                    let name = id.name.as_str();
                    if matches!(
                        self.signal_kind(name),
                        Some(BindingRuntimeKind::BindableProp)
                    ) && matches!(self.ctx.prop_reads.get(name), Some(PropRead::Getter))
                    {
                        return Some(name.to_string());
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    /// The `$.mutable_source` ROOT name of a MEMBER write target (`o.x = …` /
    /// `o.x++` — the leftmost identifier of the member chain, resolving to the
    /// promoted legacy [`BindingRuntimeKind::MutableSource`] kind), or `None`.
    /// A member write rooted at a mutable source wraps the WHOLE assignment /
    /// update in the official legacy deep-mutation helper
    /// (`$.mutate(o, $.get(o).x = v)` / `$.mutate(o, $.get(o).x++)`); the inner
    /// root read still rewrites through the normal signal-read leaf edit.
    fn member_root_mutable_source(&self, mut object: &Expression<'_>) -> Option<String> {
        loop {
            match object {
                Expression::StaticMemberExpression(m) => object = &m.object,
                Expression::ComputedMemberExpression(m) => object = &m.object,
                Expression::PrivateFieldExpression(m) => object = &m.object,
                Expression::ParenthesizedExpression(p) => object = &p.expression,
                Expression::Identifier(id) => {
                    let name = id.name.as_str();
                    if matches!(
                        self.signal_kind(name),
                        Some(BindingRuntimeKind::MutableSource)
                    ) {
                        return Some(name.to_string());
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    /// Record the first refusal (later refusals are ignored — the first surface is
    /// reported).
    fn refuse(&mut self, surface: UnsupportedSvelteRuntimeSurface) {
        if self.refusal.is_none() {
            self.refusal = Some(surface);
        }
    }

    /// Record a static-member WRITE target's span (`rest.x` in `rest.x = 1` /
    /// `rest.x++`) so the `$props()` rest / whole-object member disposition keeps
    /// it VERBATIM — de-localization is READ-only. Called top-down (before the
    /// walk re-reaches the member), so the span is set when
    /// `visit_static_member_expression` fires for the target. Only the DIRECT
    /// static-member lvalue matters; a computed / private-field target never hits
    /// the static disposition, and a `delete` argument / for-in-of left is a
    /// reference read that de-localizes (never recorded).
    fn note_member_write_target(&mut self, object_member_span: oxc_span::Span) {
        self.member_write_target_spans
            .insert((object_member_span.start, object_member_span.end));
    }

    /// Record a static-member READ that is the ENTIRE RHS of a plain `=`
    /// (`sink = rest.y`) so the rest / whole-object disposition keeps it VERBATIM
    /// (oracle parity: the coarse Assignment-child guard keeps a bare `rest.KEY`
    /// RHS local). The consumption site's rest-binding / exclude / `$$` gates decide
    /// whether a recorded span is a rest member, so a non-rest span here is inert.
    fn note_member_assign_rhs_verbatim(&mut self, member_span: oxc_span::Span) {
        self.member_assign_rhs_verbatim_spans
            .insert((member_span.start, member_span.end));
    }

    /// Run the COMPLETE scope-aware walk over one expression node.
    fn rewrite_expr(&mut self, expr: &Expression<'_>) {
        self.visit_expression(expr);
    }

    /// The rewritten READ text for a rune-binding identifier (a signal read →
    /// `$.get(name)`, a `$props()` member read → `name()` / `$$props.x`), or `None`
    /// for a non-signal / shadowed identifier (which stays the original source).
    fn read_rewrite_text(&self, name: &str) -> Option<String> {
        match self.signal_kind(name) {
            Some(k) if is_signal_kind(k) => Some(format!("$.get({name})")),
            // A `{#snippet}` PARAMETER reads as a THUNK CALL `name()` (the official
            // `transform[arg] = { read: b.call }` — a snippet receives its args as
            // zero-arg getter thunks defaulting to `$.noop`).
            Some(BindingRuntimeKind::SnippetParam) => Some(format!("{name}()")),
            // A `$store` auto-subscription read (`$count`) calls the accessor
            // thunk (`$count()` — the `const $count = () => $.store_get(count,
            // '$count', $$stores);` declaration), never `$.get`.
            Some(BindingRuntimeKind::StoreSubscription) => Some(format!("{name}()")),
            Some(BindingRuntimeKind::Prop | BindingRuntimeKind::BindableProp) => {
                match self.ctx.prop_reads.get(name) {
                    // A PROP-SOURCE member (default-bearing or written — declared
                    // via `$.prop`) reads as a getter call `name()`.
                    Some(PropRead::Getter) => Some(format!("{name}()")),
                    // A non-source prop reads off the props object by its SOURCE
                    // key. A non-identifier-safe source key (`foo-bar`) reads via
                    // BRACKET access (`$$props['foo-bar']`); an identifier-safe key
                    // reads via dotted access (`$$props.foo`).
                    Some(PropRead::PropsMember { source_key }) => {
                        Some(props_member_access(source_key))
                    }
                    // A rest / whole-object capture binding: its BARE read stays
                    // the verbatim real local (`let rest = $.rest_props(…)`). The
                    // KEY-AWARE member read (`rest.KEY` → `$$props.KEY`) is owned by
                    // the member-expression visit, NOT this identifier leaf.
                    Some(PropRead::RestBinding { .. }) => None,
                    // No recorded read form — a plain props member by name (the
                    // binding name is always identifier-safe here).
                    None => Some(format!("$$props.{name}")),
                }
            }
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
        // A DIRECT static-member assignment target (`rest.x = 1`) is a WRITE — the
        // rest / whole-object member disposition keeps it VERBATIM (READ-only
        // de-localization). Record its span BEFORE the target is descended.
        if let AssignmentTarget::StaticMemberExpression(m) = &assign.left {
            self.note_member_write_target(m.span);
        }
        // A bare static `rest.KEY`/`all.KEY` WHOLE RHS stays VERBATIM under plain `=`
        // (any target) OR a compound/logical `OP=` whose target is NOT a reassignable
        // signal (svelte re-wraps `SignalIdent`/`PropSetter` → Binary → de-localizes).
        let target = self.classify_target(&assign.left);
        if assign.operator == AssignmentOperator::Assign
            || matches!(target, ClientLvalue::PlainIdent | ClientLvalue::Member)
        {
            if let Some(span) = bare_member_rhs_verbatim_span(&assign.right) {
                self.note_member_assign_rhs_verbatim(span);
            }
        }
        match target {
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
            // A `$store` subscription write lowers through the store setter:
            // `$c = rhs` → `$.store_set(c, rhs)`; a compound `$c += y` →
            // `$.store_set(c, $c() + y)` (the official compound-assign form —
            // NEVER `$.update_store`, which is the ++/-- family). A store write
            // never carries the proxy mutation flag.
            ClientLvalue::StoreIdent { name } => {
                let base = &name[1..];
                let left_start = assign.left.span().start;
                let rhs_start = assign.right.span().start;
                let rhs_end = assign.right.span().end;
                let head_text = match assign.operator {
                    AssignmentOperator::Assign => format!("$.store_set({base}, "),
                    op => {
                        let base_op = compound_base_operator(op);
                        format!("$.store_set({base}, {name}() {base_op} ")
                    }
                };
                self.occurrences.push(Occurrence::SignalReassign {
                    head_span: oxc_span::Span::new(left_start, rhs_start),
                    head_text,
                    append_at: rhs_end,
                    append_text: ")".to_string(),
                });
                // Recurse the RHS only (the head identifier is consumed above).
                self.visit_expression(&assign.right);
            }
            // A PROP-SOURCE write lowers through the getter/setter function:
            // `name = rhs` → `name(rhs)`; a compound `name += y` →
            // `name(name() + y)`. An identifier reassignment never carries the
            // mutation flag (official `assign: (node, value) => b.call(node,
            // value)` — no trailing `true`).
            ClientLvalue::PropSetter { name } => {
                let left_start = assign.left.span().start;
                let rhs_start = assign.right.span().start;
                let rhs_end = assign.right.span().end;
                let head_text = match assign.operator {
                    AssignmentOperator::Assign => format!("{name}("),
                    op => {
                        let base = compound_base_operator(op);
                        format!("{name}({name}() {base} ")
                    }
                };
                self.occurrences.push(Occurrence::SignalReassign {
                    head_span: oxc_span::Span::new(left_start, rhs_start),
                    head_text,
                    append_at: rhs_end,
                    append_text: ")".to_string(),
                });
                // Recurse the RHS only (the head identifier is consumed above).
                self.visit_expression(&assign.right);
            }
            ClientLvalue::ImportedBinding => {
                // An assignment to an ES `import` binding — an OFFICIAL compile error
                // (`constant_assignment`, "Cannot assign to import"), carried out
                // through the rewriter's official-reject channel.
                self.refuse(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::ConstantAssignment,
                    ),
                    span: VerterSpan::new(assign.span.start, assign.span.end),
                });
            }
            ClientLvalue::UnsupportedReactiveTarget | ClientLvalue::UnsupportedTarget => {
                // A TS-wrapped reactive write or a destructuring write — fail closed.
                self.refuse(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
                    span: VerterSpan::new(assign.span.start, assign.span.end),
                });
            }
            ClientLvalue::Member | ClientLvalue::PlainIdent => {
                // A BINDABLE-prop member MUTATION wraps the whole assignment in
                // the setter with the mutation flag (`v.a = 1` → `v(v().a = 1,
                // true)` — official interop with legacy parent bindings); the
                // wrapped mutation keeps its own leaf edits (the base identifier
                // read-rewrites to the getter below). A PLAIN-prop / proxy member
                // write stays a raw member write.
                if let Some(root) = self.member_assignment_bindable_root(&assign.left) {
                    self.occurrences.push(Occurrence::WrapCall {
                        insert_at: assign.span.start,
                        head_text: format!("{root}("),
                        append_at: assign.span.end,
                        append_text: ", true)".to_string(),
                    });
                }
                // A member write rooted at a promoted legacy `$.mutable_source`
                // wraps the WHOLE assignment in the official deep-mutation
                // helper (`o.x = v` → `$.mutate(o, $.get(o).x = v)`); the root
                // read inside still rewrites through the normal signal leaf
                // edit (recursed below).
                if let Some(root) = self.member_assignment_mutable_source_root(&assign.left) {
                    self.occurrences.push(Occurrence::WrapCall {
                        insert_at: assign.span.start,
                        head_text: format!("$.mutate({root}, "),
                        append_at: assign.span.end,
                        append_text: ")".to_string(),
                    });
                }
                // A member-rooted or non-signal target: recurse both sides (a
                // `BareProxy` member write stays plain; a member of a signal object
                // reads via a getter, handled by recursing into the target object).
                self.visit_assignment_target(&assign.left);
                self.visit_expression(&assign.right);
            }
        }
    }

    /// The `$.mutable_source` root of an ASSIGNMENT member target, or `None`
    /// (delegates to [`Self::member_root_mutable_source`] over the member
    /// target's object).
    fn member_assignment_mutable_source_root(
        &self,
        target: &AssignmentTarget<'_>,
    ) -> Option<String> {
        match target {
            AssignmentTarget::StaticMemberExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            AssignmentTarget::PrivateFieldExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            _ => None,
        }
    }

    /// The bindable-prop-source root of an ASSIGNMENT member target, or `None`
    /// (delegates to [`Self::member_root_bindable_prop_source`] over the member
    /// target's object).
    fn member_assignment_bindable_root(&self, target: &AssignmentTarget<'_>) -> Option<String> {
        match target {
            AssignmentTarget::StaticMemberExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            AssignmentTarget::PrivateFieldExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            _ => None,
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
        // A DIRECT static-member update target (`rest.x++`) is a WRITE — the rest /
        // whole-object member disposition keeps it VERBATIM (READ-only
        // de-localization). Record its span BEFORE the walk re-reaches the member.
        if let SimpleAssignmentTarget::StaticMemberExpression(m) = &update.argument {
            self.note_member_write_target(m.span);
        }
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
            // A `$store` subscription update lowers through the store update
            // helpers, each carrying the CURRENT accessor value: `$c++` →
            // `$.update_store(c, $c())`, `$c--` → `$.update_store(c, $c(), -1)`,
            // `++$c` → `$.update_pre_store(c, $c())`, `--$c` →
            // `$.update_pre_store(c, $c(), -1)` (the official prefix/decrement
            // forms — oracle-verified against svelte@5.56.3).
            ClientLvalue::StoreIdent { name } => {
                let base = &name[1..];
                let helper = if update.prefix {
                    "update_pre_store"
                } else {
                    "update_store"
                };
                let text = match update.operator {
                    UpdateOperator::Increment => format!("$.{helper}({base}, {name}())"),
                    UpdateOperator::Decrement => format!("$.{helper}({base}, {name}(), -1)"),
                };
                self.occurrences.push(Occurrence::SignalUpdate {
                    span: update.span,
                    text,
                });
            }
            // A PROP-SOURCE update lowers through the prop update helpers:
            // `name++` → `$.update_prop(name)`, `--name` →
            // `$.update_pre_prop(name, -1)` (the official prefix/decrement forms,
            // mirroring the signal `$.update` family).
            ClientLvalue::PropSetter { name } => {
                let helper = if update.prefix {
                    "update_pre_prop"
                } else {
                    "update_prop"
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
            ClientLvalue::ImportedBinding => {
                // An update of an ES `import` binding (`x++`) — the same official
                // `constant_assignment` compile error as the assignment form.
                self.refuse(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::ConstantAssignment,
                    ),
                    span: VerterSpan::new(update.span.start, update.span.end),
                });
            }
            ClientLvalue::UnsupportedReactiveTarget | ClientLvalue::UnsupportedTarget => {
                self.refuse(UnsupportedSvelteRuntimeSurface::DestructuringWrite {
                    span: VerterSpan::new(update.span.start, update.span.end),
                });
            }
            ClientLvalue::Member | ClientLvalue::PlainIdent => {
                // A BINDABLE-prop member MUTATION wraps the whole update in the
                // setter with the mutation flag (`v.a++` → `v(v().a++, true)`);
                // a PLAIN-prop / proxy member update stays a raw member write
                // over the rewritten getter base.
                if let Some(root) = self.member_update_bindable_root(&update.argument) {
                    self.occurrences.push(Occurrence::WrapCall {
                        insert_at: update.span.start,
                        head_text: format!("{root}("),
                        append_at: update.span.end,
                        append_text: ", true)".to_string(),
                    });
                }
                // A member update rooted at a promoted legacy `$.mutable_source`
                // wraps in the official deep-mutation helper (`o.x++` →
                // `$.mutate(o, $.get(o).x++)`).
                if let Some(root) = self.member_update_mutable_source_root(&update.argument) {
                    self.occurrences.push(Occurrence::WrapCall {
                        insert_at: update.span.start,
                        head_text: format!("$.mutate({root}, "),
                        append_at: update.span.end,
                        append_text: ")".to_string(),
                    });
                }
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

    /// The `$.mutable_source` root of an UPDATE member target, or `None`
    /// (delegates to [`Self::member_root_mutable_source`] over the member
    /// target's object).
    fn member_update_mutable_source_root(
        &self,
        target: &SimpleAssignmentTarget<'_>,
    ) -> Option<String> {
        match target {
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                self.member_root_mutable_source(&m.object)
            }
            _ => None,
        }
    }

    /// The bindable-prop-source root of an UPDATE member target, or `None`
    /// (delegates to [`Self::member_root_bindable_prop_source`] over the member
    /// target's object).
    fn member_update_bindable_root(&self, target: &SimpleAssignmentTarget<'_>) -> Option<String> {
        match target {
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                self.member_root_bindable_prop_source(&m.object)
            }
            _ => None,
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
        if it.r#expression {
            // A CONCISE (expression-bodied) arrow: OXC models the body as ONE
            // synthetic `ExpressionStatement`, but it is an EXPRESSION position,
            // not a droppable statement — dropping it would remove the whole
            // body (`() => )`, invalid JS). Visit the params and the body
            // EXPRESSION directly so the statement-drop never fires; a concise
            // `$inspect.trace()` then refuses via the identifier walk (matching
            // the official `inspect_trace_invalid_placement` error).
            self.visit_formal_parameters(&it.params);
            if let [Statement::ExpressionStatement(stmt)] = it.body.statements.as_slice() {
                self.visit_expression(&stmt.expression);
            } else {
                self.visit_function_body(&it.body);
            }
        } else {
            walk::walk_arrow_function_expression(self, it);
        }
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

    fn visit_parenthesized_expression(&mut self, it: &ParenthesizedExpression<'a>) {
        // A transparent author-paren WRAPPER around an effect-family call
        // (`(/*#__PURE__*/ $effect.root(fn))` in a handler / nested body).
        // Record the wrapper start against the (peeled) call span BEFORE the
        // walk descends, so the accepted invocation-head rewrite can relocate
        // the wrapper-GAP comments into the emitted helper call instead of
        // leaving them call-leading. An ancestor paren is visited before the
        // call it wraps, so the FIRST insert per call is the OUTERMOST wrapper
        // — the gap range then covers every nested wrapper's comments in
        // source order. Shadowing and acceptance are the call visitor's checks
        // (an unused entry is inert); a non-family paren records nothing.
        if let Some(fact) = effect_family_expression_fact(&it.expression) {
            self.wrapper_heads
                .entry((fact.call_span.start, fact.call_span.end))
                .or_insert(it.span.start);
        }
        walk::walk_parenthesized_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // A `$host()` call (unshadowed STRICT bare-identifier callee) lowers to the
        // `$$props.$$host` member read — the official client CallExpression rewrite
        // — replacing the WHOLE call span (there are no arguments to keep). Only
        // the ZERO-ARG, NON-OPTIONAL spelling reaches this arm accepted: the rune
        // scan admits exactly that form (and only inside an ACTIVE customElement
        // component) before the plan is ever built, so an unshadowed `$host` call
        // in ANY other shape here is a scan/rewriter divergence — refuse it
        // (defense-in-depth: the rewriter can never emit a raw `$host`, which
        // would be a runtime `ReferenceError`). A SHADOWED `$host` (a local of
        // that name) is an ordinary user call and is left untouched.
        if !self.is_local("$host") {
            if let Expression::Identifier(id) = &it.callee {
                if id.name.as_str() == "$host" {
                    if !it.optional && it.arguments.is_empty() {
                        self.occurrences.push(Occurrence::ReadRewrite {
                            span: it.span,
                            text: "$$props.$$host".to_string(),
                        });
                        // The whole call (callee + empty argument list) is
                        // consumed by the rewrite — nothing left to walk.
                        return;
                    }
                    self.refuse(UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                        surface: "$host",
                        span: VerterSpan::new(it.span.start, it.span.end),
                    });
                }
            }
        }
        // A `$state.snapshot(x)` call rewrites its CALLEE member to the `$.snapshot`
        // helper (a SPAN-replacement over the `$state.snapshot` callee). The argument
        // is recursed by the generic walk below, so a nested `$state.snapshot(...)`
        // and a signal read inside the argument still rewrite. A SHADOWED `$state` (a
        // local of that name) is an ordinary member call — not the rune — and is left
        // untouched. Driven from the typed OXC AST only.
        if !self.is_local("$state") {
            if let Some(callee_span) = state_snapshot_callee_span(it) {
                self.occurrences.push(Occurrence::ReadRewrite {
                    span: callee_span,
                    text: "$.snapshot".to_string(),
                });
            }
        }
        // A WELL-FORMED effect-family rune call (`$effect(fn)` / `$effect.pre(fn)`
        // / `$effect.root(fn)` / `$effect.tracking()`, unshadowed) in a LEGAL
        // POSITION rewrites its invocation head to the registered runtime helper.
        // The user-effect members are STATEMENT-ONLY and NON-OPTIONAL (official
        // `effect_invalid_placement`: legal only as the expression of an
        // `ExpressionStatement` — a concise-arrow body, a declarator init, a
        // `return` / call argument, and EVERY optional invocation all REJECT);
        // their admissible call spans were recorded by
        // `visit_expression_statement` (or the statement-role entry seed) before
        // this visitor ran. `.root` / `.tracking` are expression-valued (no
        // position requirement; optional invocations admit). The callee is
        // CONSUMED by the rewrite — the walk covers the ARGUMENTS only, so the
        // `$effect` reference refusal below never fires on a consumed callee
        // while a nested family call / signal read / `await` inside the argument
        // still lowers (or refuses) recursively. A MALFORMED call (wrong arity,
        // a spread argument) or a VALUE-POSITION / optional user-effect call
        // records the precise family refusal — the rune scan fails these closed
        // upstream; this arm is defense-in-depth so the rewriter can NEVER emit
        // a raw or official-rejected effect-family rune.
        if !self.is_local("$effect") {
            if let Some(fact) = effect_family_call_fact(it) {
                let position_ok = match fact.kind {
                    EffectFamilyCallKind::UserEffect | EffectFamilyCallKind::UserPreEffect => {
                        !fact.optional
                            && self
                                .stmt_effect_spans
                                .contains(&(it.span.start, it.span.end))
                    }
                    EffectFamilyCallKind::EffectRoot | EffectFamilyCallKind::EffectTracking => true,
                };
                // ONE invocation-head rule for EVERY accepted family form —
                // plain, member, optional-receiver, optional-call, zero-arg:
                // overwrite `callee_start..open_paren_end` with
                // `{helper}({relocated_trivia}`. The head is rewritten THROUGH
                // the opening call paren, and comment trivia the overwrite
                // destroys re-emits INSIDE the emitted helper call, immediately
                // after the emitted `(` — the canonical call-internal slot
                // ([`call_internal_comment_trivia`]). The head can never mint a
                // call-LEADING prefix: a leading `/*#__PURE__*/` would annotate
                // the helper call and a leading `//` line comment after
                // `return` would arm ASI against the emitted call. Bytes after
                // the opening paren stay in place, so arg-leading trivia and a
                // zero-arg `()` interior (`$effect.tracking?.(/*c*/)`) survive
                // untouched — never duplicated — and every `?.` head
                // normalizes away (official emits the helper call PLAIN; a
                // callee-span-only rewrite would leave a structural
                // `$.effect_root?.(…)` divergence). The arguments keep their
                // own spans, so nested edits stay disjoint. A parsed call
                // always has its opening paren, so the not-found arm is
                // unreachable and falls through to the fail-closed family
                // refusal below (defense-in-depth — never a raw or
                // half-rewritten emission).
                let head_end = (fact.well_formed && position_ok)
                    .then(|| {
                        head_open_paren_end(
                            self.wrapped,
                            self.comments,
                            fact.callee_span.end,
                            it.arguments
                                .first()
                                .map_or(it.span.end, |arg| arg.span().start),
                        )
                    })
                    .flatten();
                if let Some(open_paren_end) = head_end {
                    let helper = format!("$.{}", fact.kind.helper().ident());
                    // Wrapper-head trivia the instance-item carrier pre-collected
                    // for THIS call (the carrier slice normalizes transparent
                    // author parens away; their interior head comments relocate
                    // here, ahead of the head's own trivia — source order).
                    let carrier_lead =
                        if self.carrier_trivia_target == Some((it.span.start, it.span.end)) {
                            self.carrier_head_trivia
                        } else {
                            ""
                        };
                    // Wrapper-GAP trivia of an IN-SOURCE transparent author-paren
                    // wrapper around this call (`(/*#__PURE__*/ $effect.root(fn))`
                    // in a handler / nested body — the general path keeps the
                    // wrapper parens, but a gap comment left in place would sit
                    // call-leading before the rewritten helper call,
                    // PURE-activating). Each gap comment is removed from its
                    // source slot and re-emitted inside the emitted helper call,
                    // between the carrier trivia and the head's own trivia
                    // (outermost-first source order).
                    let wrapper_lead = match self
                        .wrapper_heads
                        .get(&(it.span.start, it.span.end))
                        .copied()
                    {
                        Some(wrapper_start) => {
                            let comments = self.comments;
                            for c in comments {
                                if c.span.start >= wrapper_start && c.span.end <= it.span.start {
                                    self.occurrences
                                        .push(Occurrence::RelocatedWrapperComment { span: c.span });
                                }
                            }
                            call_internal_comment_trivia(
                                self.wrapped,
                                comments,
                                wrapper_start,
                                it.span.start,
                            )
                        }
                        None => String::new(),
                    };
                    let lead = call_internal_comment_trivia(
                        self.wrapped,
                        self.comments,
                        fact.callee_span.start,
                        open_paren_end,
                    );
                    self.occurrences.push(Occurrence::ReadRewrite {
                        span: oxc_span::Span::new(fact.callee_span.start, open_paren_end),
                        text: format!("{helper}({carrier_lead}{wrapper_lead}{lead}"),
                    });
                    for arg in &it.arguments {
                        match arg.as_expression() {
                            Some(expr) => self.visit_expression(expr),
                            None => {
                                if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                                    self.visit_expression(&s.argument);
                                }
                            }
                        }
                    }
                    return;
                }
                self.refuse(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: fact.kind.rune_label(),
                    span: VerterSpan::new(it.span.start, it.span.end),
                });
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        // A production-ELIDED `$inspect.trace(...)` expression STATEMENT inside a
        // lowered function / arrow body is DROPPED IN PLACE (official `dev:false`
        // removes the call; the surrounding body statements are preserved). The
        // walk does NOT descend into the dropped statement, so no inner occurrence
        // (and no `$inspect` refusal) is recorded inside the removed span. A
        // SHADOWED `$inspect` is an ordinary local call and lowers normally.
        if is_inspect_trace_call(&it.expression) && !self.is_local("$inspect") {
            self.occurrences
                .push(Occurrence::DropStatement { span: it.span });
            return;
        }
        // A statement-position `$effect(...)` / `$effect.pre(...)` call — the ONE
        // official-legal position for the user-effect members
        // (`effect_invalid_placement` is a direct-parent rule; parens are
        // transparent). Record the call span BEFORE the walk descends so the call
        // visitor admits the callee rewrite; a shadowed `$effect` never consults
        // the set (the call visitor's local check owns shadowing).
        if let Some(span) = statement_position_user_effect_span(&it.expression) {
            self.stmt_effect_spans.insert((span.start, span.end));
        }
        walk::walk_expression_statement(self, it);
    }

    fn visit_static_member_expression(&mut self, it: &oxc_ast::ast::StaticMemberExpression<'a>) {
        // A KEY-AWARE `$props()` rest / whole-object MEMBER (`rest.KEY` / `all.KEY`).
        // The bare-`rest` leaf is verbatim (its `RestBinding` read form is `None`),
        // so the member is decided HERE, over the STATIC key. Author parens around
        // the IMMEDIATE object are transparent (official's ESTree AST has no paren
        // nodes), so `(rest).KEY` / `((rest)).KEY` peel to the same root — the peel
        // is object-only and never descends a nested member chain, so `rest.x.y`
        // still keys on its ROOT property `x` (the inner member visit de-localizes
        // `x`), and a computed member (`rest['x']`) never reaches this static
        // visitor (it stays verbatim). The disposition, in order:
        //  - a `$$`-prefixed KEY is the reserved magic namespace — an OFFICIAL
        //    compile error (`props_illegal_name`), fired regardless of excludes, of
        //    parens, and of read/write position;
        //  - a WRITE-LIKE target (`rest.KEY = …` / `rest.KEY++`) OR a bare `rest.KEY`
        //    that is the entire RHS of a plain `=` (`sink = rest.KEY`) stays the
        //    verbatim `rest.KEY` — the two span sets the oracle's coarse
        //    Assignment/Update-child guard keeps local (de-localization is READ-only
        //    for the write, position-suppressed for the plain-`=` read);
        //  - a READ with a NON-excluded KEY de-localizes the OBJECT identifier
        //    (`rest` / `all`) to `$$props`, replacing ONLY the object span so the
        //    optional axis (`?.`), the property spelling, and any downstream chain
        //    stay verbatim from source: `rest.x` → `$$props.x`, `rest?.x` →
        //    `$$props?.x`, `rest?.x.y` → `$$props?.x.y`. Subtree NOT descended;
        //  - a READ with an EXCLUDED KEY stays the verbatim `rest.KEY` (the rest
        //    object owns that member) — record nothing, do not descend.
        if let Expression::Identifier(root) = peel_parens(&it.object) {
            if let Some(excludes) = self.rest_binding_excludes(root.name.as_str()) {
                let key = it.property.name.as_str();
                if key.starts_with("$$") {
                    // A `$$`-prefixed member of a rest / whole-object binding is the
                    // reserved magic namespace — an OFFICIAL compile error
                    // (`props_illegal_name`), carried out through the rewriter's
                    // official-reject channel (NOT the generic magic-identifier
                    // unsupported surface). Fires regardless of excludes, of parens
                    // (the peel above sees through them), and of read/write position.
                    self.refuse(UnsupportedSvelteRuntimeSurface::OfficialReject {
                        rejection: OfficialRejection::of(
                            CoreOfficialValidationRule::PropsIllegalName,
                        ),
                        span: VerterSpan::new(it.span.start, it.span.end),
                    });
                    return;
                }
                // A WRITE-LIKE target OR a plain-`=` bare-member RHS stays verbatim
                // (the coarse Assignment/Update-child guard — see the header above).
                let span = (it.span.start, it.span.end);
                if self.member_write_target_spans.contains(&span)
                    || self.member_assign_rhs_verbatim_spans.contains(&span)
                {
                    return;
                }
                if !excludes.contains(key) {
                    // Replace ONLY the OBJECT identifier span (`rest` / `all` →
                    // `$$props`), NOT the whole member span, so the `?.` optional
                    // axis, the property spelling, and any downstream chain stay
                    // verbatim from source: `rest.x` → `$$props.x`, `rest?.x` →
                    // `$$props?.x`, `rest?.x.y` → `$$props?.x.y`, `rest?.x?.y` →
                    // `$$props?.x?.y`. For an ASCII-identifier key the non-optional
                    // result is byte-identical to a dotted `props_member_access(key)`
                    // (`$$props.KEY`); for a non-ASCII/Unicode static key the preserved
                    // dotted spelling (`rest.café` → `$$props.café`) is official
                    // svelte's form, whereas `props_member_access` — bracketing under
                    // its ASCII-only `is_js_identifier` guard — emits `$$props['café']`.
                    // (Numeric / hyphenated keys reach only the computed-member path,
                    // never this static visitor, so Unicode is the sole non-ASCII
                    // class here.) `it.object.span()` covers the whole object
                    // subexpression (a peeled paren wrapper included), so `(rest).x`
                    // → `$$props.x` exactly like official.
                    self.occurrences.push(Occurrence::ReadRewrite {
                        span: it.object.span(),
                        text: "$$props".to_string(),
                    });
                }
                return;
            }
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // An UNSHADOWED `$inspect` reference reaching a NON-elided position (a
        // concise arrow body `() => $inspect.trace()` — an official ERROR
        // (`inspect_trace_invalid_placement`) — an interpolation `{$inspect(c)}`,
        // a call argument) is outside the supported production-elision surface
        // (statement position only). Fail closed rather than emit a raw
        // `$inspect` reference (a runtime `ReferenceError`); a statement-position
        // `$inspect.trace()` was consumed by the drop above (its subtree is never
        // walked), and the top-level `$inspect(...)` / `.with(...)` statements
        // never reach the rewriter (the instance-item classifier elides them).
        if it.name.as_str() == "$inspect" && !self.is_local("$inspect") {
            self.refuse(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$inspect",
                span: VerterSpan::new(it.span.start, it.span.end),
            });
        }
        // An unshadowed `$effect` reference NOT consumed by an admitted family
        // callee rewrite (a value reference `foo($effect)`, an uncalled member
        // `const f = $effect.pre;`, the callee of a malformed or value-position
        // user-effect call) is outside the supported effect-family surface —
        // fail closed rather than emit a raw `$effect` reference (a runtime
        // ReferenceError). The admitted family calls never reach here: their
        // callee is consumed by the call visitor (which walks only the
        // arguments).
        if it.name.as_str() == "$effect" && !self.is_local("$effect") {
            self.refuse(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$effect",
                span: VerterSpan::new(it.span.start, it.span.end),
            });
        }
        // An unshadowed `$host` reference NOT consumed by the zero-arg call
        // rewrite (an uncalled bare `$host`, the inner identifier of a
        // parenthesized-callee `($host)()` spelling) is outside the supported
        // host surface — the rune scan refuses these upstream, so this is
        // defense-in-depth: the rewriter can never emit a raw `$host` reference
        // (a runtime ReferenceError). The admitted call never reaches here (its
        // whole span is consumed by the call visitor's rewrite).
        if it.name.as_str() == "$host" && !self.is_local("$host") {
            self.refuse(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$host",
                span: VerterSpan::new(it.span.start, it.span.end),
            });
        }
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

// The leaf classification + rendering helpers the two passes consume —
// `expression_contains_ts_only_syntax`, `state_snapshot_callee_span`, the
// assignment-operator tables (`compound_base_operator` /
// `is_non_coercive_operator`), and `props_member_access` — live in the sibling
// [`super::plan_render`] module (imported at the top of this file).
