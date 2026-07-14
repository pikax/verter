//! Template block-lowering: the `{#if}` / `{#each}` / `{#await}` / `{#key}` /
//! `{#snippet}` block-construct lowering family. A cohesive sibling of the runtime
//! lowering module — these functions are called ONLY from within the runtime
//! lowering (the block arm of `super::lower_node`), and reach the shared
//! `LoweringCtx` / IR / scope helpers (including `super::lower_node` and
//! `super::lower_children_in_scope`) through the parent module.

use super::*;

/// Lower a block construct into the IR, creating its body template scopes.
pub(super) fn lower_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    scope: ScopeId,
) -> Option<NodeId> {
    match &block.kind {
        SvelteBlockKind::If => Some(lower_if_block(ctx, block, scope)),
        SvelteBlockKind::Each { item, index, key } => {
            Some(lower_each_block(ctx, block, *item, *index, *key, scope))
        }
        SvelteBlockKind::Await {
            then_binding,
            catch_binding,
        } => Some(lower_await_block(
            ctx,
            block,
            *then_binding,
            *catch_binding,
            scope,
        )),
        SvelteBlockKind::Key => Some(lower_key_block(ctx, block, scope)),
        SvelteBlockKind::Snippet {
            name,
            name_text,
            params,
        } => Some(lower_snippet_block(
            ctx, block, *name, name_text, *params, scope,
        )),
    }
}

/// Lower an `{#if}` chain into branches (the primary branch + `{:else if}` /
/// `{:else}` clauses).
fn lower_if_block(ctx: &mut LoweringCtx, block: &SvelteBlock, scope: ScopeId) -> NodeId {
    let mut branches = Vec::new();
    // The primary `{#if expr}` branch.
    let condition = block.head_expr.map(|s| ctx.push_expr(s, scope));
    let body = lower_branch_body(ctx, &block.children, scope);
    branches.push(IfBranch { condition, body });
    // The `{:else if}` / `{:else}` clauses.
    for clause in &block.clauses {
        let condition = match clause.kind {
            SvelteClauseKind::ElseIf => clause.expr.map(|s| ctx.push_expr(s, scope)),
            SvelteClauseKind::Else => None,
            // `{:then}` / `{:catch}` never appear on an `{#if}` — defensive skip.
            SvelteClauseKind::Then | SvelteClauseKind::Catch => continue,
        };
        let body = lower_branch_body(ctx, &clause.children, scope);
        branches.push(IfBranch { condition, body });
    }
    ctx.push_node(IrNode::Block(BlockIr::If { branches }))
}

