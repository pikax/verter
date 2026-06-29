//! The narrow-plan PROJECTION of the control-flow blocks
//! (`{#if}`/`{#each}`/`{#await}`/`{#key}`) + the declaration / `{@const}` / `{@debug}` tags.
//!
//! Every block-head expression (the `{#if}` test, the `{#each}` source / key, the
//! `{#await}` promise, the `{#key}` expression) and every tag expression (a `{@const}`
//! initializer, a declaration-tag initializer, a `{@debug}` argument) is REWRITTEN here
//! through the shared FALLIBLE rewriter — driven by each expression's OWN recorded scope,
//! so a body-scope read becomes `$.get(item)` while a key-scope read of the same name
//! stays plain. An `await` expression / async rune inside any head fails closed at the
//! rewrite, BEFORE the plan exists. The child block-body regions are carried by their
//! [`TemplateScopeId`]; the emitter recurses into them.

use super::client_plan::SupportedClientIr;
use super::client_plan_types::{
    ClientAwait, ClientBlock, ClientDebugEntry, ClientDeclKeyword, ClientDeclaration, ClientEach,
    ClientEachKey, ClientIfBranch, ClientNode,
};
use super::expr::{BindingRuntimeKind, ExprRefKind};
use super::expr_rewrite::is_signal_kind;
use super::ir::{
    BindingId, BlockIr, DebugArg, DeclKind, ExprId, IfBranch, PatternId, SvelteMode,
    TemplateDeclarator, TemplateRune,
};
use super::unsupported::UnsupportedSvelteRuntimeSurface;

// The official `svelte@5.56.3` EACH flag bits (`src/compiler/.../constants.js`).
const EACH_ITEM_REACTIVE: u8 = 1;
const EACH_INDEX_REACTIVE: u8 = 2;
/// The official `EACH_IS_CONTROLLED` bit: the `{#each}` is the SOLE child of a regular
/// element and anchors on that element directly (no `<!>` marker in the cloned skeleton, no
/// `$.first_child`/`$.child` descent). Unlike the item/index/immutability bits — which are
/// PLAN facts — controlledness is a DOM-POSITION fact decided by the walk, so this bit is
/// OR'd onto the projected flags at emit time ([`super::client_block_emit`]), not here.
pub(super) const EACH_IS_CONTROLLED: u8 = 4;
const EACH_ITEM_IMMUTABLE: u8 = 16;

