//! Instance-script `$state` / plain-local BINDING PREPARATION + final classification.
//!
//! This owns the binding-table population for the instance script: registering each
//! `$state` declaration with a PROVISIONAL (script-side) classification
//! ([`prepare_state_bindings`]), registering the remaining plain-local `let`
//! declarations ([`prepare_plain_local_bindings`]), then — once the template scope
//! graph is complete — attributing the template-side writes and computing each
//! `$state` binding's FINAL write-gated lowering (via [`finalize_state_classifications`]
//! and [`attribute_bind_target_writes`]). The lowering decision is write-gated, so it
//! cannot finalize until a write that may live in a template expression (an
//! `onclick={() => count++}` handler or a `bind:` write-back) is resolved
//! scope-awarely.

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;

use super::expr::{
    classify_state_lowering, reparse_module, BindingInfo, BindingRuntimeKind, BindingTable,
    BindingUseSet, ExprRefKind, ScopeGraph, ScopeId, ScriptUseCollector, StateClassification,
    StateRuneKind,
};
use super::ir::{AttrIr, BindingId, IrNode};
use super::state_scan::collect_state_declarations;
use super::{LoweringCtx, StateLowering};

/// One tracked instance-script `$state` binding awaiting final classification:
/// its declaration facts, its SCRIPT-side observed uses, and the [`BindingId`] of
/// its root-scope binding row.
pub(super) struct TrackedState {
    /// The declared rune flavour.
    declared: StateRuneKind,
    /// Whether the initializer is PROXIABLE (`should_proxy(init)`). Init-shape
    /// only — it never changes after declaration.
    proxiable: bool,
    /// The uses observed on the SCRIPT side (refined by template writes later).
    script_uses: BindingUseSet,
    /// The root-scope binding row to finalize.
    binding: BindingId,
}

/// Declare a retained script program's `$state` bindings in the owning scope with a
/// PROVISIONAL (script-side) classification, returning the tracking data the
/// post-template finalizer needs.
///
/// The classification is only provisional here because a `$state` binding's
/// lowering is WRITE-gated and a write may live in a TEMPLATE expression
/// (`onclick={() => count++}`) whose scope graph does not exist until the
/// template is lowered. The final classification happens in
/// [`finalize_state_classifications`] once the scope graph is complete and a
/// shadowing template binding can be resolved.
pub(super) fn prepare_state_bindings(
    program: Option<&Program<'_>>,
    root_scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) -> Vec<TrackedState> {
    let Some(program) = program else {
        return Vec::new();
    };
    let decls = collect_state_declarations(program);
    if decls.is_empty() {
        return Vec::new();
    }

    // Collect script-side uses (reassign / deep-mutate) for each declared state
    // name, scope-aware (a nested local of the same name shadows).
    let names: Vec<String> = decls.iter().map(|(n, _, _)| n.clone()).collect();
    let mut collector = ScriptUseCollector::tracking(&names);
    use oxc_ast_visit::Visit;
    collector.visit_program(program);

    let mut tracked = Vec::with_capacity(decls.len());
    for (name, declared, proxiable) in decls {
        let script_uses = collector.use_set(&name);
        let lowering = classify_state_lowering(declared, proxiable, script_uses);
        let binding = bindings.push(BindingInfo {
            name: name.clone(),
            scope: root_scope,
            kind: state_kind_for_lowering(lowering),
            state: Some(StateClassification {
                declared,
                proxiable,
                uses: script_uses,
                lowering,
            }),
        });
        scopes.declare(root_scope, &name, binding);
        tracked.push(TrackedState {
            declared,
            proxiable,
            script_uses,
            binding,
        });
    }
    tracked
}

/// The supported rune classification of a BLOCK declaration-tag declarator
/// (`{let x = $state(…)}` / `{let x = $derived(…)}`), returned by
/// [`classify_block_rune_declarator`].
pub(super) enum BlockRuneDeclarator {
    /// `$state(<primitive>)` — the binding was reclassified through the write-gated
    /// `$state` pipeline; carries the tracking row (for the post-template finalizer) plus
    /// the primitive init source text (inner `None` for the no-arg `$state()` form).
    State {
        /// The tracking row to push onto the post-template state finalizer.
        tracked: TrackedState,
        /// The primitive init source text, or `None` for the no-arg `$state()`.
        init: Option<String>,
    },
    /// `$derived(<arg>)` — the binding was classified as a `Derived` signal; carries the
    /// argument expression's `(start, end)` byte range RELATIVE to `init_text` (the caller
    /// pushes the `ExprId` for the projection to rewrite into a `$.derived(() => …)` body).
    Derived {
        /// The `$derived` argument span, relative to `init_text`.
        arg: (u32, u32),
    },
}