/// Lower a run of children into a fresh template scope under `parent_scope`.
fn lower_branch_body(
    ctx: &mut LoweringCtx,
    children: &[SvelteNode],
    parent_scope: ScopeId,
) -> TemplateScopeId {
    let body_scope = ctx.scopes.push_scope(Some(parent_scope));
    let ts = ctx.push_template_scope(body_scope);
    let mut roots = Vec::new();
    for child in children {
        if let Some(id) = lower_node(ctx, child, body_scope) {
            roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = roots;
    ts
}

/// Lower an `{#each}` block. The ITEM binding is a SIGNAL read (`EachSignal`),
/// declared in the body scope so a same-name outer signal is shadowed. The INDEX
/// binding is a signal ONLY for a KEYED each (where items reorder, so an item's
/// index can change — official sets `EACH_INDEX_REACTIVE` and reads `$.get(i)`);
/// for an UNKEYED each the index is positional and INERT (`PlainLocal`, read as
/// the plain callback parameter `i`, NOT `$.get(i)`), matching the official
/// `flags |= EACH_INDEX_REACTIVE` gate (`keyed && index`).
fn lower_each_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    item: Option<Span>,
    index: Option<Span>,
    key: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    let source = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));
    // The body scope binds the item as a signal; the index is reactive ONLY when
    // the each is keyed (the official `keyed && index` reactivity gate).
    let body_scope = ctx.scopes.push_scope(Some(scope));
    let item_pat = item.map(|s| ctx.push_pattern(s, body_scope, BindingRuntimeKind::EachSignal));
    let index_kind = if key.is_some() {
        BindingRuntimeKind::EachSignal
    } else {
        BindingRuntimeKind::PlainLocal
    };
    let index_pat = index.map(|s| ctx.push_pattern(s, body_scope, index_kind));
    // The KEY expression of a keyed each is rewritten in its OWN callback scope: the
    // item / index are PLAIN callback params there (`(item) => item.id` — read plainly,
    // shadowing any same-name OUTER signal), DISTINCT from the body scope where the item
    // is a signal. This mirrors the official `key_state`, which deletes the item's signal
    // transform so the key reads it directly.
    let key_expr = key.map(|s| {
        let key_scope = ctx.scopes.push_scope(Some(scope));
        if let Some(item_span) = item {
            ctx.push_pattern(item_span, key_scope, BindingRuntimeKind::PlainLocal);
        }
        if let Some(index_span) = index {
            ctx.push_pattern(index_span, key_scope, BindingRuntimeKind::PlainLocal);
        }
        ctx.push_expr(s, key_scope)
    });
    let ts = ctx.push_template_scope(body_scope);
    let mut roots = Vec::new();
    for child in &block.children {
        if let Some(id) = lower_node(ctx, child, body_scope) {
            roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = roots;
    // An `{:else}` clause on the each block.
    let else_body = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Else)
        .map(|c| lower_branch_body(ctx, &c.children, scope));
    ctx.push_node(IrNode::Block(BlockIr::Each {
        source,
        item: item_pat,
        index: index_pat,
        key: key_expr,
        body: ts,
        else_body,
    }))
}

/// Lower an `{#await}` block. The `{:then x}` / `{:catch e}` bindings are SIGNAL
/// reads (`AwaitSignal`), declared in their branch scope.
fn lower_await_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    then_binding: Option<Span>,
    catch_binding: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    let promise = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));

    // The block's IMMEDIATE children belong to exactly ONE role, decided by the
    // form. The parser promotes a `{:then v}` clause's binding onto the block
    // kind's `then_binding`, so for the canonical CLAUSE form
    // (`{#await p}<pending>{:then v}<then>{:catch e}<catch>{/await}`) BOTH a
    // `then_binding` span AND a `Then` clause are present — the clause list, NOT
    // the inline binding span, decides the form:
    //
    // - ANY `{:then}`/`{:catch}` clause present  ⇒ CLAUSE form: immediate children
    //   are the PENDING body; each clause owns its branch children + binding.
    // - else inline then (`{#await p then v}`)    ⇒ children are the THEN body,
    //   no pending branch.
    // - else inline catch (`{#await p catch e}`)  ⇒ children are the CATCH body,
    //   no pending branch.
    // - else (`{#await p}<x>{/await}`)            ⇒ children are the PENDING body.
    let then_clause = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Then);
    let catch_clause = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Catch);
    let has_branch_clause = then_clause.is_some() || catch_clause.is_some();

    // Resolve the role of the immediate children, plus the inline branch bodies.
    let mut pending = None;
    let mut then_binding_pat = None;
    let mut then_body = None;
    let mut catch_binding_pat = None;
    let mut catch_body = None;

    if has_branch_clause {
        // CLAUSE form: immediate children are the pending body.
        pending = Some(lower_branch_body(ctx, &block.children, scope));
    } else if let Some(then_span) = then_binding {
        // Inline then: the immediate children ARE the then body (no pending).
        let then_scope = ctx.scopes.push_scope(Some(scope));
        let p = ctx.push_pattern(then_span, then_scope, BindingRuntimeKind::AwaitSignal);
        then_binding_pat = Some(p);
        then_body = Some(lower_children_in_scope(ctx, &block.children, then_scope));
    } else if let Some(catch_span) = catch_binding {
        // Inline catch: the immediate children ARE the catch body (no pending).
        let catch_scope = ctx.scopes.push_scope(Some(scope));
        let p = ctx.push_pattern(catch_span, catch_scope, BindingRuntimeKind::AwaitSignal);
        catch_binding_pat = Some(p);
        catch_body = Some(lower_children_in_scope(ctx, &block.children, catch_scope));
    } else {
        // Plain `{#await p}<x>{/await}`: the immediate children are the pending body.
        pending = Some(lower_branch_body(ctx, &block.children, scope));
    }

    // The `{:then}` clause (CLAUSE form) owns its own children + binding.
    if let Some(then_clause) = then_clause {
        let then_scope = ctx.scopes.push_scope(Some(scope));
        then_binding_pat = then_clause
            .expr
            .map(|s| ctx.push_pattern(s, then_scope, BindingRuntimeKind::AwaitSignal));
        then_body = Some(lower_children_in_scope(
            ctx,
            &then_clause.children,
            then_scope,
        ));
    }

    // The `{:catch}` clause (CLAUSE form) owns its own children + binding.
    if let Some(catch_clause) = catch_clause {
        let catch_scope = ctx.scopes.push_scope(Some(scope));
        catch_binding_pat = catch_clause
            .expr
            .map(|s| ctx.push_pattern(s, catch_scope, BindingRuntimeKind::AwaitSignal));
        catch_body = Some(lower_children_in_scope(
            ctx,
            &catch_clause.children,
            catch_scope,
        ));
    }

    ctx.push_node(IrNode::Block(BlockIr::Await {
        promise,
        pending,
        then_binding: then_binding_pat,
        then_body,
        catch_binding: catch_binding_pat,
        catch_body,
    }))
}

