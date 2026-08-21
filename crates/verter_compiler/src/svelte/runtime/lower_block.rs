//! Template block-lowering: the `{#if}` / `{#each}` / `{#await}` / `{#key}` /
//! `{#snippet}` block-construct lowering family. A cohesive sibling of the runtime
//! lowering module — these functions are called ONLY from within the runtime
//! lowering (the block arm of `super::lower_node`), and reach the shared
//! `LoweringCtx` / IR / scope helpers (including `super::lower_node` and
//! `super::lower_children_in_scope`) through the parent module.

use super::ir::PatternShape;
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
            ..
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
        // An unrecognised `{#keyword}` block lowers exactly as the former
        // untyped fallback did: an expression-less key block over its body
        // (behaviour-preserving; official rejects it at parse).
        SvelteBlockKind::Unknown { .. } => Some(lower_key_block(ctx, block, scope)),
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

/// The inputs the official item-reactivity rule reads, gathered so the LOWERING
/// finalizer and the client BLOCK PLAN decide it from one implementation.
///
/// The two must not diverge: the flag the plan projects
/// (`EACH_ITEM_REACTIVE`) and the READ FORM the item binding lowers to are two
/// halves of one decision. With the flag clear the runtime hands the render
/// callback the raw item, so a `$.get(item)` read would dereference a
/// non-signal; with it set the callback receives a signal, so a plain read
/// would yield the signal object.
pub(super) struct EachReactivityFacts<'a, 'b> {
    pub(super) expressions: &'a ExprArena<'b>,
    pub(super) bindings: &'a BindingTable,
    pub(super) scopes: &'a ScopeGraph,
    pub(super) patterns: &'a [PatternBindings],
    /// Whether the component compiles in runes mode.
    pub(super) runes: bool,
}

impl EachReactivityFacts<'_, '_> {
    /// The official `EACH_ITEM_REACTIVE` rule.
    ///
    /// The pinned official compiler walks the COLLECTION expression's resolved
    /// dependencies and sets the bit for the first one that escapes the block's
    /// scope, but only when the block is NON-runes, or the key is not the item
    /// itself, or the collection subscribes a store. A collection with no
    /// resolved dependency at all (a literal, a global call) leaves the bit
    /// clear because the loop body never runs.
    ///
    /// Official additionally SKIPS a dependency whose binding does not escape
    /// the each block's own scope — an expression-local one, e.g. the arrow
    /// parameter in `{#each ((x) => [x])(1) as item}`, which official records
    /// as a dependency and then skips by function depth. That filter is not
    /// re-implemented here because the reference set it would filter never
    /// arrives: expression-local parameters and declarations are removed from
    /// an analyzed expression's stored `references` before resolution, so only
    /// bindings captured from OUTSIDE the expression reach this loop. Those are
    /// declared outside the block by construction — the collection is lowered
    /// in the each's PARENT scope — so none of them would be skipped.
    pub(super) fn each_item_is_reactive(
        &self,
        source: ExprId,
        item: Option<PatternId>,
        key: Option<ExprId>,
    ) -> bool {
        let source_expr = self.expressions.get(source);
        let mut has_dependency = false;
        let mut uses_store = false;
        for reference in &source_expr.references {
            // A free identifier resolving to NO declared binding is a global;
            // official's dependency set carries bindings only.
            let Some(kind) =
                self.bindings
                    .resolve_kind(self.scopes, source_expr.scope, &reference.name)
            else {
                continue;
            };
            has_dependency = true;
            if matches!(kind, BindingRuntimeKind::StoreSubscription) {
                uses_store = true;
            }
        }
        if !has_dependency {
            return false;
        }
        !self.runes || uses_store || !self.key_is_the_item(item, key)
    }

