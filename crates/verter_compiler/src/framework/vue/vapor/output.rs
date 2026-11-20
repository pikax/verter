//! Lower rows in their fixed order, then write immutable emission pieces once.
use verter_span::Span;

use super::effect::{EffectGroupId, EffectId, TextPart, VaporEffect};
use super::ops::{
    BlockKind, ClosedBlock, InsertionAnchor, NameKind, Nav, Part, RunBuf, RunId, Stmt,
};
use super::scope::VaporScope;
use super::state::VaporState;
use super::VaporCompiler;
use crate::emission::EmissionBuilder;
use crate::framework::vue::target::VueTargetRefusal;
use crate::template::code_gen::shared::helpers::{push_u32, VaporHelper};
use crate::template::vue_syntax::escape_js_string_into;

impl<'c> VaporCompiler<'c> {
    /// All late references must be resolved before the block acquires a run.
    pub(super) fn close_block(
        &mut self,
        mut scope: VaporScope<'c>,
        has_dynamic_text: bool,
        indent: &'c str,
        is_root: bool,
        closure: bool,
        span: Span,
    ) -> Result<ClosedBlock, VueTargetRefusal> {
        super::element::finalize_text_parts(&mut scope, has_dynamic_text, &mut self.state);
        self.resolve_pending_nav_requests(&mut scope)?;
        self.state
            .close_scope(scope, indent, is_root, closure, span)
    }

    /// A text run directly inside a transparent wrapper is one of its roots:
    /// it collects into its own scope until the run ends.
    pub(super) fn open_text_root(&mut self) {
        if !self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_transparent_wrapper)
        {
            return;
        }
        let mut scope = self.state_pool.pop().unwrap_or_default();
        scope.text_root = true;
        self.html_scope_stack.push(std::mem::take(&mut self.html));
        self.element_stack.push(scope);
    }

    /// Close an open text root: its template is the text node itself, which
    /// a dynamic run sets directly.
    pub(super) fn close_text_root(&mut self, span: Span) -> Result<(), VueTargetRefusal> {
        if !self
            .element_stack
            .last()
            .is_some_and(|scope| scope.text_root)
        {
            return Ok(());
        }
        let mut scope = self.element_stack.pop().expect("open text root");
        self.take_scope_html(&mut scope);
        let node_ref = scope.ensure_node_ref(&mut self.state.counters);
        let parts = std::mem::take(&mut scope.text_parts);
        scope.text_node_ref = None;
        if parts
            .iter()
            .any(|part| matches!(part, TextPart::Dynamic(_)))
        {
            let effect = self.state.add_effect(VaporEffect::Text {
                text_ref: node_ref,
                parts,
                generated: false,
            });
            scope.own_effects.push(effect);
        }
        self.push_root(scope, span)
    }

    /// A comment directly inside a transparent wrapper is one of its roots.
    pub(super) fn push_comment_root(
        &mut self,
        comment: &crate::ast::types::CommentNode,
        span: Span,
    ) -> Result<(), VueTargetRefusal> {
        let mut scope = self.state_pool.pop().unwrap_or_default();
        super::comment::process_comment(comment, self.inputs.source.text, true, &mut scope.html);
        self.push_root(scope, span)
    }

    /// Close one root of the enclosing transparent wrapper into its roots.
    pub(super) fn push_root(
        &mut self,
        scope: VaporScope<'c>,
        span: Span,
    ) -> Result<(), VueTargetRefusal> {
        let root_template = self
            .element_stack
            .last()
            .expect("a block root has an enclosing wrapper")
            .root_template;
        let root = self
            .state
            .close_scope(scope, "    ", root_template, false, span)?;
        self.element_stack
            .last_mut()
            .expect("a block root has an enclosing wrapper")
            .roots
            .push(root);
        Ok(())
    }

    /// A transparent wrapper's body: its roots in order, returning the sole
    /// root or, for several, all of them as an array.
    pub(super) fn close_roots(
        &mut self,
        mut scope: VaporScope<'c>,
        span: Span,
    ) -> Result<ClosedBlock, VueTargetRefusal> {
        let roots = std::mem::take(&mut scope.roots);
        let residue = !scope.html.is_empty()
            || !scope.child_nav.is_empty()
            || !scope.child_text_creations.is_empty()
            || !scope.child_statements.is_empty()
            || !scope.own_effects.is_empty()
            || !scope.child_effects.is_empty()
            || !scope.pending_nav_requests.is_empty();
        self.recycle(scope);
        let Some(first) = roots.first().copied() else {
            return Err(VueTargetRefusal::Unsupported {
                code: "empty-block-roots",
                span,
            });
        };
        if residue {
            return Err(VueTargetRefusal::Unsupported {
                code: "block-root-residue",
                span,
            });
        }
        let mut parts: Vec<Part<'c>> = roots.iter().map(|root| Part::Run(root.run)).collect();
        if let [root] = roots.as_slice() {
            parts.extend([
                Part::Text("    return "),
                Part::Name(NameKind::Node, root.node_ref),
                Part::Text("\n"),
            ]);
            return Ok(ClosedBlock {
                run: self.state.runs.commit(&mut parts),
                ..*root
            });
        }
        parts.push(Part::Text("    return ["));
        for (position, root) in roots.iter().enumerate() {
            if position > 0 {
                parts.push(Part::Text(", "));
            }
            parts.push(Part::Name(NameKind::Node, root.node_ref));
        }
        parts.push(Part::Text("]\n"));
        Ok(ClosedBlock {
            run: self.state.runs.commit(&mut parts),
            node_ref: first.node_ref,
            template_idx: None,
            is_static: roots.iter().all(|root| root.is_static),
            kind: BlockKind::Roots,
        })
    }
}

