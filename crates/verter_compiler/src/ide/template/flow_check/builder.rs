//! Source-backed [`CheckPlan`] construction from a real parsed template.
//!
//! Every input comes from an existing owner: chains and their member order
//! from the parser's `v_if_chains`, lexical scope identity from
//! `OxcParsedAst::scopes` handles, accessor prefixes from the shared
//! `BindingResolver` through `build_prefixed_expr_segments`, and callback
//! shapes and outer references from the OXC expression ASTs the template
//! parse already produced. Contextual contracts are supplied by the caller
//! per authored directive (the production emitter derives them from the
//! component/element typing it already performs).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, ChainElement, Expression, FormalParameters, Function,
    FunctionBody, Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashMap;

use super::seam::{
    Branch, BranchKey, BranchKind, Callback, CallbackFunction, Chain, CheckPlan, ExprItem, Frame,
    FrameBinding, GuardSite, Item, OuterRef, Piece, ResolvedExpr, ScopeKey, Span,
};
use crate::ast::types::{
    AstNodeKind, ConditionalChain, ElementNode, ElementNodeConditionKind, TemplateAst,
};
use crate::ide::get_directive_name;
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::expression::build_prefixed_expr_segments;
use crate::template::code_gen::types::MappedGeneratedText;
use crate::template::oxc::scope::LexicalScopeId;
use crate::template::oxc::types::{
    OxcNodeData, OxcParsedAst, OxcParsedElement, OxcParsedExpression,
};
use crate::types::{NodeId, NodeProp};

/// The caller-supplied typing context of one template.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    /// Declarations the check is typed against.
    pub declarations: &'a str,
    /// Parameter list of the generated check function.
    pub parameters: &'a str,
    /// Template binding kinds, as the script analysis reports them.
    pub bindings: &'a [(&'a str, BindingType)],
    /// Contextual contract per authored directive key (`@click`, `:on-pick`,
    /// `#row`, `v-slot`). Every callback and slot frame needs one.
    pub contracts: &'a [(&'a str, &'a str)],
}

/// Work the builder performed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildWork {
    /// Authored conditions resolved (each exactly once).
    pub conditions_resolved: u64,
    /// Scope handles read from the template parse.
    pub scope_handles: u64,
    /// Callbacks planned.
    pub callbacks: u64,
}

/// A template the builder cannot plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    NoTemplate,
    MissingContract { key: String },
    UnparsedExpression { at: u32 },
    MultiStatementHandler { at: u32 },
    UnexpectedScopeChain { at: u32 },
    ResolvedTextDiverges { at: u32 },
}

/// Parse `source` (an SFC with a `<template>` block) and plan its check.
pub fn build_plan(
    source: &str,
    context: &PlanContext<'_>,
) -> Result<(CheckPlan, BuildWork), BuildError> {
    let template_ast = parse_template(source).ok_or(BuildError::NoTemplate)?;
    let allocator = Allocator::default();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &allocator,
        oxc_span::SourceType::tsx(),
        true,
    );
    let bindings: FxHashMap<&str, BindingType> = context.bindings.iter().copied().collect();
    let mut resolver = BindingResolver::new(bindings, true);
    resolver.set_tsx(true);

    let mut builder = Builder {
        source,
        ast: &template_ast,
        oxc: &oxc_ast,
        resolver: &resolver,
        contracts: context.contracts,
        keys: FxHashMap::default(),
        work: BuildWork::default(),
    };
    let root = builder.key(LexicalScopeId::ROOT);
    let items = match &template_ast.root.content {
        Some(content) => builder.children(
            &content.children,
            &content.v_if_chains,
            LexicalScopeId::ROOT,
        )?,
        None => Vec::new(),
    };
    let work = builder.work;
    Ok((
        CheckPlan {
            declarations: context.declarations.to_string(),
            parameters: context.parameters.to_string(),
            root,
            items,
        },
        work,
    ))
}

