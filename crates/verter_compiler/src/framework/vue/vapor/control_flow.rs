use super::state::VaporState;
use crate::framework::vue::semantic::VueBindingView;
use crate::template::binding::destructure::{
    collect_binding_destructure_paths, collect_destructure_paths, DestructureAccessor,
};
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::oxc::types::OxcParsedElement;
use oxc_ast::ast::*;
/// Official `getNegativeIfBranchShape`: a `v-else-if` is the ELSE_IF shape
/// and never NO_SCOPE; a terminal `v-else` carries its own static-ness for
/// FALSE_NO_SCOPE and its own block shape.
#[derive(Clone, Copy)]
pub(super) enum IfNegative {
    /// No `v-else`/`v-else-if`.
    None,
    /// Continuing `v-else-if`.
    Chain,
    /// Terminal `v-else`: whether its own branch is static, and whether its
    /// shape is MULTI_ROOT (see [`VaporCompiler::if_branch_multi_root`]).
    Terminal { is_static: bool, multi_root: bool },
}

/// Official `_createIf` 4th argument (`genIfFlags`/`genIfFlagNames`).
/// `None` when official omits it (`flags === 1`: bare v-if, positive
/// branch not NO_SCOPE-eligible, not nested where NO_SCOPE is disallowed).
///
/// No `v-once` `v-if` is reachable. A branch's shape is
/// SINGLE_ROOT (1) or MULTI_ROOT (2); a `v-else-if` false branch is ELSE_IF (3):
/// `(positive multi ? 2 : 1) | (negative shape << 2) |
/// (positive NO_SCOPE ? 32 : 0) | (negative NO_SCOPE ? 64 : 0) |
/// (slot_root ? 128 : 0) | (has_negative ? (own_if_index + 1) << 8 : 0)`; a
/// root of a slot block with no stable root is SLOT_ROOT
/// (`markSlotRootOperations`).
pub(super) fn compute_if_flags(
    positive_static: bool,
    positive_multi_root: bool,
    negative: IfNegative,
    own_if_index: Option<u32>,
    allow_no_scope: bool,
    slot_root: bool,
    is_production: bool,
) -> Option<String> {
    let has_negative = !matches!(negative, IfNegative::None);

    let (mut flags, positive_shape) = if positive_multi_root {
        (2, "TRUE_MULTI_ROOT")
    } else {
        (1, "TRUE_SINGLE_ROOT")
    };
    let mut names: Vec<String> = vec![positive_shape.to_string()];

    if has_negative {
        let (shape, name) = match negative {
            IfNegative::Chain => (3, "FALSE_ELSE_IF"),
            IfNegative::Terminal {
                multi_root: true, ..
            } => (2, "FALSE_MULTI_ROOT"),
            _ => (1, "FALSE_SINGLE_ROOT"),
        };
        flags |= shape << 2;
        names.push(name.to_string());
    }
    if allow_no_scope && positive_static {
        flags |= 32;
        names.push("TRUE_NO_SCOPE".to_string());
    }
    if let IfNegative::Terminal {
        is_static: negative_static,
        ..
    } = negative
    {
        if allow_no_scope && negative_static {
            flags |= 64;
            names.push("FALSE_NO_SCOPE".to_string());
        }
    }
    if slot_root {
        flags |= 128;
        names.push("SLOT_ROOT".to_string());
    }
    if has_negative {
        if let Some(idx) = own_if_index {
            flags |= (idx + 1) << 8;
            names.push(format!("KEYED_INDEX_{idx}"));
        }
    }

    if flags == 1 {
        return None;
    }

    Some(if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    })
}

/// Official `encodeIfBlockShape` under `shouldForceMultiRoot`: a `v-if`
/// directly inside a `<template v-for>` is MULTI_ROOT on both sides,
/// whatever its branches hold. The false side is ELSE_IF for a continuing
/// `v-else-if` and MULTI_ROOT otherwise, even with no `v-else`; no branch is
/// NO_SCOPE. A slot root is SLOT_ROOT and the keyed index follows a present
/// negative branch, as in [`compute_if_flags`].
pub(super) fn compute_forced_multi_root_if_flags(
    negative: IfNegative,
    own_if_index: Option<u32>,
    slot_root: bool,
    is_production: bool,
) -> String {
    let (false_shape, false_name) = match negative {
        IfNegative::Chain => (3, "FALSE_ELSE_IF"),
        _ => (2, "FALSE_MULTI_ROOT"),
    };
    let mut flags = 2 | (false_shape << 2);
    let mut names = vec!["TRUE_MULTI_ROOT".to_string(), false_name.to_string()];
    if slot_root {
        flags |= 128;
        names.push("SLOT_ROOT".to_string());
    }
    if !matches!(negative, IfNegative::None) {
        if let Some(index) = own_if_index {
            flags |= (index + 1) << 8;
            names.push(format!("KEYED_INDEX_{index}"));
        }
    }
    if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    }
}

