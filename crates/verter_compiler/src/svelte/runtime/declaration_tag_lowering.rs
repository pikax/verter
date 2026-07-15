//! Declaration-TAG lowering — the `{@const … = expr}` legacy block-local
//! derived and the INERT `{const …}` / `{let …}` declaration tags. Split from
//! the template-node lowering walk (`mod.rs`): both lower a parsed declarator
//! list into binding patterns + initializer expressions through the shared
//! [`LoweringCtx`], with no `=`-splitter text surgery (the OXC-parsed
//! declarator owns names + init spans).

use super::expr::{parse_declarators, BindingRuntimeKind, DeclaratorKeyword, ScopeId};
use super::ir::{DeclKind, IrNode, NodeId, TagIr, TemplateDeclarator, TemplateRune};
use super::state_prep;
use super::{span_text, LoweringCtx};
use crate::svelte::parser::template_ast::SvelteTag;
use verter_span::Span;

/// Lower a `{@const … = expr}` tag into a binding pattern + an initializer
/// expression. The pattern's names + the initializer span both come from the
/// OXC-parsed declarator (no top-level-`=` text splitter), so a destructuring
/// `{@const {a, b} = obj}` declares one binding per name, NOT one collapsed
/// binding.
pub(super) fn lower_at_const(
    ctx: &mut LoweringCtx,
    tag: &SvelteTag,
    scope: ScopeId,
) -> Option<NodeId> {
    let text = span_text(ctx.source, tag.inner);
    // `{@const}` always carries an initializer — wrap with `const`.
    let decls = match parse_declarators(text, DeclaratorKeyword::Const) {
        Ok(decls) => decls,
        Err(()) => {
            ctx.errors.push(
                "svelte-runtime-const-parse",
                format!("could not parse `{{@const}}` declaration `{text}`"),
                tag.span,
            );
            return None;
        }
    };
    // `{@const}` declares exactly one declarator with an initializer.
    let Some(decl) = decls.into_iter().next() else {
        ctx.errors.push(
            "svelte-runtime-const-empty",
            "`{@const}` requires a declarator".to_string(),
            tag.span,
        );
        return None;
    };
    let pattern =
        ctx.push_pattern_names(&decl.names, scope, BindingRuntimeKind::LegacyConstDerived);
    let Some((s, e)) = decl.init else {
        ctx.errors.push(
            "svelte-runtime-const-no-init",
            "`{@const}` requires an initializer".to_string(),
            tag.span,
        );
        return None;
    };
    let init_span = Span::new(tag.inner.start + s, tag.inner.start + e);
    let init = ctx.push_expr(init_span, scope);
    Some(ctx.push_node(IrNode::Tag(TagIr::LegacyConst { pattern, init })))
}

/// Lower a `{const …}` / `{let …}` declaration tag — INERT block-local
/// declarators (`TemplateDeclLocal`), DISTINCT from `{@const}`. Each declarator's
/// names + initializer span come from the OXC-parsed declaration (no `=`
/// splitter), and a destructuring declarator declares one binding per name.
pub(super) fn lower_declaration_tag(
    ctx: &mut LoweringCtx,
    tag: &SvelteTag,
    kind: DeclKind,
    scope: ScopeId,
) -> Option<NodeId> {
    let text = span_text(ctx.source, tag.inner);
    // A `{let …}` tag may have NO initializer (`{let x}`), which is invalid under
    // a `const` wrapper — wrap with the matching keyword so `{let x}` parses.
    let keyword = match kind {
        DeclKind::Const => DeclaratorKeyword::Const,
        DeclKind::Let => DeclaratorKeyword::Let,
    };
    let parsed = match parse_declarators(text, keyword) {
        Ok(decls) => decls,
        Err(()) => {
            ctx.errors.push(
                "svelte-runtime-decl-parse",
                format!("could not parse declaration tag `{text}`"),
                tag.span,
            );
            return None;
        }
    };
    let mut declarators = Vec::with_capacity(parsed.len());
    for decl in parsed {
        // Every declarator is first declared as an INERT block-local (`TemplateDeclLocal`);
        // a single-name `{let x = $state(<primitive>)}` / `{let x = $derived(<arg>)}` rune
        // declarator is then RECLASSIFIED through the shared rune/state pipeline (its binding
        // row is reclassified in place — `$state` write-gated + tracked for the finalizer,
        // `$derived` a `Derived` signal), so its template reads/writes route through the
        // signal rewriter; a rune the pipeline cannot lower stays inert and fails closed.
        let pattern =
            ctx.push_pattern_names(&decl.names, scope, BindingRuntimeKind::TemplateDeclLocal);
        let init_span = decl
            .init
            .map(|(s, e)| Span::new(tag.inner.start + s, tag.inner.start + e));
        let mut rune = None;
        let mut derived_arg = None;
        if decl.names.len() == 1 {
            if let Some(span) = init_span {
                let init_text = span_text(ctx.source, span).to_string();
                let binding = ctx.patterns[pattern.0 as usize].bindings[0];
                match state_prep::classify_block_rune_declarator(
                    binding,
                    &init_text,
                    &mut ctx.bindings,
                ) {
                    Some(state_prep::BlockRuneDeclarator::State { tracked, init }) => {
                        ctx.block_rune_tracking.push(tracked);
                        rune = Some(TemplateRune::State(init));
                    }
                    Some(state_prep::BlockRuneDeclarator::Derived { arg }) => {
                        // The `$derived` ARGUMENT expr — rewritten into the `$.derived(() =>
                        // …)` body at projection; carried on the declarator's `init` slot.
                        let arg_span = Span::new(span.start + arg.0, span.start + arg.1);
                        derived_arg = Some(ctx.push_expr(arg_span, scope));
                        rune = Some(TemplateRune::Derived);
                    }
                    None => {}
                }
            }
        }
        // A `$state` rune declarator carries NO init expr (its primitive text rides the
        // `TemplateRune::State`); a `$derived` declarator carries its rewritable argument;
        // an inert declarator carries its plain initializer.
        let init = match rune {
            Some(TemplateRune::State(_)) => None,
            Some(TemplateRune::Derived) => derived_arg,
            None => init_span.map(|span| ctx.push_expr(span, scope)),
        };
        declarators.push(TemplateDeclarator {
            pattern,
            init,
            rune,
        });
    }
    Some(ctx.push_node(IrNode::Tag(TagIr::Declaration { kind, declarators })))
}
