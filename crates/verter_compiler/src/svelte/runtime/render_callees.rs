//! The post-lowering `{@render}` CALLEE-RESOLUTION pass.
//!
//! Template lowering interns every `{@render callee(args)}` tag as a
//! provisional [`RenderCallee::Dynamic`] node and records a pending row; once
//! the FULL scope graph exists (a forward-referenced `{#snippet}` declared
//! later in the same scope must resolve), this pass reads each pending render
//! expression's stored `render_callee` fact — classified ONCE by the same
//! parse that interned the provisional expression (no second reparse) — and
//! finalizes the callee: a static name resolving to a `{#snippet}` NAME
//! binding becomes [`RenderCallee::Snippet`]; everything else stays the
//! provisional dynamic callee. A spread argument marks the node for the
//! client-surface fail-closed gate (the official
//! `render_tag_invalid_spread_argument` hard error) — never a silent arg drop.

use verter_span::Span;

use super::expr::{BindingRuntimeKind, RenderCalleeShape};
use super::ir::{ExprId, IrNode, RenderCallee, TagIr};
use super::{span_text, LoweringCtx};

/// Resolve every pending `{@render}` callee now that the full scope graph exists.
///
/// A static-name call (`row(1)`) whose callee resolves to a `{#snippet}` NAME
/// binding becomes [`RenderCallee::Snippet`] with the parsed argument
/// expressions; an optional call (`getSnippet()?.()`), a non-identifier callee,
/// or an unresolved name stays [`RenderCallee::Dynamic`] (the whole inner
/// expression).
pub(super) fn resolve_render_callees(ctx: &mut LoweringCtx) {
    let pending = std::mem::take(&mut ctx.pending_renders);
    for render in pending {
        let node = render.node;
        let shape = match ctx.expressions.get(render.expr).render_callee.clone() {
            Ok(shape) => shape,
            Err(()) => {
                // The provisional expression was TORN — surface the render-tag
                // diagnostic exactly as the old reparse failure did.
                let text = span_text(ctx.source, render.inner);
                ctx.errors.push(
                    "svelte-runtime-render-parse",
                    format!("could not parse `{{@render}}` expression `{text}`"),
                    render.inner,
                );
                continue;
            }
        };
        // Both call shapes carry the trailing call's argument spans; the callee
        // discriminant (a `{#snippet}` NAME vs the dynamic prop/member callee) is the
        // only difference. Build the argument ExprIds ONCE so EVERY callee keeps its
        // argument thunks (the `$.snippet(node, callee, …args)` shape) — not just the
        // static snippet-name callee.
        let (static_name, arg_spans) = match shape {
            RenderCalleeShape::StaticName {
                name,
                optional,
                args,
            } => (Some((name, optional)), args),
            RenderCalleeShape::Dynamic { args } => (None, args),
            // A SPREAD argument is an official HARD ERROR
            // (`render_tag_invalid_spread_argument`). Mark the node with the tag span;
            // the client-surface gate fails it closed (the callee/args stay provisional
            // — they are never emitted). NEVER the silent arg-drop the empty-args path
            // produced.
            RenderCalleeShape::SpreadArguments => {
                if let IrNode::Tag(TagIr::Render {
                    spread_arg_span, ..
                }) = &mut ctx.nodes[node.0 as usize]
                {
                    *spread_arg_span = Some(render.inner);
                }
                continue;
            }
        };
        let arg_ids: Vec<ExprId> = arg_spans
            .into_iter()
            .map(|(s, e)| {
                let span = Span::new(render.inner.start + s, render.inner.start + e);
                ctx.push_expr(span, render.scope)
            })
            .collect();
        // A static-name callee is a snippet call ONLY when it resolves to a
        // `{#snippet}` NAME binding in scope (the `optional` flag rides along — a
        // resolved `row?.(…)` emits the direct optional call); a prop /
        // dynamic-snippet-value callee stays the provisional `Dynamic(inner)` node.
        let snippet_binding = static_name.and_then(|(name, optional)| {
            let binding = ctx.scopes.resolve(&ctx.bindings, render.scope, &name)?;
            (ctx.bindings.get(binding).kind == BindingRuntimeKind::SnippetName)
                .then_some((binding, optional))
        });
        if let IrNode::Tag(TagIr::Render { callee, args, .. }) = &mut ctx.nodes[node.0 as usize] {
            if let Some((binding, optional)) = snippet_binding {
                *callee = RenderCallee::Snippet { binding, optional };
            }
            // Otherwise the callee stays the provisional `Dynamic(inner)` — correct
            // for a prop / member / call-expression / ternary callee.
            *args = arg_ids;
        }
    }
}