fn parse_template(source: &str) -> Option<TemplateAst> {
    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |event| {
        syntax.handle(
            &event,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    syntax.take_template_ast()
}

struct Builder<'s, 'p, 'alloc> {
    source: &'s str,
    ast: &'p TemplateAst,
    oxc: &'p OxcParsedAst<'alloc>,
    resolver: &'p BindingResolver<'alloc>,
    contracts: &'p [(&'p str, &'p str)],
    keys: FxHashMap<LexicalScopeId, ScopeKey>,
    work: BuildWork,
}

impl<'s, 'p, 'alloc> Builder<'s, 'p, 'alloc> {
    fn key(&mut self, scope: LexicalScopeId) -> ScopeKey {
        self.work.scope_handles += 1;
        let next = ScopeKey(self.keys.len() as u32);
        *self.keys.entry(scope).or_insert(next)
    }

    fn contract(&self, key: &str) -> Result<String, BuildError> {
        self.contracts
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, contract)| (*contract).to_string())
            .ok_or_else(|| BuildError::MissingContract {
                key: key.to_string(),
            })
    }

    fn element(
        &self,
        id: NodeId,
    ) -> Option<(&'p ElementNode, Option<&'p OxcParsedElement<'alloc>>)> {
        let AstNodeKind::Element(el) = &self.ast.nodes[id.0].kind else {
            return None;
        };
        let oxc_el = match &self.oxc.data[id.0] {
            OxcNodeData::Element(el) => Some(el.as_ref()),
            _ => None,
        };
        Some((el.as_ref(), oxc_el))
    }

    fn children(
        &mut self,
        children: &[NodeId],
        chains: &[ConditionalChain],
        scope: LexicalScopeId,
    ) -> Result<Vec<Item>, BuildError> {
        let mut chain_of: FxHashMap<usize, (usize, usize)> = FxHashMap::default();
        for (chain_index, chain) in chains.iter().enumerate() {
            for (position, &member) in chain.member_indices.iter().enumerate() {
                chain_of.insert(member, (chain_index, position));
            }
        }
        let mut items = Vec::new();
        for (index, &child) in children.iter().enumerate() {
            match chain_of.get(&index) {
                Some(&(chain_index, 0)) => {
                    let members: Vec<NodeId> = chains[chain_index]
                        .member_indices
                        .iter()
                        .map(|&member| children[member])
                        .collect();
                    items.push(Item::Chain(self.chain(&members, scope)?));
                }
                Some(_) => {}
                None => items.extend(self.node(child, scope)?),
            }
        }
        Ok(items)
    }

    fn chain(&mut self, members: &[NodeId], scope: LexicalScopeId) -> Result<Chain, BuildError> {
        let scope_key = self.key(scope);
        let mut branches = Vec::with_capacity(members.len());
        let mut predecessor = None;
        for &member in members {
            let Some((el, oxc_el)) = self.element(member) else {
                continue;
            };
            let Some(directive) = &el.v_condition else {
                continue;
            };
            if self.oxc.scope_of(member, self.ast) != scope {
                return Err(BuildError::UnexpectedScopeChain {
                    at: directive.prop.start,
                });
            }
            let kind = match directive.kind {
                ElementNodeConditionKind::If => BranchKind::If,
                ElementNodeConditionKind::ElseIf => BranchKind::ElseIf,
                ElementNodeConditionKind::Else => BranchKind::Else,
            };
            let condition = match (kind, directive.prop.value_start, directive.prop.value_end) {
                (BranchKind::Else, ..) => None,
                (_, Some(start), Some(end)) => {
                    let parsed = oxc_el
                        .and_then(|el| el.condition.as_ref())
                        .ok_or(BuildError::UnparsedExpression { at: start })?;
                    self.work.conditions_resolved += 1;
                    Some(self.resolve(parsed, start, end)?)
                }
                _ => {
                    return Err(BuildError::UnparsedExpression {
                        at: directive.prop.start,
                    })
                }
            };
            let key = BranchKey(member.0 as u32);
            branches.push(Branch {
                key,
                kind,
                predecessor,
                condition,
                scope: scope_key,
                items: self.element_body(member, el, oxc_el, scope)?,
            });
            predecessor = Some(key);
        }
        Ok(Chain { branches })
    }

    fn node(&mut self, id: NodeId, scope: LexicalScopeId) -> Result<Vec<Item>, BuildError> {
        match &self.ast.nodes[id.0].kind {
            AstNodeKind::Element(_) => {
                let (el, oxc_el) = self.element(id).expect("element node");
                self.element_body(id, el, oxc_el, scope)
            }
            AstNodeKind::Interpolation(interpolation) => {
                let OxcNodeData::Interpolation(parsed) = &self.oxc.data[id.0] else {
                    return Err(BuildError::UnparsedExpression {
                        at: interpolation.inner_start,
                    });
                };
                let expr =
                    self.resolve(parsed, interpolation.inner_start, interpolation.inner_end)?;
                Ok(vec![Item::Expr(ExprItem {
                    scope: self.key(scope),
                    contract: None,
                    expr,
                })])
            }
            AstNodeKind::Text(_) | AstNodeKind::Comment(_) => Ok(Vec::new()),
        }
    }

    /// The items an element contributes once its own condition (if any) holds:
    /// a `v-for` frame around its attributes and content, and a `v-slot` frame
    /// around its content.
    fn element_body(
        &mut self,
        id: NodeId,
        el: &'p ElementNode,
        oxc_el: Option<&'p OxcParsedElement<'alloc>>,
        scope: LexicalScopeId,
    ) -> Result<Vec<Item>, BuildError> {
        let props_scope = oxc_el.map_or(scope, |el| el.props_scope);
        let children_scope = self.oxc.children_scope(id);
        self.work.scope_handles += 2;

        let mut items = self.attributes(el, oxc_el, props_scope)?;
        let content = match &el.content {
            Some(content) => {
                self.children(&content.children, &content.v_if_chains, children_scope)?
            }
            None => Vec::new(),
        };
        if children_scope != props_scope {
            let slot = el.v_slot.as_ref().ok_or(BuildError::UnexpectedScopeChain {
                at: el.tag_open.start,
            })?;
            if self.oxc.scopes.parent(children_scope) != props_scope {
                return Err(BuildError::UnexpectedScopeChain { at: slot.start });
            }
            let (Some(start), Some(end)) = (slot.value_start, slot.value_end) else {
                return Err(BuildError::UnexpectedScopeChain { at: slot.start });
            };
            items.push(Item::Frame(Frame {
                scope: self.key(children_scope),
                parent: self.key(props_scope),
                binding: FrameBinding::Slot {
                    pattern: trim(self.source, Span::new(start, end)),
                    contract: self.contract(directive_key(slot, self.source))?,
                },
                items: content,
            }));
        } else {
            items.extend(content);
        }

        if props_scope == scope {
            return Ok(items);
        }
        let v_for = el.v_for.as_ref().ok_or(BuildError::UnexpectedScopeChain {
            at: el.tag_open.start,
        })?;
        if self.oxc.scopes.parent(props_scope) != scope {
            return Err(BuildError::UnexpectedScopeChain { at: v_for.start });
        }
        let binding = self.v_for(v_for, oxc_el)?;
        Ok(vec![Item::Frame(Frame {
            scope: self.key(props_scope),
            parent: self.key(scope),
            binding,
            items,
        })])
    }

    fn v_for(
        &mut self,
        prop: &NodeProp,
        oxc_el: Option<&'p OxcParsedElement<'alloc>>,
    ) -> Result<FrameBinding, BuildError> {
        let (Some(start), Some(end)) = (prop.value_start, prop.value_end) else {
            return Err(BuildError::UnparsedExpression { at: prop.start });
        };
        let parsed = oxc_el
            .and_then(|el| el.v_for.as_ref())
            .ok_or(BuildError::UnparsedExpression { at: start })?;
        let (Some(left), Some(right)) = (parsed.parsed.left(), parsed.parsed.right()) else {
            return Err(BuildError::UnparsedExpression { at: start });
        };
        let (aliases, arity) = match left.without_parentheses() {
            Expression::SequenceExpression(sequence) => {
                (sequence.span, sequence.expressions.len() as u8)
            }
            other => (other.span(), 1),
        };
        // The v-for parse reports file-relative spans.
        let aliases = Span::new(aliases.start, aliases.end);
        let iterable = Span::new(right.span().start, right.span().end);
        if aliases.end > iterable.start || iterable.end > end {
            return Err(BuildError::UnparsedExpression { at: start });
        }
        let text = &self.source[iterable.start as usize..iterable.end as usize];
        let mgt = crate::ide::template::directives::resolve_iterable_segments(
            text,
            iterable.start,
            oxc_el,
            self.source,
            self.resolver,
        );
        Ok(FrameBinding::VFor {
            aliases,
            arity,
            source: self.pieces(iterable, &mgt)?,
        })
    }

    fn attributes(
        &mut self,
        el: &'p ElementNode,
        oxc_el: Option<&'p OxcParsedElement<'alloc>>,
        scope: LexicalScopeId,
    ) -> Result<Vec<Item>, BuildError> {
        let mut items = Vec::new();
        for (index, prop) in el.props.iter().enumerate() {
            if !prop.is_directive {
                continue;
            }
            let name = get_directive_name(prop, self.source);
            if !matches!(name, "on" | "bind" | "show") {
                continue;
            }
            let (Some(start), Some(end)) = (prop.value_start, prop.value_end) else {
                continue;
            };
            let parsed = oxc_el
                .and_then(|el| el.prop(index))
                .and_then(|p| p.exp.as_ref())
                .ok_or(BuildError::UnparsedExpression { at: start })?;
            if parsed.multi_statement {
                return Err(BuildError::MultiStatementHandler { at: start });
            }
            let expression = parsed
                .expression
                .as_ref()
                .ok_or(BuildError::UnparsedExpression { at: start })?;
            let key = directive_key(prop, self.source);
            let scope_key = self.key(scope);
            let resolved = self.resolve(parsed, start, end)?;
            let item = match function_of(expression) {
                Some(function) => {
                    self.work.callbacks += 1;
                    let base = parsed.offset;
                    let guard = match function {
                        CallbackShape::Arrow(arrow) => match arrow.get_expression() {
                            Some(body) => GuardSite::Expression {
                                before: base + body.span().start,
                            },
                            None => GuardSite::Block {
                                after: base + arrow.body.span().start + 1,
                            },
                        },
                        CallbackShape::Function(function) => {
                            let body = function
                                .body
                                .as_ref()
                                .ok_or(BuildError::UnparsedExpression { at: start })?;
                            GuardSite::Block {
                                after: base + body.span.start + 1,
                            }
                        }
                    };
                    let outer_refs = self.outer_refs(parsed, OuterRefSource::Function(function));
                    Item::Callback(Callback {
                        scope: scope_key,
                        contract: self.contract(key)?,
                        function: CallbackFunction::Authored {
                            expr: resolved,
                            guard,
                        },
                        outer_refs,
                    })
                }
                None if name == "on" && !is_reference_path(expression) => {
                    self.work.callbacks += 1;
                    let uses_event = parsed
                        .bindings
                        .as_ref()
                        .is_some_and(|b| b.bindings.iter().any(|b| b.name == "$event"));
                    let outer_refs = self.outer_refs(parsed, OuterRefSource::Handler(expression));
                    Item::Callback(Callback {
                        scope: scope_key,
                        contract: self.contract(key)?,
                        function: CallbackFunction::Handler {
                            parameters: if uses_event {
                                "$event".into()
                            } else {
                                String::new()
                            },
                            body: resolved,
                        },
                        outer_refs,
                    })
                }
                None => Item::Expr(ExprItem {
                    scope: scope_key,
                    contract: match name {
                        "on" => Some(self.contract(key)?),
                        _ => self
                            .contracts
                            .iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, c)| (*c).to_string()),
                    },
                    expr: resolved,
                }),
            };
            items.push(item);
        }
        Ok(items)
    }

    fn resolve(
        &self,
        parsed: &OxcParsedExpression<'alloc>,
        start: u32,
        end: u32,
    ) -> Result<ResolvedExpr, BuildError> {
        let span = Span::new(start, end);
        let text = &self.source[start as usize..end as usize];
        let mgt = build_prefixed_expr_segments(text, start, parsed, self.resolver, &[]);
        self.pieces(span, &mgt)
    }

    /// Convert the shared producer's segment plan into seam pieces, refusing a
    /// plan whose authored runs are not the authored bytes.
    fn pieces(&self, span: Span, mgt: &MappedGeneratedText) -> Result<ResolvedExpr, BuildError> {
        let mut pieces = Vec::with_capacity(mgt.segments.len());
        for segment in &mgt.segments {
            let text = &mgt.text[segment.generated_start as usize..segment.generated_end as usize];
            match segment.source_start {
                Some(source_start) => {
                    let authored = Span::new(source_start, source_start + text.len() as u32);
                    let slice = self
                        .source
                        .get(authored.start as usize..authored.end as usize);
                    if slice != Some(text) {
                        return Err(BuildError::ResolvedTextDiverges { at: source_start });
                    }
                    match pieces.last_mut() {
                        Some(Piece::Authored(previous)) if previous.end == authored.start => {
                            previous.end = authored.end;
                        }
                        _ => pieces.push(Piece::Authored(authored)),
                    }
                }
                None => pieces.push(Piece::Synthetic(text.to_string())),
            }
        }
        // The producer drops the whitespace around an expression; the resolved
        // expression spans exactly its authored runs.
        let mut authored = pieces.iter().filter_map(|piece| match piece {
            Piece::Authored(span) => Some(*span),
            Piece::Synthetic(_) => None,
        });
        let span = authored
            .clone()
            .next()
            .zip(authored.next_back())
            .map_or(Span::new(span.start, span.start), |(first, last)| {
                Span::new(first.start, last.end)
            });
        Ok(ResolvedExpr { span, pieces })
    }

    /// The reference chains a callback body reads in its own flow, each with
    /// its prefixes, prefixes first.
    fn outer_refs(
        &self,
        parsed: &OxcParsedExpression<'alloc>,
        function: OuterRefSource<'_, 'alloc>,
    ) -> Vec<OuterRef> {
        let base = parsed.offset;
        let mut locals = Locals::default();
        match function {
            OuterRefSource::Function(CallbackShape::Arrow(arrow)) => {
                locals.function(
                    arrow.span,
                    &arrow.params,
                    arrow.get_function_body(),
                    arrow.get_expression(),
                );
            }
            OuterRefSource::Function(CallbackShape::Function(function)) => {
                locals.function(
                    function.span,
                    &function.params,
                    function.body.as_deref(),
                    None,
                );
            }
            OuterRefSource::Handler(expression) => {
                locals.declare("$event", expression.span());
                locals.visit_expression(expression);
            }
        }
        let mut refs = Refs {
            locals: &locals,
            chains: Vec::new(),
        };
        match function {
            OuterRefSource::Function(CallbackShape::Arrow(arrow)) => {
                for param in &arrow.params.items {
                    if let Some(initializer) = &param.initializer {
                        refs.visit_expression(initializer);
                    }
                }
                match (arrow.get_function_body(), arrow.get_expression()) {
                    (_, Some(expression)) => refs.visit_expression(expression),
                    (Some(body), None) => refs.statements(&body.statements),
                    (None, None) => {}
                }
            }
            OuterRefSource::Function(CallbackShape::Function(function)) => {
                if let Some(body) = &function.body {
                    refs.statements(&body.statements);
                }
            }
            OuterRefSource::Handler(expression) => refs.visit_expression(expression),
        }

        let mut seen: FxHashMap<String, usize> = FxHashMap::default();
        let mut out: Vec<(usize, OuterRef)> = Vec::new();
        for chain in refs.chains {
            let root_text = self.root_text(parsed, base + chain.root.start, chain.name);
            let mut text = root_text;
            let mut occurrence = Span::new(base + chain.root.start, base + chain.root.end);
            for depth in 0..=chain.steps.len() {
                if depth > 0 {
                    let step = &chain.steps[depth - 1];
                    text.push_str(if step.optional { "?." } else { "!." });
                    match step.key {
                        StepKey::Name(name) => text.push_str(name),
                        StepKey::Index(span) => {
                            text.pop();
                            if step.optional {
                                text.push('.');
                            }
                            text.push('[');
                            text.push_str(
                                &self.source
                                    [(base + span.start) as usize..(base + span.end) as usize],
                            );
                            text.push(']');
                        }
                    }
                    occurrence.end = base + step.end;
                }
                if !seen.contains_key(&text) {
                    seen.insert(text.clone(), out.len());
                    out.push((
                        depth,
                        OuterRef {
                            text: text.clone(),
                            occurrence,
                        },
                    ));
                }
            }
        }
        out.sort_by_key(|(depth, _)| *depth);
        out.into_iter().map(|(_, outer)| outer).collect()
    }

    /// Resolved text of a free root identifier: its accessor prefix when the
    /// template parse classified it as an outer binding, the bare name for a
    /// template-scope local.
    fn root_text(&self, parsed: &OxcParsedExpression<'alloc>, pos: u32, name: &str) -> String {
        let outer = parsed
            .bindings
            .as_ref()
            .and_then(|b| b.bindings.iter().find(|b| b.pos == pos))
            .is_some_and(|b| !b.ignore);
        if outer {
            format!(
                "{}{}{}",
                self.resolver.resolve_prefix(name),
                name,
                self.resolver.resolve_suffix(name)
            )
        } else {
            name.to_string()
        }
    }
}