/// Classify a BLOCK declaration-tag declarator's initializer as a SUPPORTED rune.
///
/// A `{let x = $state(<primitive>)}` is reclassified through the SAME write-gated `$state`
/// pipeline as an instance-script declaration (in the block-body `scope`); a
/// `{let x = $derived(<arg>)}` becomes a `Derived` signal (reads `$.get`). ANY other init
/// — a non-rune expression, an object/array (proxy) `$state`, a multi-arg `$state`,
/// `$derived.by`, `$derived()`/multi-arg `$derived`, a spread — returns `None`: the
/// declarator stays INERT and a rune form the pipeline cannot lower fails closed at the
/// rewriter's advanced-rune gate (never mis-emitting the un-imported `$state`/`$derived`).
pub(super) fn classify_block_rune_declarator(
    binding: BindingId,
    init_text: &str,
    bindings: &mut BindingTable,
) -> Option<BlockRuneDeclarator> {
    use oxc_ast::ast::{Expression, Statement};
    use oxc_span::GetSpan;
    let alloc = Allocator::default();
    let wrapped = format!("({init_text});");
    let program = reparse_module(&alloc, &wrapped)?;
    let Some(Statement::ExpressionStatement(stmt)) = program.body.first() else {
        return None;
    };
    let mut expr = &stmt.expression;
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    let Expression::CallExpression(call) = expr else {
        return None;
    };

    // `$derived(<single expr>)` — a block-local derived memo. `$derived.by`, a multi-arg /
    // no-arg / spread `$derived` is NOT lowered here (it stays inert → the rewriter's
    // advanced-rune gate refuses it). The check is on the BARE `$derived` callee.
    if matches!(&call.callee, Expression::Identifier(id) if id.name.as_str() == "$derived") {
        if let [arg] = call.arguments.as_slice() {
            if let Some(arg_expr) = arg.as_expression() {
                // Reads of a derived binding are signals (`$.get`); it is not write-gated.
                let info = bindings.get_mut(binding);
                info.kind = BindingRuntimeKind::Derived;
                info.state = None;
                let span = arg_expr.span();
                // `wrapped = "(<init_text>);"` — strip the leading `(` to map to `init_text`.
                return Some(BlockRuneDeclarator::Derived {
                    arg: (span.start.saturating_sub(1), span.end.saturating_sub(1)),
                });
            }
        }
        return None;
    }

    // `$state(<primitive literal>)` — the supported state declarator. The object/array
    // proxy form, a multi-arg / spread init, and a non-`$state` call are NOT reclassified
    // (they stay inert; a `$state`/`$derived` call init then fails closed at the rewriter).
    let declared = super::expr::state_rune_call(call)?;
    let init = match call.arguments.as_slice() {
        [] => None,
        [arg] => {
            let arg = arg.as_expression()?;
            if !block_rune_init_is_primitive(arg) {
                return None;
            }
            let span = arg.span();
            Some(wrapped[span.start as usize..span.end as usize].to_string())
        }
        _ => return None,
    };
    // A block rune declarator is a primitive `$state`, never proxiable; the provisional
    // (script-side) use set is empty — a template write is attributed by the finalizer. The
    // binding row already exists (declared by `push_pattern_names`); reclassify it in place.
    let proxiable = false;
    let script_uses = BindingUseSet::default();
    let lowering = classify_state_lowering(declared, proxiable, script_uses);
    let info = bindings.get_mut(binding);
    info.kind = state_kind_for_lowering(lowering);
    info.state = Some(StateClassification {
        declared,
        proxiable,
        uses: script_uses,
        lowering,
    });
    Some(BlockRuneDeclarator::State {
        tracked: TrackedState {
            declared,
            proxiable,
            script_uses,
            binding,
        },
        init,
    })
}

/// Whether a `$state(<arg>)` block-declarator init is a PRIMITIVE literal (the supported
/// shape): a number / string / boolean / null / bigint literal, or a unary `-`/`+`/`!` of
/// one. An object / array / call / identifier init is the deferred non-primitive form.
fn block_rune_init_is_primitive(expr: &oxc_ast::ast::Expression<'_>) -> bool {
    use oxc_ast::ast::Expression;
    match expr {
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::UnaryExpression(unary) => block_rune_init_is_primitive(&unary.argument),
        _ => false,
    }
}