impl<'a> SupportedClientIr<'a> {
    /// Project a control-flow block (`{#if}`/`{#each}`/`{#await}`/`{#key}`) into its
    /// narrow [`ClientNode::Block`]. A `{#snippet}` is the component/snippet surface,
    /// refused upstream, and never reaches here.
    pub(super) fn project_block(
        &self,
        block: &BlockIr,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let client_block = match block {
            BlockIr::If { branches } => ClientBlock::If {
                branches: self.project_if_branches(branches)?,
            },
            BlockIr::Each {
                source,
                item,
                index,
                key,
                body,
                else_body,
            } => ClientBlock::Each(
                self.project_each(*source, *item, *index, *key, *body, *else_body)?,
            ),
            BlockIr::Await {
                promise,
                pending,
                then_binding,
                then_body,
                catch_binding,
                catch_body,
            } => ClientBlock::Await(ClientAwait {
                promise: self.rewrite(*promise, self.expr_scope(*promise))?,
                pending: *pending,
                then_param: self.pattern_single_name(*then_binding)?,
                then_body: *then_body,
                catch_param: self.pattern_single_name(*catch_binding)?,
                catch_body: *catch_body,
            }),
            BlockIr::Key { expr, body } => ClientBlock::Key {
                expr: self.rewrite(*expr, self.expr_scope(*expr))?,
                body: *body,
            },
            // A `{#snippet}` is the component/snippet surface — refused at the classifier,
            // never reaches the projection.
            BlockIr::Snippet { .. } => {
                return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "snippet",
                    span: verter_span::Span::new(0, 0),
                });
            }
        };
        Ok(ClientNode::Block(client_block))
    }

    /// Project the `{#if}` chain branches (the trailing `condition: None` branch is the
    /// `{:else}`). Each test is rewritten in its OWN recorded (parent) scope.
    fn project_if_branches(
        &self,
        branches: &[IfBranch],
    ) -> Result<Vec<ClientIfBranch>, UnsupportedSvelteRuntimeSurface> {
        branches
            .iter()
            .map(|branch| {
                let test = match branch.condition {
                    Some(cond) => Some(self.rewrite(cond, self.expr_scope(cond))?),
                    None => None,
                };
                Ok(ClientIfBranch {
                    test,
                    body: branch.body,
                })
            })
            .collect()
    }

    /// Project an `{#each}` block: compute the official flag bits (item / index
    /// reactivity + immutability — the `EACH_IS_CONTROLLED` bit is OR'd in by the emitter
    /// from the DOM-position context), the `uses_index` render-param decision, the source
    /// thunk, and the keyed key callback (rewritten PLAIN in the key scope).
    fn project_each(
        &self,
        source: ExprId,
        item: Option<PatternId>,
        index: Option<PatternId>,
        key: Option<ExprId>,
        body: super::ir::TemplateScopeId,
        else_body: Option<super::ir::TemplateScopeId>,
    ) -> Result<ClientEach, UnsupportedSvelteRuntimeSurface> {
        let item_binding = self.pattern_single_binding(item)?;
        let index_binding = self.pattern_single_binding(index)?;
        let item_param = item_binding.map(|b| self.binding_name(b));
        let user_index_name = index_binding.map(|b| self.binding_name(b));

        // The official flag bits (sans `EACH_IS_CONTROLLED`, OR'd in at emit). The item is
        // reactive unless its binding is inert; the index is reactive only for a keyed each
        // (its binding kind already encodes this — `EachSignal` keyed vs `PlainLocal`
        // unkeyed); runes mode is immutable.
        let mut flags = 0u8;
        if item_binding.is_some_and(|b| is_signal_kind(self.binding_kind(b))) {
            flags |= EACH_ITEM_REACTIVE;
        }
        if index_binding.is_some_and(|b| is_signal_kind(self.binding_kind(b))) {
            flags |= EACH_INDEX_REACTIVE;
        }
        if matches!(self.ir.component.mode, SvelteMode::Runes) {
            flags |= EACH_ITEM_IMMUTABLE;
        }

        // The official `uses_index` render-param rule: the index render param is emitted
        // when the index is READ in the body, OR the item is reassigned / mutated. A forced
        // (item-mutation) index with no user index uses the synthesized `$$index` name.
        let index_read = index_binding.is_some_and(|b| self.binding_is_referenced(b, false));
        let item_mutated = item_binding.is_some_and(|b| self.binding_is_referenced(b, true));
        let emit_index = index_read || item_mutated;
        let index_param = user_index_name
            .clone()
            .or_else(|| item_mutated.then(|| "$$index".to_string()));

        // The keyed key callback emits in its OWN scope (the key expr was lowered with
        // item / index as PLAIN locals), so the rewrite reads them plainly.
        let key = match key {
            Some(key_expr) => {
                let expr = self.rewrite(key_expr, self.expr_scope(key_expr))?;
                let key_uses_index = user_index_name.as_ref().is_some_and(|idx| {
                    self.ir
                        .analysis
                        .expressions
                        .get(key_expr)
                        .references
                        .iter()
                        .any(|r| &r.name == idx)
                });
                let mut params = Vec::new();
                if let Some(item_param) = &item_param {
                    params.push(item_param.clone());
                }
                if key_uses_index {
                    if let Some(idx) = &user_index_name {
                        params.push(idx.clone());
                    }
                }
                Some(ClientEachKey { params, expr })
            }
            None => None,
        };

        Ok(ClientEach {
            flags,
            source: self.rewrite(source, self.expr_scope(source))?,
            key,
            item_param,
            index_param,
            emit_index,
            body,
            else_body,
        })
    }

    /// Project a `{@const x = …}` tag into a block-local derived declaration
    /// (`const x = $.derived(() => …)`). A destructuring `{@const {a, b} = …}` (the
    /// `computed_const` form) is not yet supported — fail closed.
    pub(super) fn project_const_tag(
        &self,
        pattern: PatternId,
        init: ExprId,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let Some(name) = self.pattern_single_name(Some(pattern))? else {
            return Err(UnsupportedSvelteRuntimeSurface::Block {
                construct: "const",
                span: verter_span::Span::new(0, 0),
            });
        };
        let init = self.rewrite(init, self.expr_scope(init))?;
        Ok(ClientNode::Declarations {
            decls: vec![ClientDeclaration::Derived { name, init }],
        })
    }

    /// Project a `{const …}` / `{let …}` declaration tag into INERT block-local
    /// declarations (the initializer is signal-rewritten, but the binding is inert — read
    /// as the plain name, never `$.get`). A rune-carrying declarator is classified through
    /// the instance-script rune/state pipeline (its binding kind is a signal kind), and a
    /// destructuring declarator fails closed.
    pub(super) fn project_declaration_tag(
        &self,
        kind: DeclKind,
        declarators: &[TemplateDeclarator],
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let keyword = match kind {
            DeclKind::Const => ClientDeclKeyword::Const,
            DeclKind::Let => ClientDeclKeyword::Let,
        };
        let refuse = || UnsupportedSvelteRuntimeSurface::Block {
            construct: match kind {
                DeclKind::Const => "const",
                DeclKind::Let => "let",
            },
            span: verter_span::Span::new(0, 0),
        };
        let mut decls = Vec::with_capacity(declarators.len());
        for declarator in declarators {
            let Some(binding) = self.pattern_single_binding(Some(declarator.pattern))? else {
                return Err(refuse());
            };
            let name = self.binding_name(binding);
            match &declarator.rune {
                // A `{let x = $state(<primitive>)}` declarator: lower through the SHARED
                // `$state` wrapper, reading the lowering from THIS declarator's OWN binding
                // (by binding id — a same-name shadowed outer binding can never select the
                // wrapper), matching the instance-script form.
                Some(TemplateRune::State(rune_init)) => {
                    let lowering = self
                        .ir
                        .analysis
                        .bindings
                        .get(binding)
                        .state
                        .map(|s| s.lowering);
                    let code = super::expr_emit::state_primitive_decl(
                        &name,
                        rune_init.as_deref(),
                        lowering,
                    );
                    decls.push(ClientDeclaration::Rune { code });
                }
                // A `{let x = $derived(<arg>)}` declarator: a block-local derived memo
                // (`<keyword> x = $.derived(() => <rewritten arg>)`). The argument rides
                // `declarator.init`; reads of `x` are signals (`$.get(x)`).
                Some(TemplateRune::Derived) => {
                    let arg = declarator.init.ok_or_else(&refuse)?;
                    let body = self.rewrite(arg, self.expr_scope(arg))?;
                    let kw = match keyword {
                        ClientDeclKeyword::Const => "const",
                        ClientDeclKeyword::Let => "let",
                    };
                    decls.push(ClientDeclaration::Rune {
                        code: format!("{kw} {name} = $.derived(() => {body});"),
                    });
                }
                None => {
                    let init = match declarator.init {
                        Some(init) => Some(self.rewrite(init, self.expr_scope(init))?),
                        None => None,
                    };
                    decls.push(ClientDeclaration::Inert {
                        keyword,
                        name,
                        init,
                    });
                }
            }
        }
        Ok(ClientNode::Declarations { decls })
    }

    /// Project a `{@debug a, b}` tag into a reactive snapshot-logging effect. Each debug
    /// identifier becomes a `{ key: $.snapshot(<rewritten>) }` entry — the key is the PARSED
    /// identifier name (carried on [`DebugArg`] from the OXC-parsed `IdentifierReference`,
    /// NOT a re-sliced raw source span), and the snapshot argument is the rewritten read
    /// (`$.get(a)` for a signal, plain for a non-signal).
    pub(super) fn project_debug_tag(
        &self,
        args: &[DebugArg],
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let mut entries = Vec::with_capacity(args.len());
        for arg in args {
            let snapshot_arg = self.rewrite(arg.expr, self.expr_scope(arg.expr))?;
            entries.push(ClientDebugEntry {
                key: arg.name.clone(),
                snapshot_arg,
            });
        }
        Ok(ClientNode::Debug { entries })
    }

    /// The recorded (lexical) scope of a template expression — the scope the rewriter
    /// resolves the expression's free identifiers against. (The `rewrite` helper re-reads
    /// it from the analyzed expression, so this is passed only to satisfy the signature.)
    fn expr_scope(&self, expr: ExprId) -> super::expr::ScopeId {
        self.ir.analysis.expressions.get(expr).scope
    }

    /// The single declared binding of an optional pattern, or `None` for an absent
    /// pattern. A multi-name (destructuring) pattern fails closed.
    fn pattern_single_binding(
        &self,
        pattern: Option<PatternId>,
    ) -> Result<Option<BindingId>, UnsupportedSvelteRuntimeSurface> {
        let Some(pattern) = pattern else {
            return Ok(None);
        };
        match self.ir.pattern_bindings(pattern) {
            [binding] => Ok(Some(*binding)),
            _ => Err(UnsupportedSvelteRuntimeSurface::Block {
                construct: "destructuring-binding",
                span: verter_span::Span::new(0, 0),
            }),
        }
    }

    /// The single declared NAME of an optional pattern, or `None` for an absent pattern.
    fn pattern_single_name(
        &self,
        pattern: Option<PatternId>,
    ) -> Result<Option<String>, UnsupportedSvelteRuntimeSurface> {
        Ok(self
            .pattern_single_binding(pattern)?
            .map(|b| self.binding_name(b)))
    }

    /// The name of a binding.
    fn binding_name(&self, binding: BindingId) -> String {
        self.ir.analysis.bindings.get(binding).name.clone()
    }

    /// The runtime kind of a binding.
    fn binding_kind(&self, binding: BindingId) -> BindingRuntimeKind {
        self.ir.analysis.bindings.get(binding).kind
    }

    /// Whether `binding` is referenced anywhere in the template (scope-awarely — a
    /// shadowing local of the same name in an inner scope does NOT count). `writes_only`
    /// restricts the match to write references (reassign / deep-mutate).
    fn binding_is_referenced(&self, binding: BindingId, writes_only: bool) -> bool {
        for expr in self.ir.analysis.expressions.all() {
            for reference in &expr.references {
                if writes_only
                    && !matches!(
                        reference.kind,
                        ExprRefKind::Reassign | ExprRefKind::DeepMutate
                    )
                {
                    continue;
                }
                if self.ir.analysis.scopes.resolve(
                    &self.ir.analysis.bindings,
                    expr.scope,
                    &reference.name,
                ) == Some(binding)
                {
                    return true;
                }
            }
        }
        false
    }
}