impl<'c> VaporState<'c> {
    pub(super) fn close_scope(
        &mut self,
        mut scope: VaporScope<'c>,
        indent: &'c str,
        is_root: bool,
        closure: bool,
        span: Span,
    ) -> Result<ClosedBlock, VueTargetRefusal> {
        let resolved = scope.pending_nav_requests.is_empty()
            && scope
                .child_nav
                .iter()
                .all(|row| !matches!(row, Nav::Reserved))
            && scope
                .child_statements
                .iter()
                .all(|row| !matches!(row, Stmt::Reserved));
        if !resolved {
            return Err(VueTargetRefusal::Unsupported {
                code: "unresolved-operation-slot",
                span,
            });
        }
        verter_debug_assert!(resolved, "a closed block contains no reserved slots");
        let is_static = scope.child_nav.is_empty()
            && scope.child_text_creations.is_empty()
            && scope.text_node_ref.is_none()
            && scope.child_statements.is_empty()
            && scope.own_effects.is_empty()
            && scope.child_effects.is_empty();
        // A component or slot body made of one text run has no element: its
        // template's root is the text node itself, which its text effect sets
        // directly (`txt(node)` would read the text node's absent first child).
        let text_root = scope.is_component_scope
            && !scope.html.starts_with('<')
            && scope.text_node_ref.is_some()
            && scope.text_node_ref == scope.node_ref;
        if text_root {
            for effect in &scope.own_effects {
                if let VaporEffect::Text { generated, .. } = &mut self.effects[effect.0 as usize] {
                    *generated = false;
                }
            }
            scope.text_ref_generated = false;
        }
        super::element::strip_trailing_close_tags(&mut scope.html);
        let template_idx = Some(self.declare_template(
            std::mem::take(&mut scope.html),
            is_root,
            is_static,
            scope.namespace,
        ));
        let node_ref = scope.ensure_node_ref(&mut self.counters);
        let mut parts = Vec::new();
        if let Some(template) = template_idx {
            parts.extend([
                Part::Text(indent),
                Part::Text("const "),
                Part::Name(NameKind::Node, node_ref),
                Part::Text(" = "),
                Part::Name(NameKind::Template, template),
                Part::Text("()\n"),
            ]);
        }
        let mut cursors = Vec::new();
        for row in scope.child_nav {
            parts.push(Part::Text(indent));
            self.record_nav(&mut parts, row, &mut cursors);
            parts.push(Part::Text("\n"));
        }
        for row in scope.child_text_creations {
            parts.extend([
                Part::Text(indent),
                Part::Text("const "),
                Part::Name(NameKind::Text, row.text),
                Part::Text(" = _txt("),
                Part::Name(row.from.0, row.from.1),
                Part::Text(")\n"),
            ]);
            self.use_helper(VaporHelper::Txt);
        }
        if let Some(text) = scope.text_node_ref {
            if scope.text_ref_generated {
                parts.extend([
                    Part::Text(indent),
                    Part::Text("const "),
                    Part::Name(NameKind::Text, text),
                    Part::Text(" = _txt("),
                    Part::Name(NameKind::Node, node_ref),
                    Part::Text(")\n"),
                ]);
                self.use_helper(VaporHelper::Txt);
            }
        }
        for row in scope.child_statements {
            parts.push(Part::Text(indent));
            match row {
                Stmt::Reserved => unreachable!("checked before closing"),
                Stmt::Run(run) => parts.push(Part::Indented(run, indent)),
                Stmt::InsertionState { container, anchor } => {
                    parts.extend([
                        Part::Text("_setInsertionState("),
                        Part::Name(NameKind::Node, container),
                    ]);
                    match anchor {
                        InsertionAnchor::Append => {}
                        InsertionAnchor::NodeRef(node) => {
                            parts.extend([Part::Text(", "), Part::Name(NameKind::Node, node)]);
                        }
                        InsertionAnchor::Index(index) => {
                            parts.push(Part::Text(", "));
                            parts.push(Part::Name(NameKind::Number, index));
                        }
                    }
                    parts.push(Part::Text(")"));
                    self.use_helper(VaporHelper::SetInsertionState);
                }
            }
            parts.push(Part::Text("\n"));
        }
        scope.own_effects.append(&mut scope.child_effects);
        if !scope.own_effects.is_empty() {
            let wrapper = if scope.once {
                super::graph::EffectWrapper::Once
            } else if let Some(deps) = scope.memo {
                super::graph::EffectWrapper::Memo {
                    deps,
                    cache: self.counters.next_memo(),
                }
            } else {
                super::graph::EffectWrapper::Render
            };
            let group = self.add_effect_group(scope.own_effects, indent, wrapper)?;
            parts.push(Part::Text(indent));
            parts.push(Part::Effects(group));
            parts.push(Part::Text("\n"));
        }
        if closure {
            parts.extend([
                Part::Text(indent),
                Part::Text("return "),
                Part::Name(NameKind::Node, node_ref),
                Part::Text("\n"),
            ]);
        }
        Ok(ClosedBlock {
            run: self.runs.commit(&mut parts),
            node_ref,
            template_idx,
            is_static,
            kind: BlockKind::Node,
        })
    }