#[derive(Clone, Copy)]
enum CallbackShape<'e, 'alloc> {
    Arrow(&'e ArrowFunctionExpression<'alloc>),
    Function(&'e Function<'alloc>),
}

#[derive(Clone, Copy)]
enum OuterRefSource<'e, 'alloc> {
    Function(CallbackShape<'e, 'alloc>),
    Handler(&'e Expression<'alloc>),
}

fn function_of<'e, 'alloc>(
    expression: &'e Expression<'alloc>,
) -> Option<CallbackShape<'e, 'alloc>> {
    match expression.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => Some(CallbackShape::Arrow(arrow)),
        Expression::FunctionExpression(function) => Some(CallbackShape::Function(function)),
        _ => None,
    }
}

/// `handler` / `obj.handler`: a value the emitter passes as is, not a statement.
fn is_reference_path(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::Identifier(_) => true,
        Expression::StaticMemberExpression(member) => is_reference_path(&member.object),
        _ => false,
    }
}

/// `@click`, `:on-pick`, `#row`, `v-slot`: the authored directive name with
/// its argument and modifiers, without the value.
fn directive_key<'s>(prop: &NodeProp, source: &'s str) -> &'s str {
    let end = prop
        .modifiers
        .last()
        .map(|m| m.end)
        .or(prop.arg_end)
        .unwrap_or(prop.name_end)
        .max(prop.name_end);
    &source[prop.start as usize..end as usize]
}