/// Lower a `{#key}` block.
fn lower_key_block(ctx: &mut LoweringCtx, block: &SvelteBlock, scope: ScopeId) -> NodeId {
    let expr = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));
    let body = lower_branch_body(ctx, &block.children, scope);
    ctx.push_node(IrNode::Block(BlockIr::Key { expr, body }))
}

/// Lower a `{#snippet}` block. The snippet name is a binding in the ENCLOSING
/// scope; its params are INERT (`SnippetParam`) locals in the body scope.
fn lower_snippet_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    _name_span: Span,
    name_text: &str,
    params: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    // The snippet name binds in the enclosing scope (callable by siblings via
    // `{@render name(...)}`). It is an authored template declaration, so it reserves
    // the component-function name (svelte deconflicts `{#snippet Foo}` + name `Foo`
    // to `Foo_1`).
    let name_binding = ctx.bindings.push(BindingInfo {
        name: name_text.to_string(),
        scope,
        kind: BindingRuntimeKind::SnippetName,
        state: None,
    });
    ctx.scopes.declare(scope, name_text, name_binding);
    ctx.template_declarations.insert(name_text.to_string());

    let body_scope = ctx.scopes.push_scope(Some(scope));
    let mut param_pats = Vec::new();
    if let Some(params_span) = params {
        let p = ctx.push_pattern(params_span, body_scope, BindingRuntimeKind::SnippetParam);
        param_pats.push(p);
    }
    let ts = lower_children_in_scope(ctx, &block.children, body_scope);
    // Record the lowered SOURCE-LEVEL DIRECT children of the snippet body: the
    // unified slot choke-point accepts a STATIC `slot=` on these hosts (official
    // validates a snippet direct child as component-owned placement) while rejecting
    // the dynamic/mixed forms. Populated at the SNIPPET call site — never inside
    // `lower_children_in_scope`, which `{#await}` bodies share (their roots are NOT
    // snippet children).
    let snippet_roots: Vec<NodeId> = ctx.template_scopes[ts.0 as usize].roots.clone();
    ctx.direct_snippet_slot_attr_child_hosts
        .extend(snippet_roots);
    ctx.push_node(IrNode::Block(BlockIr::Snippet {
        name: name_binding,
        params: param_pats,
        body: ts,
    }))
}