    /// Whether the each's COLLECTION expression subscribes a `$store` —
    /// exposed independently of [`Self::each_item_is_reactive`] for the
    /// official `EACH_ITEM_IMMUTABLE` rule (`runes && !uses_store`).
    pub(super) fn collection_uses_store(&self, source: ExprId) -> bool {
        let source_expr = self.expressions.get(source);
        source_expr.references.iter().any(|reference| {
            matches!(
                self.bindings
                    .resolve_kind(self.scopes, source_expr.scope, &reference.name),
                Some(BindingRuntimeKind::StoreSubscription)
            )
        })
    }

    /// The official `key_is_item` predicate: the each CONTEXT is a bare
    /// identifier pattern, the KEY expression is a bare identifier, and the two
    /// name the same binding.
    ///
    /// Both halves read typed facts — the pattern producer's observed
    /// [`PatternShape`] and the key expression's own parsed root — never the
    /// authored text. A single-name DESTRUCTURE (`{#each items as { id } (id)}`)
    /// is not an identifier context and is `false`, exactly as official's
    /// `node.context?.type === 'Identifier'` check decides it.
    ///
    /// The key is compared AFTER TypeScript-only wrappers are peeled, because
    /// official runs this check on a TS-ERASED tree: `(item!)` and
    /// `(item as T)` are the identifier `item` by the time official reaches the
    /// transform, so they must be `key_is_item` here too.
    fn key_is_the_item(&self, item: Option<PatternId>, key: Option<ExprId>) -> bool {
        self.key_names_the_pattern(item, key)
    }

    /// The official `key_is_the_index`-shaped predicate: the each's INDEX
    /// context is a bare identifier pattern, the KEY expression is a bare
    /// identifier, and the two name the same binding — the each's OWN index,
    /// keyed by itself. Symmetric to [`Self::key_is_the_item`], over the
    /// index pattern instead of the item pattern.
    ///
    /// A key that IS the index makes the each UNKEYED for official
    /// (`metadata.keyed = false`): the custom key callback is dropped in
    /// favour of the plain `$.index` selector, `EACH_INDEX_REACTIVE` clears,
    /// and the index reads plainly rather than through `$.get`. Both the
    /// client-plan keyedness decision and this binding-kind finalizer read
    /// this SAME predicate, so the two never disagree.
    pub(super) fn key_is_the_index(&self, index: Option<PatternId>, key: Option<ExprId>) -> bool {
        self.key_names_the_pattern(index, key)
    }

    /// Shared core: `pattern` is a bare-identifier context whose sole binding
    /// is the SAME name as `key`'s (TS-erased) bare-identifier root.
    fn key_names_the_pattern(&self, pattern: Option<PatternId>, key: Option<ExprId>) -> bool {
        let (Some(pattern), Some(key)) = (pattern, key) else {
            return false;
        };
        let pattern = &self.patterns[pattern.0 as usize];
        if pattern.shape != PatternShape::BareIdentifier {
            return false;
        }
        let [binding] = pattern.bindings.as_slice() else {
            return false;
        };
        let name = self.bindings.get(*binding).name.as_str();
        let key_expr = self.expressions.get(key);
        let Some(key_root) = key_expr.parsed_expression() else {
            return false;
        };
        super::expr::expr_wrapped_ident(key_root).is_some_and(|key_name| key_name == name)
    }
}

/// Demote every `{#each}` ITEM binding the official item-reactivity rule leaves
/// non-reactive from `EachSignal` to `PlainLocal`, so its reads lower PLAINLY.
///
/// Runs once the scope graph is complete and the component's final mode is
/// known, because the rule reads both. Keeping the binding kind and the
/// projected `EACH_ITEM_REACTIVE` flag on the same predicate is what stops the
/// two from disagreeing — a keyed runes each whose key IS its item gets flag
/// `20` AND a plain `item` read, which is exactly what the official compiler
/// emits for it.
pub(super) fn finalize_each_item_reactivity(ctx: &mut LoweringCtx, runes: bool) {
    let mut demote: Vec<BindingId> = Vec::new();
    {
        let facts = EachReactivityFacts {
            expressions: &ctx.expressions,
            bindings: &ctx.bindings,
            scopes: &ctx.scopes,
            patterns: &ctx.patterns,
            runes,
        };
        for node in &ctx.nodes {
            let IrNode::Block(BlockIr::Each {
                source, item, key, ..
            }) = node
            else {
                continue;
            };
            if facts.each_item_is_reactive(*source, *item, *key) {
                continue;
            }
            let Some(item) = item else {
                continue;
            };
            demote.extend(ctx.patterns[item.0 as usize].bindings.iter().copied());
        }
    }
    for binding in demote {
        let info = ctx.bindings.get_mut(binding);
        if info.kind == BindingRuntimeKind::EachSignal {
            info.kind = BindingRuntimeKind::EachPlain;
        }
    }
}