fn trim(source: &str, span: Span) -> Span {
    let text = &source[span.start as usize..span.end as usize];
    let leading = (text.len() - text.trim_start().len()) as u32;
    let trailing = (text.len() - text.trim_end().len()) as u32;
    Span::new(
        span.start + leading,
        (span.end - trailing).max(span.start + leading),
    )
}

/// Names a callback declares, each with the authored range it is visible in.
#[derive(Default)]
struct Locals<'alloc> {
    declared: Vec<(&'alloc str, oxc_span::Span)>,
    /// Innermost lexical range first; the function range at the bottom.
    blocks: Vec<oxc_span::Span>,
    function: Option<oxc_span::Span>,
}

impl<'alloc> Locals<'alloc> {
    fn function(
        &mut self,
        span: oxc_span::Span,
        params: &FormalParameters<'alloc>,
        body: Option<&FunctionBody<'alloc>>,
        expression: Option<&Expression<'alloc>>,
    ) {
        self.function = Some(span);
        self.blocks.push(span);
        for param in &params.items {
            self.pattern(&param.pattern, span);
        }
        if let Some(rest) = &params.rest {
            self.pattern(&rest.rest.argument, span);
        }
        if let Some(body) = body {
            for statement in &body.statements {
                self.visit_statement(statement);
            }
        }
        if let Some(expression) = expression {
            self.visit_expression(expression);
        }
    }