/// Official loop flags consume the producer's returned block kind, including
/// through transparent wrappers. Components and fragments never use DOM-node ops.
/// `wrapped_rows` is independent of the returned kind: a `<template v-for>`
/// row wraps its content unless it is one plain element.
/// A root of a slot block with no stable root is SLOT_ROOT
/// (`markSlotRootOperations`).
pub(super) fn compute_for_flags(
    only_child: bool,
    kind: BlockKind,
    wrapped_rows: bool,
    slot_root: bool,
    is_production: bool,
) -> Option<String> {
    let mut flags: u32 = 0;
    let mut names: Vec<&str> = Vec::new();
    if only_child {
        flags |= 1;
        names.push("FAST_REMOVE");
    }
    match kind {
        BlockKind::Node => {
            flags |= 8;
            names.push("IS_SINGLE_NODE");
        }
        BlockKind::Component => {
            flags |= 2;
            names.push("IS_COMPONENT");
        }
        BlockKind::Fragment => {
            flags |= 16;
            names.push("IS_FRAGMENT");
        }
        BlockKind::Roots => {}
    }
    if slot_root {
        flags |= 32;
        names.push("SLOT_ROOT");
    }
    if wrapped_rows {
        flags |= 64;
        names.push("WRAPPED_ROWS");
    }
    if flags == 0 {
        return None;
    }
    Some(if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    })
}

/// Register the target's runtime helper for a shared binding accessor.
pub(super) fn register_destructure_import(
    accessor: &DestructureAccessor,
    out: &mut VaporState<'_>,
) {
    match accessor {
        DestructureAccessor::Path(_) => {}
        DestructureAccessor::Rest { .. } => out.use_helper(VaporHelper::GetRestElement),
        DestructureAccessor::Default { .. } => out.use_helper(VaporHelper::GetDefaultValue),
    }
}

fn resolve_default(
    expression: &Expression<'_>,
    source: &str,
    resolver: &VueBindingView<'_>,
    force_js: bool,
    annotation_offset: u32,
) -> String {
    use oxc_span::GetSpan;
    let span = expression.span();
    let text = &source[span.start as usize..span.end as usize];
    if !force_js {
        return resolver.resolve_simple_expr(text);
    }
    let skips = crate::strip_types::typescript::collect_ts_removal_spans_with_annotation_offset(
        expression,
        annotation_offset,
    )
    .into_iter()
    .map(|(start, end)| {
        let relative = verter_span::Span::new(start, end).to_relative(span.start);
        (relative.start, relative.end)
    })
    .collect::<Vec<_>>();
    let mut stripped = String::new();
    crate::template::binding::prefixed_expr::write_source_expr(
        &mut crate::template::binding::prefixed_expr::StringSink(&mut stripped),
        text,
        span.start,
        &skips,
    );
    resolver.resolve_simple_expr(&stripped)
}

/// Leaf-name → accessor list for a v-for VALUE position's destructuring
/// pattern, or `None` if it isn't an object/array pattern at all (bare
/// identifier — handled by the existing simple-ident path) or hits an
/// unsupported construct. Official only destructures the value position —
/// key/index stay identifier-only (the shared collector's eligibility).
fn destructure_value_paths(
    expr: &Expression<'_>,
    source: &str,
    resolver: &VueBindingView<'_>,
    force_js: bool,
    annotation_offset: u32,
) -> Option<Vec<(String, DestructureAccessor)>> {
    if !matches!(
        expr,
        Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
    ) {
        return None;
    }
    collect_destructure_paths(expr, |expression| {
        resolve_default(expression, source, resolver, force_js, annotation_offset)
    })
}