/// Declare the top-level PLAIN-local instance-script bindings (`let v = …` that is
/// NOT a `$state` / `$derived` / `$props()` rune call) in `scope` as `PlainLocal`.
///
/// Only an identifier-pattern `let` declarator whose init is NOT a rune call
/// contributes — a `$state` / `$derived` / `$props()` declarator was already
/// registered by [`prepare_state_bindings`] / `prepare_rune_bindings`, and a name
/// ALREADY declared in the scope is skipped (so a reactive binding is never demoted to
/// a plain local). A plain local is a non-reactive binding; it is needed so a DOM
/// bind-target lvalue ROOT (`bind:value={v}` / `bind:value={o.x}`) resolves to
/// `PlainLocal`, selecting the plain-assignment setter. (A `const` / `var` is NOT
/// registered here — those keep their distinct surfaces; only `let` is core.)
pub(super) fn prepare_plain_local_bindings(
    program: Option<&Program<'_>>,
    scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) {
    let Some(program) = program else {
        return;
    };
    use oxc_ast::ast::{BindingPattern, Expression, Statement, VariableDeclarationKind};
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        if decl.kind != VariableDeclarationKind::Let {
            continue;
        }
        for d in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            // A rune-call init (`$state` / `$derived` / `$props()`) was already
            // registered by the rune passes — skip it (never demote to plain local).
            if let Some(Expression::CallExpression(call)) = &d.init {
                if super::expr::state_rune_call(call).is_some()
                    || super::expr::is_derived_callee(&call.callee)
                    || super::expr::is_props_callee(&call.callee)
                {
                    continue;
                }
            }
            let name = id.name.as_str();
            // A name already declared in the scope (a `$state` / rune binding of the
            // same name) is never re-registered.
            if scopes.resolve(bindings, scope, name).is_some() {
                continue;
            }
            let binding = bindings.push(BindingInfo {
                name: name.to_string(),
                scope,
                kind: BindingRuntimeKind::PlainLocal,
                state: None,
            });
            scopes.declare(scope, name, binding);
        }
    }
}

/// One tracked LEGACY top-level plain `let` awaiting the demand-driven
/// `$.mutable_source` promotion decision: the root-scope binding row plus the
/// SCRIPT-side observed writes (template writes + bind-target write-backs are
/// attributed by [`finalize_legacy_let_promotions`] once the template scope
/// graph exists).
pub(super) struct TrackedLegacyLet {
    /// The SCRIPT-side observed WRITES (writes-only collection — a method call
    /// is never a write).
    script_uses: BindingUseSet,
    /// The root-scope `PlainLocal` binding row to promote.
    binding: BindingId,
}

/// Collect the LEGACY promotion-candidate rows: every top-level single-identifier
/// non-rune `let` declarator (the bindings [`prepare_plain_local_bindings`]
/// registered as `PlainLocal`), each with its SCRIPT-side WRITE uses observed in
/// WRITES-ONLY mode (assignment / update writes only — a mutating method call
/// like `arr.push(…)` is NOT a write: official keeps such a `let` verbatim-plain,
/// so it must never promote). Runs AFTER [`prepare_plain_local_bindings`] (it
/// resolves each candidate to its registered binding row). The promotion itself
/// is decided by [`finalize_legacy_let_promotions`] once the template writes and
/// bind-target write-backs are attributable.
pub(super) fn prepare_legacy_let_tracking(
    program: Option<&Program<'_>>,
    root_scope: ScopeId,
    scopes: &ScopeGraph,
    bindings: &BindingTable,
) -> Vec<TrackedLegacyLet> {
    let Some(program) = program else {
        return Vec::new();
    };
    use oxc_ast::ast::{BindingPattern, Expression, Statement, VariableDeclarationKind};
    // The candidate names: top-level `let` identifier declarators registered as
    // PlainLocal. A RUNE-call init (`$state` / `$derived` / `$props()`) is
    // EXCLUDED by init shape (the same predicate `prepare_plain_local_bindings`
    // uses) — a `$state` declarator's kind is only PROVISIONAL here (the
    // write-gated finalizer may flip a template-written one to a signal later),
    // so the kind check alone cannot exclude it.
    let mut names = Vec::new();
    let mut rows = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        if decl.kind != VariableDeclarationKind::Let {
            continue;
        }
        for d in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            if let Some(Expression::CallExpression(call)) = &d.init {
                if super::expr::state_rune_call(call).is_some()
                    || super::expr::is_derived_callee(&call.callee)
                    || super::expr::is_props_callee(&call.callee)
                {
                    continue;
                }
            }
            let name = id.name.as_str();
            let Some(binding) = scopes.resolve(bindings, root_scope, name) else {
                continue;
            };
            if bindings.get(binding).kind != super::expr::BindingRuntimeKind::PlainLocal {
                continue;
            }
            names.push(name.to_string());
            rows.push((name.to_string(), binding));
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }
    // The SCRIPT-side writes, in WRITES-ONLY mode (scope-aware — a shadowing
    // nested local of the same name never counts).
    let mut collector = super::expr::ScriptUseCollector::tracking_writes_only(&names);
    use oxc_ast_visit::Visit;
    collector.visit_program(program);
    rows.into_iter()
        .map(|(name, binding)| TrackedLegacyLet {
            script_uses: collector.use_set(&name),
            binding,
        })
        .collect()
}