    fn declare(&mut self, name: &'alloc str, span: oxc_span::Span) {
        self.declared.push((name, span));
    }

    fn pattern(&mut self, pattern: &BindingPattern<'alloc>, scope: oxc_span::Span) {
        match pattern {
            BindingPattern::BindingIdentifier(ident) => self.declare(ident.name.as_str(), scope),
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    self.pattern(&property.value, scope);
                }
                if let Some(rest) = &object.rest {
                    self.pattern(&rest.argument, scope);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.pattern(element, scope);
                }
                if let Some(rest) = &array.rest {
                    self.pattern(&rest.argument, scope);
                }
            }
            BindingPattern::AssignmentPattern(assign) => self.pattern(&assign.left, scope),
        }
    }

    fn block(&self) -> oxc_span::Span {
        *self.blocks.last().expect("a callback scope is open")
    }

    fn is_local(&self, name: &str, at: u32) -> bool {
        self.declared
            .iter()
            .any(|(declared, scope)| *declared == name && scope.start <= at && at < scope.end)
    }

    fn scoped(&mut self, span: oxc_span::Span, visit: impl FnOnce(&mut Self)) {
        self.blocks.push(span);
        visit(self);
        self.blocks.pop();
    }
}

impl<'alloc> Visit<'alloc> for Locals<'alloc> {
    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_block_statement(this, it));
    }

    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_statement(this, it));
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_in_statement(this, it));
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_of_statement(this, it));
    }

    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_switch_statement(this, it));
    }

    fn visit_catch_clause(&mut self, it: &oxc_ast::ast::CatchClause<'alloc>) {
        self.scoped(it.span, |this| {
            if let Some(param) = &it.param {
                let scope = this.block();
                this.pattern(&param.pattern, scope);
            }
            this.visit_block_statement(&it.body);
        });
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'alloc>) {
        let scope = if it.kind.is_var() {
            self.function.expect("a callback scope is open")
        } else {
            self.block()
        };
        for declarator in &it.declarations {
            self.pattern(&declarator.id, scope);
            if let Some(init) = &declarator.init {
                self.visit_expression(init);
            }
        }
    }

    fn visit_function(&mut self, it: &Function<'alloc>, _flags: ScopeFlags) {
        if let Some(id) = &it.id {
            if it.is_declaration() {
                let scope = self.block();
                self.declare(id.name.as_str(), scope);
            }
        }
    }

    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'alloc>) {
        if let Some(id) = &it.id {
            if it.is_declaration() {
                let scope = self.block();
                self.declare(id.name.as_str(), scope);
            }
        }
    }

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'alloc>) {}
}