/// The v-for value (item) position's own expression — first element of a
/// parenthesized/sequence left-hand side (`(pattern, key) in …`), or the
/// bare expression itself for a single-position `v-for="item in items"`.
fn for_value_expr<'r, 'e>(left: &'r Expression<'e>) -> &'r Expression<'e> {
    let mut cur = left;
    while let Expression::ParenthesizedExpression(p) = cur {
        cur = &p.expression;
    }
    match cur {
        Expression::SequenceExpression(seq) => seq.expressions.first().unwrap_or(cur),
        other => other,
    }
}

/// The OXC-parsed v-for value pattern, if this element has a v-for with a
/// successfully-parsed left-hand side. `None` for no v-for / no OXC data /
/// a parse failure — callers fall back to the pre-existing raw-text path.
pub(super) fn v_for_value_pattern<'r, 'e>(
    oxc_el: Option<&'r OxcParsedElement<'e>>,
) -> Option<&'r Expression<'e>> {
    let left = oxc_el?.v_for.as_ref()?.parsed.result.left.as_ref()?;
    Some(for_value_expr(left))
}

/// The v-slot scoped-slot destructure leaves, if this `<template
/// v-slot="…">` wrapper's own param is an object/array PATTERN (never a
/// bare identifier — official only enters a fresh scope for a pattern; a
/// bare identifier keeps the pre-existing, already-correct raw-passthrough
/// behavior) AND every leaf resolves ([`collect_binding_destructure_paths`]
/// — a computed key or nested-pattern rest/default target bails the whole
/// element to `None`, same conservative fallback as the v-for value
/// position).
pub(super) fn v_slot_destructure_leaves<'e>(
    oxc_el: Option<&OxcParsedElement<'e>>,
    source: &str,
    resolver: &VueBindingView<'_>,
    force_js: bool,
) -> Option<Vec<(String, DestructureAccessor)>> {
    let slot = &oxc_el?.v_slot.as_ref()?.parsed;
    let annotation_offset = slot.offset();
    let params = slot.params()?;
    if params.items.len() != 1 {
        return None;
    }
    let pattern = &params.items[0].pattern;
    if !matches!(
        pattern,
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_)
    ) {
        return None;
    }
    collect_binding_destructure_paths(pattern, |expression| {
        resolve_default(expression, source, resolver, force_js, annotation_offset)
    })
}

/// Loop-variable rename map for the shared expression writer’s local hook —
/// official `itemVar = _for_item${depth}` + `buildDestructureIdMap` (rc.5).
/// `param_part` is [`helpers::parse_v_for_expression`]'s first return
/// (parens stripped); positions are value → key → index
/// ([`helpers::split_v_for_params`]).
///
/// A bare identifier is always renamed. The VALUE position (index 0) also
/// renames when it's a destructuring pattern [`destructure_value_paths`]
/// can fully resolve — each leaf identifier maps to
/// `_for_item{depth}.value<path>`. Key/index positions stay identifier-only
/// (official doesn't destructure them). `_` never gets an entry.
pub(super) fn build_for_scope_map(
    param_part: &str,
    depth: u32,
    value_pattern: Option<&Expression<'_>>,
    source: &str,
    resolver: &VueBindingView<'_>,
    force_js: bool,
    out: &mut VaporState<'_>,
) -> rustc_hash::FxHashMap<String, String> {
    use crate::template::binding::is_simple_ident;
    use crate::template::code_gen::shared::helpers::{push_u32, split_v_for_params};

    let mut map = rustc_hash::FxHashMap::default();
    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    for (i, (part, prefix)) in parts.iter().zip(prefixes.iter()).enumerate() {
        let Some(name) = part.map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "_" {
            continue;
        }
        let mut accessor_base = String::with_capacity(prefix.len() + 10);
        accessor_base.push_str(prefix);
        push_u32(&mut accessor_base, depth);
        accessor_base.push_str(".value");

        if is_simple_ident(name) {
            map.insert(name.to_string(), accessor_base);
            continue;
        }
        if i == 0 {
            if let Some(leaves) = value_pattern
                .and_then(|p| destructure_value_paths(p, source, resolver, force_js, 0))
            {
                for (leaf_name, accessor) in leaves {
                    register_destructure_import(&accessor, out);
                    map.insert(leaf_name, accessor.render(&accessor_base));
                }
            }
        }
    }
    map
}