    /// Declare one navigated child as runtime-vapor 3.6.0-rc.15 reaches it
    /// (`dom/node.ts`): `child(n)` is `n.firstChild`, `next(n)` is
    /// `n.nextSibling` and reads its second argument only as a hydration text
    /// flag, and `nthChild(n, i)` is `n.childNodes[i]`. As Vue's
    /// `genAccessPath` does, the sibling right after the container's previous
    /// navigated child chains off it; any other child is reached from its
    /// container by position.
    fn record_nav(&mut self, parts: &mut RunBuf<'c>, nav: Nav, cursors: &mut Vec<NavCursor>) {
        let Nav::At {
            target,
            container,
            index,
            text,
        } = nav
        else {
            unreachable!("checked before closing")
        };
        parts.extend([
            Part::Text("const "),
            Part::Name(target.0, target.1),
            Part::Text(" = "),
        ]);
        let previous = cursors
            .iter()
            .position(|cursor| cursor.container == container);
        match previous.map(|position| &cursors[position]) {
            Some(previous) if previous.index + 1 == index => {
                parts.extend([
                    Part::Text("_next("),
                    Part::Name(previous.name.0, previous.name.1),
                ]);
                self.use_helper(VaporHelper::Next);
            }
            _ if index == 0 => {
                parts.extend([Part::Text("_child("), Part::Name(NameKind::Node, container)]);
                self.use_helper(VaporHelper::Child);
            }
            _ if index == 1 => {
                parts.extend([
                    Part::Text("_next(_child("),
                    Part::Name(NameKind::Node, container),
                    Part::Text(")"),
                ]);
                self.use_helper(VaporHelper::Child);
                self.use_helper(VaporHelper::Next);
            }
            _ => {
                parts.extend([
                    Part::Text("_nthChild("),
                    Part::Name(NameKind::Node, container),
                    Part::Text(", "),
                    Part::Name(NameKind::Number, index),
                ]);
                self.use_helper(VaporHelper::NthChild);
            }
        }
        if text {
            parts.push(Part::Text(", true"));
        }
        parts.push(Part::Text(")"));
        let cursor = NavCursor {
            container,
            name: target,
            index,
        };
        match previous {
            Some(position) => cursors[position] = cursor,
            None => cursors.push(cursor),
        }
    }