/// Promote each WRITTEN tracked legacy `let` to the
/// [`BindingRuntimeKind::MutableSource`] kind — the demand-driven legacy
/// reactivity decision. A `let` is WRITTEN when its script-side writes-only
/// uses, its scope-resolved TEMPLATE writes (`Reassign` / `DeepMutate`
/// references), or a two-way `bind:` write-back (the SAME
/// [`attribute_bind_target_writes`] attribution the `$state` finalizer uses —
/// a bare-identifier bind target reassigns, a member target deep-mutates,
/// `bind:this` reassigns its element local) marks it. An unwritten `let` stays
/// `PlainLocal` (official static-folds its reads — no promotion, no blanket
/// accept). The caller gates this on the FINAL `SvelteMode::Legacy`; it is
/// never run for a runes component.
pub(super) fn finalize_legacy_let_promotions(ctx: &mut LoweringCtx, tracked: &[TrackedLegacyLet]) {
    if tracked.is_empty() {
        return;
    }
    let tracked_ids: rustc_hash::FxHashMap<BindingId, usize> = tracked
        .iter()
        .enumerate()
        .map(|(i, t)| (t.binding, i))
        .collect();
    let mut combined: Vec<BindingUseSet> = tracked.iter().map(|t| t.script_uses).collect();
    // TEMPLATE writes (handlers / interpolations / bind expressions), resolved
    // scope-awarely to the EXACT tracked binding.
    for expr in ctx.expressions.all() {
        for r in &expr.references {
            let deep = match r.kind {
                ExprRefKind::Reassign => false,
                ExprRefKind::DeepMutate => true,
                ExprRefKind::Read => continue,
            };
            let Some(resolved) = ctx.scopes.resolve(&ctx.bindings, expr.scope, &r.name) else {
                continue;
            };
            if let Some(&idx) = tracked_ids.get(&resolved) {
                if deep {
                    combined[idx].deep_mutated = true;
                } else {
                    combined[idx].reassigned = true;
                }
            }
        }
    }
    // Two-way `bind:` write-backs (the shared attribution the `$state`
    // finalizer uses).
    attribute_bind_target_writes(ctx, &tracked_ids, &mut combined);
    for (t, uses) in tracked.iter().zip(combined) {
        // Promote ONLY a binding still classified PlainLocal — a row whose kind a
        // later pass legitimately reclassified (defensive; the rune-init shapes
        // are already excluded at tracking) is never clobbered.
        let info = ctx.bindings.get_mut(t.binding);
        if info.kind == super::expr::BindingRuntimeKind::PlainLocal
            && (uses.reassigned || uses.deep_mutated)
        {
            info.kind = super::expr::BindingRuntimeKind::MutableSource;
        }
    }
}