/// Main-closure params: renamed `_for_item{depth}`… for each bare
/// identifier, or for the VALUE position when it's a destructuring pattern
/// [`destructure_value_paths`] can fully resolve — same eligibility as
/// [`build_for_scope_map`], computed independently since the two run at
/// different points in the walk (enter vs. leave). A position this pass
/// can't destructure stays as authored text (still valid JS).
pub(super) fn build_for_callback_params(
    param_part: &str,
    depth: u32,
    value_pattern: Option<&Expression<'_>>,
    source: &str,
    resolver: &VueBindingView<'_>,
    force_js: bool,
) -> String {
    use crate::template::binding::is_simple_ident;
    use crate::template::code_gen::shared::helpers::{push_u32, split_v_for_params};

    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    let mut pieces: Vec<String> = Vec::with_capacity(3);
    for (i, (part, prefix)) in parts.iter().zip(prefixes.iter()).enumerate() {
        let Some(name) = part.map(str::trim) else {
            break;
        };
        let renames = (is_simple_ident(name) && name != "_")
            || (i == 0
                && value_pattern
                    .and_then(|p| destructure_value_paths(p, source, resolver, force_js, 0))
                    .is_some());
        if renames {
            let mut renamed = String::with_capacity(prefix.len() + 3);
            renamed.push_str(prefix);
            push_u32(&mut renamed, depth);
            pieces.push(renamed);
        } else {
            pieces.push(name.to_string());
        }
    }
    pieces.join(", ")
}

use super::ops::{BlockKind, ClosedBlock, NameKind, Part, RunId};
use super::scope::{MergedConstructKind, VaporScope};
use super::VaporCompiler;
use crate::ast::types::ElementNodeConditionKind;
use crate::framework::vue::structure::{VueChildKind, VueCompileNodeId};
use crate::framework::vue::target::VueTargetRefusal;
use crate::template::code_gen::shared::helpers;
use crate::template::oxc::types::LexicalScopeId;
use crate::template::topology::VueTemplateNodeKind;

pub(super) struct IfBranch {
    condition: Option<RunId>,
    body: ClosedBlock,
    own_if_index: Option<u32>,
    /// Official `getBlockShape`: MULTI_ROOT rather than SINGLE_ROOT.
    multi_root: bool,
}

pub(super) struct IfChain {
    outer_ref: u32,
    branches: Vec<IfBranch>,
    last_branch: VueCompileNodeId,
    pub(super) target_stack_index: Option<usize>,
}

/// A loop's source getter, its reactive callback parameters, its aliases as
/// authored, and its key getter.
pub(super) struct LoopParts<'c> {
    pub(super) source: RunId,
    pub(super) callback: &'c str,
    pub(super) aliases: &'c str,
    pub(super) key: Option<RunId>,
}

/// Structural wrappers and their payload each own a distinct reservation.
#[derive(Clone, Copy)]
pub(super) struct ConstructRefs {
    pub(super) condition: Option<u32>,
    pub(super) for_loop: Option<u32>,
    pub(super) slot: Option<u32>,
    /// The element is a `<template v-slot>`: its `v-for` repeats slot
    /// functions, so its rows are not loop rows.
    pub(super) loops_slots: bool,
}

impl<'c> VaporCompiler<'c> {
    pub(super) fn leave_for_scope(&mut self, refs: ConstructRefs) {
        if refs.for_loop.is_some() {
            self.for_scope_depth -= 1;
            self.renames.pop().expect("balanced loop alias frame");
        }
    }

    /// Official `getBlockShape` for a `v-if` branch: one root only when its
    /// content is exactly one element (a component, slot outlet or
    /// `<template>` included) without its own `v-if`. Text, a comment,
    /// several children, no content or a nested `v-if` make the branch
    /// MULTI_ROOT. A branch on any element other than `<template>` is that
    /// element alone.
    fn if_branch_multi_root(&self, node: VueCompileNodeId) -> bool {
        let topology = self.inputs.topology;
        if topology.kind(self.inputs.structure.template_id(node)) != VueTemplateNodeKind::Template {
            return false;
        }
        let mut content = self.inputs.structure.children(node).iter().filter(|child| {
            !matches!(
                child.kind,
                VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
            )
        });
        match (content.next(), content.next()) {
            (Some(only), None) => topology
                .element(self.inputs.structure.template_id(only.node))
                .is_none_or(|element| {
                    element.v_condition.as_ref().is_some_and(|condition| {
                        matches!(condition.kind, ElementNodeConditionKind::If)
                    })
                }),
            _ => true,
        }
    }