    pub(super) fn write_preamble(&mut self) {
        let single_root = self.roots.len() == 1;
        for root in &self.roots {
            if let Some(index) = root.template_idx {
                self.templates[index as usize].is_root = single_root;
            }
        }
        self.templates
            .sort_unstable_by_key(|template| template.index);
        let mut line = String::new();
        for template in &self.templates {
            line.clear();
            write_template_declaration(&mut line, template);
            line.push('\n');
            self.preamble.push_unmapped(&line);
        }
        if !self.delegated_events.is_empty() {
            let mut names: Vec<&str> = self.delegated_events.iter().map(String::as_str).collect();
            names.sort_unstable();
            line.clear();
            line.push_str("_delegateEvents(");
            for (position, name) in names.iter().enumerate() {
                if position > 0 {
                    line.push_str(", ");
                }
                line.push('"');
                escape_js_string_into(&mut line, name);
                line.push('"');
            }
            line.push_str(")\n");
            self.preamble.push_unmapped(&line);
        }
    }

    pub(super) fn write_render(
        &mut self,
        allocator: &'c oxc_allocator::Allocator,
    ) -> Result<(), VueTargetRefusal> {
        self.effect_graph.relations()?;
        let graph = &self.effect_graph;
        self.render
            .push_borrowed("function render(_ctx, $props, $emit, $attrs, $slots) {\n");
        if self.template_ref_setter {
            self.render.push_borrowed(
                "  const _setTemplateRef = _createTemplateRefSetter()
",
            );
        }
        // Each component asset is resolved once, before any block uses it.
        for asset in &self.component_assets {
            self.render.push_unmapped_fmt(format_args!(
                "  const {} = _resolveComponent({})\n",
                asset.name, asset.quoted_tag
            ));
        }
        for root in &self.roots {
            write_run(&mut self.render, &self.runs, graph, allocator, root.run)?;
        }
        match self.roots.as_slice() {
            [] => self.render.push_borrowed("  return null\n"),
            [root] => self
                .render
                .push_unmapped_fmt(format_args!("  return n{}\n", root.node_ref)),
            roots => {
                self.render.push_borrowed("  return [");
                for (position, root) in roots.iter().enumerate() {
                    if position > 0 {
                        self.render.push_borrowed(", ");
                    }
                    self.render
                        .push_unmapped_fmt(format_args!("n{}", root.node_ref));
                }
                self.render.push_borrowed("]\n");
            }
        }
        self.render.push_borrowed("}");
        Ok(())
    }
}

/// `const t<index> = _template("<html>", flags, namespace)`, as
/// runtime-vapor 3.6.0-rc.15 `template(html, flags, ns)` reads it: the
/// flags are omitted when zero and no namespace follows, and the namespace
/// is written only for an SVG or MathML payload.
fn write_template_declaration(line: &mut String, template: &super::state::TemplateDeclaration) {
    line.push_str("const t");
    push_u32(line, template.index);
    line.push_str(" = _template(\"");
    escape_js_string_into(line, &template.html);
    line.push('"');
    let flags = u32::from(template.is_root) | (u32::from(template.is_static) << 1);
    if flags != 0 || template.namespace != 0 {
        line.push_str(", ");
        push_u32(line, flags);
    }
    if template.namespace != 0 {
        line.push_str(", ");
        push_u32(line, u32::from(template.namespace));
    }
    line.push(')');
}