/// Demote every `{#each}` INDEX binding whose KEY is the index ITSELF from
/// `EachSignal` to `PlainLocal`: official makes such an each UNKEYED
/// (`metadata.keyed = false`), so the index reads plainly rather than through
/// `$.get`. The client plan reads the SAME [`EachReactivityFacts::key_is_the_index`]
/// predicate to decide the projected keyedness (`ClientEach::key` / the
/// `EACH_INDEX_REACTIVE` bit), so the two never disagree.
pub(super) fn finalize_each_index_reactivity(ctx: &mut LoweringCtx) {
    let mut demote: Vec<BindingId> = Vec::new();
    {
        let facts = EachReactivityFacts {
            expressions: &ctx.expressions,
            bindings: &ctx.bindings,
            scopes: &ctx.scopes,
            patterns: &ctx.patterns,
            runes: false,
        };
        for node in &ctx.nodes {
            let IrNode::Block(BlockIr::Each { index, key, .. }) = node else {
                continue;
            };
            if !facts.key_is_the_index(*index, *key) {
                continue;
            }
            let Some(index) = index else {
                continue;
            };
            demote.extend(ctx.patterns[index.0 as usize].bindings.iter().copied());
        }
    }
    for binding in demote {
        let info = ctx.bindings.get_mut(binding);
        if info.kind == BindingRuntimeKind::EachSignal {
            info.kind = BindingRuntimeKind::EachPlain;
        }
    }
}

/// Lower an `{#each}` block. The ITEM binding is declared as a SIGNAL read
/// (`EachSignal`) in the body scope so a same-name outer signal is shadowed;
/// [`finalize_each_item_reactivity`] demotes it to `PlainLocal` once the final
/// mode and full scope graph make the official item-reactivity rule decidable.
/// The INDEX binding is a signal ONLY for a KEYED each (where items reorder, so
/// an item's index can change — official sets `EACH_INDEX_REACTIVE` and reads
/// `$.get(i)`); for an UNKEYED each the index is positional and INERT
/// (`PlainLocal`, read as the plain callback parameter `i`, NOT `$.get(i)`),
/// matching the official `flags |= EACH_INDEX_REACTIVE` gate (`keyed && index`).
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
    // the each is keyed (the official `keyed && index` reactivity gate). A
    // SHORTHAND-SINGLE-PROPERTY item context (`{ id }`) binds its ONE declared
    // field as a per-field getter-thunk read instead — official routes the raw
    // item through a synthesized `$$item` param and reads the field through its
    // own `() => $.get($$item).<field>` thunk. Every OTHER decomposition
    // (`{ id: foo }`, `[id]`, `{ ...rest }`) also lands `EachDestructuredField`
    // here — inert, because the client plan (`project_each`) refuses those
    // shapes before ever projecting a read of the binding; only the SHORTHAND
    // shape's field binding is actually exercised.
    let body_scope = ctx.scopes.push_scope(Some(scope));
    let item_pat = item.map(|s| {
        ctx.push_pattern_by_shape(s, body_scope, |shape| {
            if shape == ir::PatternShape::BareIdentifier {
                BindingRuntimeKind::EachSignal
            } else {
                BindingRuntimeKind::EachDestructuredField
            }
        })
    });
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