    /// Vue's `WRAPPED_ROWS`: a `<template v-for>` outside a `<Transition>`
    /// whose content is not exactly one element without its own `v-if` or
    /// `v-for` (text, a comment, several children, or one construct).
    fn wraps_rows(&self, node: VueCompileNodeId) -> bool {
        let topology = self.inputs.topology;
        let template = self.inputs.structure.template_id(node);
        if topology.kind(template) != VueTemplateNodeKind::Template {
            return false;
        }
        let in_transition = topology.parent(template).is_some_and(|parent| {
            topology.kind(parent) == VueTemplateNodeKind::Component
                && self
                    .inputs
                    .structure
                    .tag_name(self.inputs.structure.compile_id(parent))
                    .is_some_and(|tag| {
                        tag.eq_ignore_ascii_case("transition")
                            || tag.eq_ignore_ascii_case("vaportransition")
                    })
        });
        if in_transition {
            return false;
        }
        let mut content = self.inputs.structure.children(node).iter().filter(|child| {
            !matches!(
                child.kind,
                VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
            )
        });
        match (content.next(), content.next()) {
            (Some(only), None) => topology
                .element(self.inputs.structure.template_id(only.node))
                .is_none_or(|element| {
                    element.v_for.is_some()
                        || element.v_condition.as_ref().is_some_and(|condition| {
                            matches!(condition.kind, ElementNodeConditionKind::If)
                        })
                }),
            _ => true,
        }
    }

    /// The names a loop element's own `v-for` declares, from the
    /// parser-owned scope inventory: its value, key and index aliases,
    /// including every name a destructuring pattern binds. The element's
    /// props scope holds them; its own `v-slot` parameters are excluded, as
    /// only its children see those.
    fn loop_declared_names(&self, node: VueCompileNodeId) -> impl Iterator<Item = &'c str> + '_ {
        let expressions = self.inputs.semantic.expressions();
        let template = self.inputs.structure.template_id(node);
        let enclosing = self
            .inputs
            .topology
            .syntax(template)
            .map_or(LexicalScopeId::ROOT, |syntax| {
                expressions.scope_of(syntax, self.inputs.topology.ast())
            });
        let own = self
            .element_of(node)
            .1
            .map_or(enclosing, |element| element.props_scope);
        std::iter::successors(Some(own), move |&frame| {
            Some(expressions.scopes.parent(frame))
        })
        .take_while(move |&frame| frame != enclosing && frame != LexicalScopeId::ROOT)
        .flat_map(move |frame| expressions.scopes.own_names(frame).iter().copied())
    }

    /// Before the walk renders a child, a pending conditional chain that this
    /// child does not continue is placed, so its placeholder and insertion
    /// position precede the child. A transparent wrapper's text run ends at
    /// its next non-text child and starts at its first text child.
    pub(super) fn begin_child(
        &mut self,
        node: VueCompileNodeId,
        text_like: bool,
    ) -> Result<(), VueTargetRefusal> {
        let info = self.child_info[node.index()];
        if info.kind.is_none() {
            return Ok(());
        }
        if !text_like {
            self.close_text_root(self.span_of(node))?;
        }
        if !info.continuation {
            self.flush_if_chain()?;
        }
        if text_like {
            self.open_text_root();
        }
        Ok(())
    }

    /// An element body already carries its closure return; construct payloads
    /// acquire one only when a structural wrapper consumes them.
    fn construct_body(&mut self, root: ClosedBlock) -> ClosedBlock {
        ClosedBlock {
            run: self.state.runs.commit(&mut vec![
                Part::Text("    "),
                Part::Indented(root.run, "    "),
                Part::Text("\n    return "),
                Part::Name(NameKind::Node, root.node_ref),
                Part::Text("\n"),
            ]),
            ..root
        }
    }

    pub(super) fn close_structural_scope(
        &mut self,
        node: VueCompileNodeId,
        scope: VaporScope<'c>,
        dynamic_text: bool,
        refs: ConstructRefs,
    ) -> Result<(), VueTargetRefusal> {
        self.leave_for_scope(refs);
        let span = self.span_of(node);
        let body = if scope.is_transparent_wrapper && !scope.roots.is_empty() {
            self.close_roots(scope, span)?
        } else {
            let root = self.template_single_root && self.depth == 0 && refs.for_loop.is_none();
            self.close_block(scope, dynamic_text, "    ", root, true, span)?
        };
        self.finish_structural_body(node, body, refs)
    }