/// The last child a closure declared under one container.
struct NavCursor {
    container: u32,
    name: (NameKind, u32),
    index: u32,
}

pub(super) fn record_effect<'c>(effect: &VaporEffect<'c>, parts: &mut RunBuf<'c>) {
    match effect {
        VaporEffect::Text {
            text_ref,
            parts: text,
            generated,
        } => {
            parts.extend([
                Part::Text("_setText("),
                Part::Name(
                    if *generated {
                        NameKind::Text
                    } else {
                        NameKind::Node
                    },
                    *text_ref,
                ),
                Part::Text(", "),
            ]);
            for (position, part) in text.iter().enumerate() {
                if position > 0 {
                    parts.push(Part::Text(" + "));
                }
                parts.push(match part {
                    TextPart::Static(text) => Part::Text(text),
                    TextPart::Dynamic(run) => Part::Run(*run),
                });
            }
            parts.push(Part::Text(")"));
        }
        VaporEffect::Class {
            node_ref,
            expr,
            svg,
        } => record_setter(parts, "_setClass(", *node_ref, None, *expr, svg_tail(*svg)),
        VaporEffect::Style { node_ref, expr } => {
            record_setter(parts, "_setStyle(", *node_ref, None, *expr, ")")
        }
        VaporEffect::Prop {
            node_ref,
            attr,
            expr,
        } => record_setter(parts, "_setProp(", *node_ref, Some(attr), *expr, ")"),
        VaporEffect::Attr {
            node_ref,
            attr,
            expr,
            svg,
        } => record_setter(
            parts,
            "_setAttr(",
            *node_ref,
            Some(attr),
            *expr,
            svg_tail(*svg),
        ),
        VaporEffect::DomProp {
            node_ref,
            attr,
            expr,
        } => record_setter(parts, "_setDOMProp(", *node_ref, Some(attr), *expr, ")"),
        VaporEffect::Html { node_ref, expr } => {
            record_setter(parts, "_setHtml(", *node_ref, None, *expr, ")")
        }
        VaporEffect::Value { node_ref, expr } => {
            record_setter(parts, "_setValue(", *node_ref, None, *expr, ")")
        }
        VaporEffect::ElementText { node_ref, expr } => {
            record_setter(parts, "_setElementText(", *node_ref, None, *expr, ")")
        }
        // Vue 3.6.0-rc.15 `genDynamicProps`: the SVG flag follows the
        // static-key list, which is `null` when no key is static.
        VaporEffect::DynamicProps {
            node_ref,
            expr,
            svg,
        } => parts.extend([
            Part::Text("_setDynamicProps("),
            Part::Name(NameKind::Node, *node_ref),
            Part::Text(", ["),
            Part::Run(*expr),
            Part::Text(if *svg { "], null, true)" } else { "])" }),
        ]),
    }
}

/// The end of a setter call: runtime-vapor's trailing SVG flag, if set.
fn svg_tail(svg: bool) -> &'static str {
    if svg {
        ", true)"
    } else {
        ")"
    }
}

/// One setter call, closed by `tail`.
fn record_setter<'c>(
    parts: &mut RunBuf<'c>,
    helper: &'static str,
    node: u32,
    attr: Option<&'c str>,
    expr: RunId,
    tail: &'static str,
) {
    parts.extend([
        Part::Text(helper),
        Part::Name(NameKind::Node, node),
        Part::Text(", "),
    ]);
    if let Some(attr) = attr {
        parts.extend([Part::Text("\""), Part::Text(attr), Part::Text("\", ")]);
    }
    parts.extend([Part::Run(expr), Part::Text(tail)]);
}