/// Attribute scope-resolved TEMPLATE writes to the tracked `$state` bindings and
/// finalize each binding's classification.
///
/// Walks every analyzed template expression. For each WRITE reference (a
/// reassignment or a deep mutation), it resolves the referenced name through the
/// scope graph at the expression's own scope: only when it resolves to the EXACT
/// tracked `$state` binding (not a shadowing each / `{@const}` / nested local of
/// the same name) is the write merged into that binding's use-set. The final
/// `StateClassification` + binding kind are then recomputed from the combined
/// script + template uses.
pub(super) fn finalize_state_classifications(ctx: &mut LoweringCtx, tracked: &[TrackedState]) {
    if tracked.is_empty() {
        return;
    }
    // Index the tracked bindings by their root BindingId for O(1) resolution.
    let tracked_ids: rustc_hash::FxHashMap<BindingId, usize> = tracked
        .iter()
        .enumerate()
        .map(|(i, t)| (t.binding, i))
        .collect();

    // Start each binding's combined uses from its script-side observation.
    let mut combined: Vec<BindingUseSet> = tracked.iter().map(|t| t.script_uses).collect();

    for expr in ctx.expressions.all() {
        for r in &expr.references {
            let write = match r.kind {
                ExprRefKind::Reassign => Some(false),
                ExprRefKind::DeepMutate => Some(true),
                ExprRefKind::Read => None,
            };
            let Some(deep) = write else { continue };
            // Resolve the written name in the expression's own scope: only a write
            // that resolves to the EXACT tracked $state binding counts (a shadowing
            // local of the same name resolves elsewhere).
            let Some(resolved) = ctx.scopes.resolve(&ctx.bindings, expr.scope, &r.name) else {
                continue;
            };
            if let Some(&idx) = tracked_ids.get(&resolved) {
                if deep {
                    combined[idx].deep_mutated = true;
                } else {
                    combined[idx].reassigned = true;
                }
            }
        }
    }

    // A TWO-WAY `bind:` writes back to its bound target, so the bound `$state` is
    // observed as WRITTEN even though its expression is a syntactic READ — a
    // `bind:value={name}` makes `name` a reassigned signal (a bare-identifier
    // target is a reassignment; a member target `bind:value={o.x}` is a deep
    // mutation). This mirrors the official compiler treating a bind target as
    // mutated. The write attribution is scope-resolved, so a shadowing local is
    // never mis-attributed.
    attribute_bind_target_writes(ctx, &tracked_ids, &mut combined);

    for (t, uses) in tracked.iter().zip(combined) {
        let lowering = classify_state_lowering(t.declared, t.proxiable, uses);
        let info = ctx.bindings.get_mut(t.binding);
        info.kind = state_kind_for_lowering(lowering);
        info.state = Some(StateClassification {
            declared: t.declared,
            proxiable: t.proxiable,
            uses,
            lowering,
        });
    }
}

/// Attribute the WRITE-BACK of every two-way `bind:` directive to its bound
/// `$state` binding. Walks the IR nodes for an `AttrIr::Bind { target, expr }`
/// whose target is a two-way writable bind (anything except `this`, which is a
/// one-way element-ref write of the binding, also a reassignment), resolves the
/// bind expression's referenced binding scope-awarely, and marks it reassigned (a
/// bare-identifier target) or deep-mutated (a member target).
fn attribute_bind_target_writes(
    ctx: &LoweringCtx,
    tracked_ids: &rustc_hash::FxHashMap<BindingId, usize>,
    combined: &mut [BindingUseSet],
) {
    for node in &ctx.nodes {
        let attrs = match node {
            IrNode::Element(el) => &el.attrs,
            IrNode::Component(c) => &c.attrs,
            IrNode::Special(s) => &s.attrs,
            _ => continue,
        };
        for attr in attrs {
            let AttrIr::Bind {
                expr: Some(expr_id),
                ..
            } = attr
            else {
                continue;
            };
            let analyzed = ctx.expressions.get(*expr_id);
            // The bind expression's STRUCTURAL lvalue shape decides the write: a
            // bare-identifier target is a reassignment; a member target is a deep
            // mutation. Read the shared bind-target fact (classified once at analysis
            // time from the parsed OXC node, NOT a `source` text scan), so a member
            // access that is not the target root cannot mis-classify. A non-lvalue target
            // (a literal / call) carries no attributable write.
            let is_member = match analyzed.bind_target.kind {
                Some(super::expr::BindTargetKind::Member) => true,
                Some(super::expr::BindTargetKind::Identifier) => false,
                // A FUNCTION-PAIR target (`{get, set}`) does NOT make the bound binding
                // a reassignment/mutation OF the bind target — the user-supplied setter
                // contains the actual write, which the GENERAL template write-analysis
                // (the per-reference `Reassign`/`DeepMutate` loop above) already
                // attributes. Skip the bind-target-as-write attribution here.
                Some(super::expr::BindTargetKind::FunctionPair) => continue,
                // A non-lvalue bind target attributes no write.
                None => continue,
            };
            for r in &analyzed.references {
                let Some(resolved) = ctx.scopes.resolve(&ctx.bindings, analyzed.scope, &r.name)
                else {
                    continue;
                };
                if let Some(&idx) = tracked_ids.get(&resolved) {
                    if is_member {
                        combined[idx].deep_mutated = true;
                    } else {
                        combined[idx].reassigned = true;
                    }
                }
            }
        }
    }
}

/// Map a resolved `$state` lowering to its binding runtime kind.
fn state_kind_for_lowering(lowering: StateLowering) -> BindingRuntimeKind {
    match lowering {
        StateLowering::PlainLet => BindingRuntimeKind::PlainLocal,
        StateLowering::StateSignal => BindingRuntimeKind::StateSignal { raw: false },
        StateLowering::RawStateSignal => BindingRuntimeKind::StateSignal { raw: true },
        StateLowering::BareProxy => BindingRuntimeKind::BareProxy,
        StateLowering::StateProxy => BindingRuntimeKind::StateProxy,
    }
}