    pub(super) fn finish_construct(
        &mut self,
        node: VueCompileNodeId,
        root: ClosedBlock,
        kind: MergedConstructKind,
        refs: ConstructRefs,
    ) -> Result<(), VueTargetRefusal> {
        self.leave_for_scope(refs);
        if self.element_of(node).0.v_condition.is_some() || refs.for_loop.is_some() {
            let body = self.construct_body(root);
            self.finish_structural_body(node, body, refs)
        } else {
            self.place_construct(node, root, kind);
            Ok(())
        }
    }

    fn finish_structural_body(
        &mut self,
        node: VueCompileNodeId,
        mut body: ClosedBlock,
        refs: ConstructRefs,
    ) -> Result<(), VueTargetRefusal> {
        let conditional = self.element_of(node).0.v_condition.is_some();
        if let Some(outer) = refs.for_loop {
            let root = self.build_loop(node, body, outer)?;
            if !conditional {
                self.place_construct(node, root, MergedConstructKind::Other);
                return Ok(());
            }
            body = self.construct_body(root);
        }
        self.handle_condition(node, body, refs.condition)
    }

    pub(super) fn place_construct(
        &mut self,
        node: VueCompileNodeId,
        root: ClosedBlock,
        kind: MergedConstructKind,
    ) {
        if let Some(index) = self.element_stack.len().checked_sub(1) {
            self.merge_into_stack_index(index, node, root, kind);
        } else {
            self.push_construct_root(root);
        }
    }

    fn push_construct_root(&mut self, mut root: ClosedBlock) {
        root.run = self.state.runs.commit(&mut vec![
            Part::Text("  "),
            Part::Indented(root.run, "  "),
            Part::Text("\n"),
        ]);
        self.state.roots.push(root);
    }

    pub(super) fn construct(
        &mut self,
        node_ref: u32,
        parts: &mut Vec<Part<'c>>,
        kind: BlockKind,
    ) -> ClosedBlock {
        ClosedBlock {
            node_ref,
            run: self.state.runs.commit(parts),
            template_idx: None,
            is_static: false,
            kind,
        }
    }