fn write_effect_group<'c>(
    graph: &super::graph::EffectGraph<'c>,
    group: EffectGroupId,
    allocator: &'c oxc_allocator::Allocator,
) -> Result<RunBuf<'c>, VueTargetRefusal> {
    use super::graph::{EffectWrapper, VaporEffectNode};
    let relations = graph.relations()?;
    let Some(VaporEffectNode::Group { indent, wrapper }) = relations.node(group) else {
        return Err(VueTargetRefusal::Unsupported {
            code: "invalid-effect-group",
            span: Span::new(0, 0),
        });
    };
    let indent = *indent;
    let mut parts = Vec::new();
    match wrapper {
        EffectWrapper::Once => {
            for (index, (effect, _)) in relations.edges(group).iter().enumerate() {
                if index > 0 {
                    parts.extend([Part::Text("\n"), Part::Text(indent)]);
                }
                let EffectId(row) = super::graph::effect_row(relations, *effect)?;
                parts.extend_from_slice(
                    &graph.records[row as usize]
                        .as_ref()
                        .expect("effect record")
                        .parts,
                );
            }
        }
        EffectWrapper::Memo { deps, cache } => {
            parts.extend([
                Part::Text("_renderEffect(() => _withMemo("),
                Part::Run(*deps),
                Part::Text(", () => {\n"),
            ]);
            for (effect, _) in relations.edges(group) {
                parts.extend([Part::Text(indent), Part::Text("  ")]);
                let EffectId(row) = super::graph::effect_row(relations, *effect)?;
                parts.extend_from_slice(
                    &graph.records[row as usize]
                        .as_ref()
                        .expect("effect record")
                        .parts,
                );
                parts.push(Part::Text("\n"));
            }
            parts.extend([
                Part::Text(indent),
                Part::Text("}, _cache, "),
                Part::Name(NameKind::Number, *cache),
                Part::Text("))"),
            ]);
        }
        EffectWrapper::Render => {
            let hoisted = super::repeated_reads::hoist(
                relations,
                &graph.records,
                group,
                &graph.tokens,
                allocator,
                indent,
            )?;
            parts.push(Part::Text("_renderEffect(() => "));
            if relations.edges(group).len() == 1 && hoisted.names.is_empty() {
                let EffectId(row) =
                    super::graph::effect_row(relations, relations.edges(group)[0].0)?;
                parts.extend_from_slice(
                    &graph.records[row as usize]
                        .as_ref()
                        .expect("effect record")
                        .parts,
                );
            } else {
                parts.push(Part::Text("{\n"));
                parts.extend(hoisted.declarations);
                for (effect, _) in relations.edges(group) {
                    let EffectId(row) = super::graph::effect_row(relations, *effect)?;
                    parts.extend([Part::Text(indent), Part::Text("  ")]);
                    super::repeated_reads::substitute(
                        graph.records[row as usize].as_ref().expect("effect record"),
                        &hoisted.names,
                        &mut parts,
                    );
                    parts.push(Part::Text("\n"));
                }
                parts.extend([Part::Text(indent), Part::Text("}")]);
            }
            parts.push(Part::Text(")"));
        }
    }
    Ok(parts)
}

fn write_run<'c>(
    builder: &mut EmissionBuilder<'c>,
    runs: &super::ops::Runs<'c>,
    graph: &super::graph::EffectGraph<'c>,
    allocator: &'c oxc_allocator::Allocator,
    run: RunId,
) -> Result<(), VueTargetRefusal> {
    let mut pending: Vec<(Part<'c>, &'c str)> = runs
        .get(run)
        .iter()
        .rev()
        .map(|part| (*part, "  "))
        .collect();
    while let Some((part, indent)) = pending.pop() {
        match part {
            Part::Text(text) => builder.push_borrowed(text),
            Part::Name(kind, number) => {
                builder.push_unmapped_fmt(format_args!("{}{number}", kind.prefix()))
            }
            Part::Source(span) => builder
                .push_source(span)
                .map_err(|error| VueTargetRefusal::Emit(error.into()))?,
            Part::Anchored(text, at) => builder.push_anchored(text, at),
            Part::Run(run) => {
                pending.extend(runs.get(run).iter().rev().map(|part| (*part, indent)))
            }
            Part::Indented(run, indent) => {
                pending.extend(runs.get(run).iter().rev().map(|part| (*part, indent)))
            }
            Part::LineBreak => {
                builder.push_borrowed("\n");
                builder.push_borrowed(indent);
            }
            Part::Effects(group) => pending.extend(
                write_effect_group(graph, group, allocator)?
                    .into_iter()
                    .rev()
                    .map(|part| (part, indent)),
            ),
        }
    }
    Ok(())
}