/// One reference chain: a free root identifier followed by static or
/// literal-indexed steps. Spans are relative to the parsed expression.
struct RefChain<'alloc> {
    name: &'alloc str,
    root: oxc_span::Span,
    steps: Vec<Step<'alloc>>,
}

struct Step<'alloc> {
    optional: bool,
    key: StepKey<'alloc>,
    /// End of this step in the parsed expression.
    end: u32,
}

enum StepKey<'alloc> {
    Name(&'alloc str),
    /// Span of a string or numeric literal index.
    Index(oxc_span::Span),
}

struct Refs<'l, 'alloc> {
    locals: &'l Locals<'alloc>,
    chains: Vec<RefChain<'alloc>>,
}

impl<'alloc> Refs<'_, 'alloc> {
    fn statements(&mut self, statements: &[Statement<'alloc>]) {
        for statement in statements {
            self.visit_statement(statement);
        }
    }

    fn free(&self, name: &str, at: u32) -> bool {
        !matches!(name, "undefined" | "NaN" | "Infinity" | "arguments")
            && !self.locals.is_local(name, at)
    }

    /// Record the longest reference chain ending at `expression`, visiting
    /// whatever is not part of it. Returns whether `expression` was a chain.
    fn chain(&mut self, expression: &Expression<'alloc>) -> bool {
        let mut steps = Vec::new();
        let mut current = expression;
        loop {
            match current {
                Expression::StaticMemberExpression(member) => {
                    steps.push(Step {
                        optional: member.optional,
                        key: StepKey::Name(member.property.name.as_str()),
                        end: member.span.end,
                    });
                    current = &member.object;
                }
                Expression::ComputedMemberExpression(member) => match &member.expression {
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_) => {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Index(member.expression.span()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    other => {
                        self.visit_expression(other);
                        steps.clear();
                        current = &member.object;
                    }
                },
                Expression::ChainExpression(chain) => match &chain.expression {
                    ChainElement::StaticMemberExpression(member) => {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Name(member.property.name.as_str()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    ChainElement::ComputedMemberExpression(member)
                        if matches!(
                            member.expression,
                            Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                        ) =>
                    {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Index(member.expression.span()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    _ => {
                        walk::walk_chain_expression(self, chain);
                        return true;
                    }
                },
                Expression::TSNonNullExpression(non_null) => current = &non_null.expression,
                Expression::ParenthesizedExpression(paren) => current = &paren.expression,
                Expression::Identifier(ident) => {
                    if self.free(ident.name.as_str(), ident.span.start) {
                        steps.reverse();
                        self.chains.push(RefChain {
                            name: ident.name.as_str(),
                            root: ident.span,
                            steps,
                        });
                    }
                    return true;
                }
                other => {
                    if steps.is_empty() && std::ptr::eq(other, expression) {
                        return false;
                    }
                    self.visit_expression(other);
                    return true;
                }
            }
        }
    }
}

impl<'alloc> Visit<'alloc> for Refs<'_, 'alloc> {
    fn visit_expression(&mut self, it: &Expression<'alloc>) {
        if !self.chain(it) {
            walk::walk_expression(self, it);
        }
    }

    fn visit_function(&mut self, _it: &Function<'alloc>, _flags: ScopeFlags) {}

    fn visit_class(&mut self, _it: &oxc_ast::ast::Class<'alloc>) {}

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'alloc>) {}
}