    pub(super) fn handle_condition(
        &mut self,
        node: VueCompileNodeId,
        body: ClosedBlock,
        construct_ref: Option<u32>,
    ) -> Result<(), VueTargetRefusal> {
        let (element, oxc) = self.element_of(node);
        let condition = element.v_condition.as_ref().expect("condition");
        let expr = if matches!(condition.kind, ElementNodeConditionKind::Else) {
            None
        } else if let (Some(start), Some(end)) =
            (condition.prop.value_start, condition.prop.value_end)
        {
            Some(self.record_expression(
                &self.inputs.source.text[start as usize..end as usize],
                start,
                oxc.and_then(|oxc| oxc.condition.as_ref()),
            ))
        } else {
            Some(self.state.runs.commit(&mut vec![Part::Text("true")]))
        };
        let own_if_index = expr.map(|_| {
            let index = self.if_index_counter;
            self.if_index_counter += 1;
            index
        });
        let branch = IfBranch {
            condition: expr,
            body,
            own_if_index,
            multi_root: self.if_branch_multi_root(node),
        };
        match condition.kind {
            ElementNodeConditionKind::If => {
                self.flush_if_chain()?;
                self.pending_if = Some(IfChain {
                    outer_ref: construct_ref.expect("if construct id"),
                    branches: vec![branch],
                    last_branch: node,
                    target_stack_index: self.element_stack.len().checked_sub(1),
                });
            }
            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                if let Some(chain) = self.pending_if.as_mut() {
                    chain.branches.push(branch);
                    chain.last_branch = node;
                }
                if matches!(condition.kind, ElementNodeConditionKind::Else) {
                    self.flush_if_chain()?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn flush_if_chain(&mut self) -> Result<(), VueTargetRefusal> {
        let Some(chain) = self.pending_if.take() else {
            return Ok(());
        };
        // Official `shouldForceMultiRoot`: the chain's parent is a
        // `<template v-for>`.
        let force_multi_root = self
            .inputs
            .topology
            .parent(self.inputs.structure.template_id(chain.last_branch))
            .is_some_and(|parent| {
                self.inputs.topology.kind(parent) == VueTemplateNodeKind::Template
                    && self
                        .inputs
                        .topology
                        .element(parent)
                        .is_some_and(|element| element.v_for.is_some())
            });
        let mut parts = vec![
            Part::Text("const "),
            Part::Name(NameKind::Node, chain.outer_ref),
            Part::Text(" = "),
        ];
        // Build the nested negative from the end, keeping each body as a run.
        let mut negative = None;
        for (index, branch) in chain.branches.iter().enumerate().rev() {
            let mut current = Vec::new();
            if let Some(condition) = branch.condition {
                current.extend([
                    Part::Text("_createIf(() => ("),
                    Part::Run(condition),
                    Part::Text("), () => {\n"),
                    Part::Run(branch.body.run),
                ]);
                let shape = match chain.branches.get(index + 1) {
                    None => IfNegative::None,
                    Some(next) if next.condition.is_some() => IfNegative::Chain,
                    Some(next) => IfNegative::Terminal {
                        is_static: next.body.is_static,
                        multi_root: next.multi_root,
                    },
                };
                let is_production = self.inputs.options.is_production;
                let if_flags = |slot_root: bool| {
                    if force_multi_root {
                        Some(compute_forced_multi_root_if_flags(
                            shape,
                            branch.own_if_index,
                            slot_root,
                            is_production,
                        ))
                    } else {
                        compute_if_flags(
                            branch.body.is_static,
                            branch.multi_root,
                            shape,
                            branch.own_if_index,
                            self.block_depth == 0,
                            slot_root,
                            is_production,
                        )
                    }
                };
                let (flags, marked) = (if_flags(false), if_flags(true));
                let separator = if let Some(next) = negative {
                    current.extend([Part::Text("  }, () => "), Part::Run(next)]);
                    ", "
                } else {
                    current.push(Part::Text("  }"));
                    ", null, "
                };
                let allocator = self.inputs.allocator;
                let mut plain = flags.map_or_else(Vec::new, |flags| {
                    vec![
                        Part::Text(separator),
                        Part::Text(allocator.alloc_str(&flags)),
                    ]
                });
                let mut slot_root = marked.map_or_else(Vec::new, |flags| {
                    vec![
                        Part::Text(separator),
                        Part::Text(allocator.alloc_str(&flags)),
                    ]
                });
                let flags = self
                    .state
                    .slot_root_flags(chain.outer_ref, &mut plain, &mut slot_root);
                current.extend([Part::Run(flags), Part::Text(")")]);
            } else {
                current.extend([
                    Part::Text("{\n"),
                    Part::Run(branch.body.run),
                    Part::Text("}"),
                ]);
            }
            negative = Some(self.state.runs.commit(&mut current));
        }
        // `markSlotRootIf` marks every branch of the chain in turn.
        for branch in &chain.branches {
            let roots = self.state.block_roots(&branch.body);
            self.state.slot_root_block(chain.outer_ref, roots);
        }
        if let Some(run) = negative {
            parts.push(Part::Run(run));
        }
        let root = self.construct(chain.outer_ref, &mut parts, BlockKind::Fragment);
        self.state.use_helper(VaporHelper::CreateIf);
        if let Some(index) = chain.target_stack_index {
            self.merge_into_stack_index(index, chain.last_branch, root, MergedConstructKind::Other);
        } else {
            self.push_construct_root(root);
        }
        Ok(())
    }

    /// The pieces every loop helper writes for a `v-for` whose alias frame
    /// the walk has left: its source getter, the callback parameters that read
    /// the aliases reactively, the aliases as authored, and its key getter.
    pub(super) fn loop_parts(&mut self, node: VueCompileNodeId) -> LoopParts<'c> {
        let (element, oxc) = self.element_of(node);
        let prop = element.v_for.as_ref().expect("loop");
        let source = self.inputs.source.text;
        let full = match (prop.value_start, prop.value_end) {
            (Some(start), Some(end)) => &source[start as usize..end as usize],
            _ => "",
        };
        let (params, iterable) = helpers::parse_v_for_expression(full);
        let start = prop.value_start.unwrap_or(0)
            + (iterable.as_ptr() as usize - full.as_ptr() as usize) as u32;
        // The source is outside this loop's alias frame; only outer renames apply.
        let trimmed = iterable.trim();
        let offset = start + (trimmed.as_ptr() as usize - iterable.as_ptr() as usize) as u32;
        let iterable_run =
            self.record_loop_source(trimmed, offset, oxc.and_then(|oxc| oxc.v_for.as_ref()));
        let callback = build_for_callback_params(
            params,
            self.for_scope_depth,
            v_for_value_pattern(oxc),
            source,
            &self.resolver,
            self.inputs.options.force_js,
        );
        let key_attr = self.inputs.structure.find_attr(
            node,
            crate::framework::vue::structure::VueAttrKind::Bind,
            "key",
        );
        let key = key_attr.and_then(|attr| attr.value_span).map(|key| {
            let parsed = crate::template::binding::prefixed_expr::find_prop_oxc_exp(
                oxc,
                key_attr.expect("key attribute").prop_index as usize,
            );
            self.record_with_authored_aliases(
                node,
                &source[key.start as usize..key.end as usize],
                key.start,
                parsed,
            )
        });
        LoopParts {
            source: iterable_run,
            callback: self.inputs.allocator.alloc_str(&callback),
            aliases: self.inputs.allocator.alloc_str(params),
            key,
        }
    }

    /// An expression a loop callback evaluates with this loop's aliases as
    /// authored parameters: every name the loop's own scope frames declare
    /// stays as written, shadowing any enclosing loop's renamed alias of the
    /// same name, while setup reads and enclosing aliases resolve like every
    /// other expression.
    pub(super) fn record_with_authored_aliases(
        &mut self,
        node: VueCompileNodeId,
        text: &str,
        start: u32,
        parsed: Option<&crate::template::oxc::types::OxcParsedExpression<'c>>,
    ) -> RunId {
        let shadowed: rustc_hash::FxHashMap<String, String> = self
            .loop_declared_names(node)
            .filter(|name| self.renames.iter().any(|frame| frame.contains_key(*name)))
            .map(|name| (name.to_owned(), name.to_owned()))
            .collect();
        self.renames.push(shadowed);
        let run = self.record_expression(text, start, parsed);
        self.renames.pop();
        run
    }

    pub(super) fn build_loop(
        &mut self,
        node: VueCompileNodeId,
        body: ClosedBlock,
        outer: u32,
    ) -> Result<ClosedBlock, VueTargetRefusal> {
        let LoopParts {
            source,
            callback,
            aliases,
            key,
        } = self.loop_parts(node);
        let mut parts = vec![
            Part::Text("const "),
            Part::Name(NameKind::Node, outer),
            Part::Text(" = _createFor(() => ("),
            Part::Run(source),
            Part::Text("), ("),
            Part::Text(callback),
            Part::Text(") => {\n"),
            Part::Run(body.run),
            Part::Text("  }"),
        ];
        if let Some(key) = key {
            parts.extend([
                Part::Text(", ("),
                Part::Text(aliases),
                Part::Text(") => ("),
                Part::Run(key),
                Part::Text(")"),
            ]);
        }
        let parent = self
            .inputs
            .topology
            .parent(self.inputs.structure.template_id(node));
        let parent_component_scope = parent.is_some_and(|parent| {
            matches!(
                self.inputs.topology.kind(parent),
                crate::template::topology::VueTemplateNodeKind::Component
                    | crate::template::topology::VueTemplateNodeKind::SlotOutlet
            ) || self
                .inputs
                .topology
                .element(parent)
                .is_some_and(|element| element.v_slot.is_some())
        });
        let only_child = !parent_component_scope
            && parent.is_some_and(|parent| {
                self.inputs.topology.kind(parent)
                    != crate::template::topology::VueTemplateNodeKind::Root
            })
            && self.is_sole_meaningful_child(node);
        let for_flags = |slot_root: bool| {
            compute_for_flags(
                only_child,
                body.kind,
                self.wraps_rows(node),
                slot_root,
                self.inputs.options.is_production,
            )
        };
        let (flags, marked) = (for_flags(false), for_flags(true));
        let allocator = self.inputs.allocator;
        let arguments = |flags: Option<String>| {
            let mut parts = Vec::new();
            if let Some(flags) = flags {
                if key.is_none() {
                    parts.push(Part::Text(", undefined"));
                }
                parts.extend([Part::Text(", "), Part::Text(allocator.alloc_str(&flags))]);
            }
            parts
        };
        let (mut plain, mut slot_root) = (arguments(flags), arguments(marked));
        let flags = self
            .state
            .slot_root_flags(outer, &mut plain, &mut slot_root);
        // `markSlotRootFor` marks the loop's row in turn.
        let roots = self.state.block_roots(&body);
        self.state.slot_root_block(outer, roots);
        parts.extend([Part::Run(flags), Part::Text(")")]);
        self.state.use_helper(VaporHelper::CreateFor);
        Ok(self.construct(outer, &mut parts, BlockKind::Fragment))
    }
}
