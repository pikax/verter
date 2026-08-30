//! Read-only LSP views over the registered carrier inventory.
//!
//! Geometry in this module is copied from `CarrierBlockInventory`; source text
//! is sliced only through validated spans. No carrier delimiter is searched.

use verter_language::parse_artifact::carrier_inventory::{
    AttributeValue, CarrierAttribute, CarrierBlock, DirectiveArgument, MarkupElementKind,
    MarkupElementSyntax, MarkupNodeKind, MarkupSyntaxNode, SectionRole, SourceSpan,
    SvelteAwaitInlineBranch, SvelteClauseHead, SvelteControlBlockHead, SvelteStandaloneTagFamily,
    SyntaxTermination, TaggedSyntax,
};
use verter_session::carrier_publication_store::{
    ArtifactAttributeRef, FrameworkBlockRef, RegisteredFileStructure,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttr {
    pub attribute_ref: ArtifactAttributeRef,
    pub name: String,
    pub value: Option<String>,
    pub name_start: u32,
    pub name_end: u32,
    pub value_start: Option<u32>,
    pub value_end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierBlockView {
    pub block_ref: FrameworkBlockRef,
    pub tag_name: String,
    pub open_tag_start: u32,
    pub open_tag_end: u32,
    pub close_tag_start: u32,
    pub close_tag_end: u32,
    pub opening_name_start: u32,
    pub opening_name_end: u32,
    pub attribute_insertion_anchor: u32,
    pub attrs_raw: String,
    pub attributes: Vec<ParsedAttr>,
}

impl CarrierBlockView {
    pub fn content_range(&self) -> (u32, u32) {
        (self.open_tag_end, self.close_tag_start)
    }

    pub fn attr(&self, name: &str) -> Option<Option<&str>> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_deref())
    }

    pub fn is_setup(&self) -> bool {
        self.tag_name == "script" && self.attr("setup").is_some()
    }

    pub fn lang(&self) -> Option<&str> {
        self.attr("lang").flatten()
    }

    pub fn is_scoped(&self) -> bool {
        self.attr("scoped").is_some()
    }

    pub fn is_module(&self) -> bool {
        self.attr("module").is_some()
    }

    pub fn attrs(&self) -> Option<&str> {
        self.attr("attributes")
            .flatten()
            .or_else(|| self.attr("attrs").flatten())
    }
}

/// One markup open-tag fact projected from the registered inventory's markup
/// arena: the parser-identified opening span (+ authored name) of an
/// element-like node — `Element`, `Recovered`, or `Unknown` — plus the nearest
/// element-like ancestor for typed parent walks. Geometry is copied from the
/// arena; no carrier delimiter is searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupOpenTagFact {
    /// Authored tag name, when the parser retained one.
    pub name: Option<String>,
    /// Parser-identified opening-tag span (`<tag …>` inclusive of delimiters).
    pub opening_start: u32,
    pub opening_end: u32,
    /// End of the tag-name token inside the opening span.
    pub name_end: u32,
    /// Full node span (opening + content + closing).
    pub full_start: u32,
    pub full_end: u32,
    /// Nearest element-like ancestor, as an index into the projected vec.
    pub parent: Option<usize>,
}

/// Project every element-like markup node (element / recovered / unknown with
/// a retained opening span) into [`MarkupOpenTagFact`]s, preserving arena
/// parent relations collapsed to the nearest element-like ancestor.
pub fn project_markup_open_tags(structure: &RegisteredFileStructure) -> Vec<MarkupOpenTagFact> {
    let inventory = structure.inventory();
    let nodes = inventory.markup().nodes();
    let mut arena_to_fact: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut facts = Vec::new();
    for (arena_index, node) in nodes.iter().enumerate() {
        let projected = match node.kind() {
            MarkupNodeKind::Element(element) => {
                let full = element.full_span;
                Some((
                    inventory
                        .slice(element.authored_name)
                        .ok()
                        .map(str::to_string),
                    element.opening_span.start,
                    element.opening_span.end,
                    element.opening_name_span.end,
                    full.start,
                    full.end,
                ))
            }
            MarkupNodeKind::Recovered {
                opening_span,
                opening_name_span,
                full_span,
                ..
            }
            | MarkupNodeKind::Unknown {
                opening_span,
                opening_name_span,
                full_span,
                ..
            } => opening_span.map(|opening| {
                (
                    opening_name_span.and_then(|span| {
                        inventory
                            .source_spaces()
                            .first()
                            .and_then(|space| {
                                space.bytes().get(span.start as usize..span.end as usize)
                            })
                            .map(str::to_string)
                    }),
                    opening.start,
                    opening.end,
                    opening_name_span.map_or(opening.start, |span| span.end),
                    full_span.start,
                    full_span.end,
                )
            }),
            _ => None,
        };
        let Some((name, opening_start, opening_end, name_end, full_start, full_end)) = projected
        else {
            continue;
        };
        let parent = {
            let mut ancestor = node.parent;
            loop {
                match ancestor {
                    Some(id) => match arena_to_fact.get(id.get() as usize).copied().flatten() {
                        Some(fact_index) => break Some(fact_index),
                        None => {
                            ancestor = nodes.get(id.get() as usize).and_then(|node| node.parent)
                        }
                    },
                    None => break None,
                }
            }
        };
        arena_to_fact[arena_index] = Some(facts.len());
        facts.push(MarkupOpenTagFact {
            name,
            opening_start,
            opening_end,
            name_end,
            full_start,
            full_end,
            parent,
        });
    }
    facts
}

/// Innermost fact whose parser-identified OPENING span contains `offset`.
pub fn markup_open_tag_at(facts: &[MarkupOpenTagFact], offset: u32) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| offset >= fact.opening_start && offset < fact.opening_end)
        .max_by_key(|(_, fact)| fact.opening_start)
        .map(|(index, _)| index)
}

/// Innermost fact whose FULL node span contains `offset` (for ancestor walks
/// from content positions).
pub fn markup_element_at(facts: &[MarkupOpenTagFact], offset: u32) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| offset >= fact.full_start && offset < fact.full_end)
        .min_by_key(|(_, fact)| fact.full_end - fact.full_start)
        .map(|(index, _)| index)
}

/// Nearest parser-owned component element containing `offset`.
///
/// The walk starts from the innermost markup arena node whose full span owns
/// the cursor and follows only parser parent links. Recovered/unknown nodes and
/// native elements never mint a tag identity; an authored component name is
/// returned only from a real [`MarkupNodeKind::Element`] classified as a
/// component. No carrier source text is searched.
pub fn nearest_component_ancestor_tag(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<String> {
    let inventory = structure.inventory();
    let nodes = inventory.markup().nodes();
    let mut current = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            let span = node.kind().full_span();
            offset >= span.start && offset < span.end
        })
        .min_by_key(|(_, node)| {
            let span = node.kind().full_span();
            span.end.saturating_sub(span.start)
        })
        .map(|(index, _)| index)?;

    loop {
        let node = nodes.get(current)?;
        if let MarkupNodeKind::Element(element) = node.kind() {
            if element.kind == MarkupElementKind::Component {
                return inventory
                    .slice(element.authored_name)
                    .ok()
                    .map(str::to_string);
            }
        }
        current = node.parent?.get() as usize;
    }
}

/// One parser-identified markup comment interior from the registered arena.
/// `interior_start` is the end of the `<!--` opener; `end` is the parser-owned
/// node end. An `open_ended` comment (no closer retained) extends through the
/// parser-decided recovery end. Geometry is copied from the arena; no
/// delimiter is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkupCommentFact {
    pub interior_start: u32,
    pub end: u32,
    pub open_ended: bool,
}

/// Project every markup comment node into [`MarkupCommentFact`]s.
pub fn project_markup_comment_facts(structure: &RegisteredFileStructure) -> Vec<MarkupCommentFact> {
    structure
        .inventory()
        .markup()
        .nodes()
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::Comment {
                opening_span,
                closing_span,
                full_span,
                ..
            } => Some(MarkupCommentFact {
                interior_start: opening_span.end,
                end: full_span.end,
                open_ended: closing_span.is_none(),
            }),
            _ => None,
        })
        .collect()
}

/// Whether `offset` sits inside a parser-identified comment INTERIOR — past
/// the opener and before the node end (an open-ended comment claims its end
/// position too, so typing at EOF inside `<!-- …` stays suppressed).
pub fn offset_in_markup_comment(facts: &[MarkupCommentFact], offset: u32) -> bool {
    facts.iter().any(|fact| {
        offset >= fact.interior_start
            && (offset < fact.end || (fact.open_ended && offset == fact.end))
    })
}

/// Parser-owned component attribute-name context at `offset`.
///
/// This is deliberately stricter than the general cursor classifier. Only a
/// real component [`MarkupNodeKind::Element`] can establish the authored tag
/// identity, and the cursor must be in the opening tag after its name, at an
/// attribute head/name, or in a parser-owned gap before the insertion anchor.
/// Value bodies, directive arguments/modifiers, spreads, attaches, and their
/// inclusive end positions are rejected so value typing cannot accidentally
/// capture component-contract authority. Recovered/unknown nodes fail closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredComponentAttributeNameContext {
    ExactAttribute { tag: String },
    InexactUnclosedOpening { tag: String },
}

impl AuthoredComponentAttributeNameContext {
    pub fn tag(&self) -> &str {
        match self {
            Self::ExactAttribute { tag } | Self::InexactUnclosedOpening { tag } => tag,
        }
    }
}

pub fn authored_component_attribute_name_context(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<AuthoredComponentAttributeNameContext> {
    fn contains_inclusive(span: SourceSpan, offset: u32) -> bool {
        offset >= span.start && offset <= span.end
    }

    fn value_contains_inclusive(value: &AttributeValue, offset: u32) -> bool {
        match value {
            AttributeValue::Missing => false,
            AttributeValue::Static { value_span, .. }
            | AttributeValue::Expression {
                full_span: value_span,
                ..
            }
            | AttributeValue::Mixed {
                full_span: value_span,
                ..
            } => contains_inclusive(*value_span, offset),
        }
    }

    fn directive_argument_contains_inclusive(argument: &DirectiveArgument, offset: u32) -> bool {
        match argument {
            DirectiveArgument::None => false,
            DirectiveArgument::Static { name } => contains_inclusive(name.name_span, offset),
            DirectiveArgument::Dynamic { full_span, .. } => contains_inclusive(*full_span, offset),
        }
    }

    let inventory = structure.inventory();
    inventory
        .markup()
        .nodes()
        .iter()
        .filter_map(|node| {
            let (opening_start, context) = match node.kind() {
                MarkupNodeKind::Element(element) => {
                    if element.kind != MarkupElementKind::Component {
                        return None;
                    }
                    let unclosed_attribute_name = element.attributes.iter().any(|attribute| {
                        matches!(
                            attribute,
                            CarrierAttribute::Named {
                                name,
                                value: AttributeValue::Missing,
                                ..
                            } if offset >= name.authored.span.start
                                && offset <= name.authored.span.end
                        )
                    });
                    let unclosed_eof_gap = offset == structure.source().bytes().len() as u32
                        && offset == element.full_span.end
                        && offset > element.opening_name_span.end
                        && element.attributes.is_empty();
                    if matches!(element.termination, SyntaxTermination::UnclosedEof)
                        && (unclosed_attribute_name || unclosed_eof_gap)
                    {
                        return Some((
                            element.opening_name_span.start.saturating_sub(1),
                            AuthoredComponentAttributeNameContext::InexactUnclosedOpening {
                                tag: inventory.slice(element.authored_name).ok()?.to_string(),
                            },
                        ));
                    }
                    if offset < element.opening_name_span.end
                        || offset > element.attribute_insertion_anchor.start
                    {
                        return None;
                    }
                    for attribute in element.attributes.iter() {
                        match attribute {
                            CarrierAttribute::Named { value, .. }
                                if value_contains_inclusive(value, offset) =>
                            {
                                return None;
                            }
                            CarrierAttribute::Directive {
                                argument,
                                modifiers,
                                value,
                                ..
                            } if directive_argument_contains_inclusive(argument, offset)
                                || modifiers.iter().any(|modifier| {
                                    contains_inclusive(modifier.full_span, offset)
                                })
                                || value_contains_inclusive(value, offset) =>
                            {
                                return None;
                            }
                            CarrierAttribute::Spread { full_span, .. }
                            | CarrierAttribute::Attach { full_span, .. }
                                if contains_inclusive(*full_span, offset) =>
                            {
                                return None;
                            }
                            _ => {}
                        }
                    }
                    (
                        element.opening_span.start,
                        AuthoredComponentAttributeNameContext::ExactAttribute {
                            tag: inventory.slice(element.authored_name).ok()?.to_string(),
                        },
                    )
                }
                _ => return None,
            };
            Some((opening_start, context))
        })
        .max_by_key(|(opening_start, _)| *opening_start)
        .map(|(_, context)| context)
}

/// Parser-owned Svelte head position at the edit cursor.
///
/// `RenderCallee` intentionally covers the whole in-progress expression for
/// completion recovery. It is not an authored-definition identity; navigation
/// authority must use [`svelte_static_render_callee_span_at`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteHeadCursorFact {
    SnippetName,
    RenderCallee,
}

/// Classify a Svelte snippet/render head only from parser-minted family and
/// payload spans. The inclusive end admits the ordinary edit cursor directly
/// after an empty or in-progress identifier without searching source text.
pub fn svelte_head_cursor_fact(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<SvelteHeadCursorFact> {
    structure
        .inventory()
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::SvelteControlBlock(block) => match &block.head {
                SvelteControlBlockHead::Snippet { name_span, .. }
                    if offset >= name_span.start.saturating_sub(1) && offset <= name_span.end =>
                {
                    Some(SvelteHeadCursorFact::SnippetName)
                }
                _ => None,
            },
            MarkupNodeKind::SvelteStandaloneTag(tag)
                if tag.family == SvelteStandaloneTagFamily::Render
                    && tag.expression_span.is_some_and(|span| {
                        offset >= span.start.saturating_sub(1) && offset <= span.end
                    }) =>
            {
                Some(SvelteHeadCursorFact::RenderCallee)
            }
            _ => None,
        })
}

/// Exact authored token for a direct static `{@render name(...)}` callee.
///
/// The registered expression span proves the Svelte render family and bounds
/// all slicing. Admission is deliberately fail-closed: member/dynamic callees,
/// argument identifiers, and syntax requiring a second parser retain provider
/// navigation rather than minting native source authority.
pub fn svelte_static_render_callee_span_at(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<SourceSpan> {
    svelte_static_render_callee_at(structure, offset).map(|(_, span)| span)
}

/// Exact parser-owned declaration for a direct static local snippet render.
///
/// The inward-to-outward walk follows Svelte's branch topology: a clause sees
/// declarations in that clause and genuinely outer scopes, never declarations
/// in its controller's main branch. Parser-owned bindings stop outer lookup;
/// duplicate declarations in one admitted scope fail closed instead of
/// inventing an identity. This lookup deliberately does not consult template
/// analysis, so it remains available in the post-edit window where registered
/// structure is current but BUILD/template analysis has not been republished.
pub fn svelte_local_render_snippet_definition_at(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<SourceSpan> {
    match svelte_render_lexical_visibility_at(structure, offset)? {
        SvelteRenderLexicalVisibility::LocalSnippet(span) => Some(span),
        SvelteRenderLexicalVisibility::ScriptRootVisible
        | SvelteRenderLexicalVisibility::Blocked
        | SvelteRenderLexicalVisibility::Ambiguous => None,
    }
}

/// Parser-owned lexical admission for a direct static Svelte render callee.
///
/// `ScriptRootVisible` is the only state in which a script-level `$props`
/// binding may own navigation. A nearer local snippet wins; bindings introduced
/// by control/snippet scopes block the script root; malformed topology and
/// duplicate declarations never mint source authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteRenderLexicalVisibility {
    LocalSnippet(SourceSpan),
    ScriptRootVisible,
    Blocked,
    Ambiguous,
}

pub fn svelte_render_lexical_visibility_at(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<SvelteRenderLexicalVisibility> {
    let (render, callee) = svelte_static_render_callee_at(structure, offset)?;
    let inventory = structure.inventory();
    let callee_name = inventory.slice_span(callee).ok()?;
    let nodes = inventory.markup().nodes();

    let mut scope = render.parent;
    loop {
        let mut matching_declaration = None;
        for node in nodes.iter().filter(|node| node.parent == scope) {
            let MarkupNodeKind::SvelteControlBlock(block) = node.kind() else {
                continue;
            };
            let SvelteControlBlockHead::Snippet {
                authored_name,
                name_span,
                ..
            } = &block.head
            else {
                continue;
            };
            if inventory.slice(*authored_name).ok()? != callee_name {
                continue;
            }
            if matching_declaration.replace(*name_span).is_some() {
                return Some(SvelteRenderLexicalVisibility::Ambiguous);
            }
        }
        if let Some(declaration) = matching_declaration {
            return Some(SvelteRenderLexicalVisibility::LocalSnippet(declaration));
        }

        let Some(scope_id) = scope else {
            return Some(SvelteRenderLexicalVisibility::ScriptRootVisible);
        };
        let Some(scope_node) = nodes.iter().find(|node| node.id == scope_id) else {
            return Some(SvelteRenderLexicalVisibility::Blocked);
        };
        match scope_node.kind() {
            MarkupNodeKind::SvelteClause(clause) => {
                if matches!(
                    clause.head,
                    SvelteClauseHead::Then { binding: Some(_) }
                        | SvelteClauseHead::Catch { binding: Some(_) }
                ) {
                    return Some(SvelteRenderLexicalVisibility::Blocked);
                }
                let Some(controller_id) = scope_node.parent else {
                    return Some(SvelteRenderLexicalVisibility::Blocked);
                };
                let Some(controller) = nodes.iter().find(|node| node.id == controller_id) else {
                    return Some(SvelteRenderLexicalVisibility::Blocked);
                };
                if !matches!(controller.kind(), MarkupNodeKind::SvelteControlBlock(_)) {
                    return Some(SvelteRenderLexicalVisibility::Blocked);
                }
                scope = controller.parent;
            }
            MarkupNodeKind::SvelteControlBlock(block) => {
                let binding_barrier = match &block.head {
                    SvelteControlBlockHead::Each { item, index, .. } => {
                        item.is_some() || index.is_some()
                    }
                    SvelteControlBlockHead::Await { inline_branch, .. } => matches!(
                        inline_branch,
                        SvelteAwaitInlineBranch::Then {
                            binding: Some(_),
                            ..
                        } | SvelteAwaitInlineBranch::Catch {
                            binding: Some(_),
                            ..
                        }
                    ),
                    SvelteControlBlockHead::Snippet {
                        params_span: Some(params),
                        ..
                    } => params.start < params.end,
                    _ => false,
                };
                if binding_barrier {
                    return Some(SvelteRenderLexicalVisibility::Blocked);
                }
                scope = scope_node.parent;
            }
            _ => scope = scope_node.parent,
        }
    }
}

fn svelte_static_render_callee_at(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<(&MarkupSyntaxNode, SourceSpan)> {
    let source = structure.source().bytes().as_bytes();
    structure
        .inventory()
        .markup()
        .nodes()
        .iter()
        .find_map(|node| {
            let MarkupNodeKind::SvelteStandaloneTag(tag) = node.kind() else {
                return None;
            };
            if tag.family != SvelteStandaloneTagFamily::Render {
                return None;
            }
            let expression = tag.expression_span?;
            let bytes = source.get(expression.start as usize..expression.end as usize)?;
            let mut cursor = 0usize;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            let token_start = cursor;
            let first = *bytes.get(cursor)?;
            if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
                return None;
            }
            cursor += 1;
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$')
            {
                cursor += 1;
            }
            let token_end = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            let direct_call = bytes.get(cursor) == Some(&b'(')
                || bytes
                    .get(cursor..cursor.saturating_add(3))
                    .is_some_and(|suffix| suffix == b"?.(");
            if !direct_call {
                return None;
            }
            let span = SourceSpan::new(
                expression.source_space,
                expression.start + token_start as u32,
                expression.start + token_end as u32,
            );
            (offset >= span.start && offset < span.end).then_some((node, span))
        })
}

/// Cursor-region classification over the registered markup arena, for
/// analysis-absent fallback classification. Every region is a parser fact;
/// `None` means no arena node owns the offset (a parser-unowned gap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkupCursorRegion {
    /// Inside an element-like node's parser-identified opening span.
    OpeningTag {
        name: Option<String>,
        name_start: u32,
        name_end: u32,
    },
    /// Inside an interpolation node, past the opening delimiter.
    InterpolationExpression,
    /// Inside a comment interior.
    CommentInterior,
    /// Any other parser-owned position (text, closers, delimiters).
    Neutral,
}

/// Innermost arena node owning `offset`, mapped to a [`MarkupCursorRegion`].
///
/// Containment is end-exclusive except at the very end of the source, where an
/// UNTERMINATED span (one whose final byte is not `>`) still owns the typing
/// position — the live-edit case of an opening tag or comment growing at EOF.
pub fn markup_cursor_region(
    structure: &RegisteredFileStructure,
    offset: u32,
) -> Option<MarkupCursorRegion> {
    let inventory = structure.inventory();
    let source = inventory.source_spaces().first()?.bytes();
    let source_len = source.len() as u32;
    let contains = |start: u32, end: u32| {
        offset >= start
            && (offset < end
                || (offset == end
                    && end == source_len
                    && start < end
                    && source.as_bytes().get(end as usize - 1) != Some(&b'>')))
    };

    let mut best: Option<(u32, MarkupCursorRegion)> = None;
    let mut consider = |start: u32, end: u32, region: MarkupCursorRegion| {
        if contains(start, end) {
            let size = end - start;
            if best.as_ref().is_none_or(|(best_size, _)| size < *best_size) {
                best = Some((size, region));
            }
        }
    };

    for node in inventory.markup().nodes() {
        match node.kind() {
            MarkupNodeKind::Element(element) => {
                let full = element.full_span;
                let region = if contains(element.opening_span.start, element.opening_span.end) {
                    MarkupCursorRegion::OpeningTag {
                        name: inventory
                            .slice(element.authored_name)
                            .ok()
                            .map(str::to_string),
                        name_start: element.opening_name_span.start,
                        name_end: element.opening_name_span.end,
                    }
                } else {
                    MarkupCursorRegion::Neutral
                };
                consider(full.start, full.end, region);
            }
            MarkupNodeKind::Recovered {
                opening_span,
                opening_name_span,
                full_span,
                ..
            }
            | MarkupNodeKind::Unknown {
                opening_span,
                opening_name_span,
                full_span,
                ..
            } => {
                let in_opening =
                    opening_span.is_some_and(|opening| contains(opening.start, opening.end));
                let region = match (in_opening, opening_span, opening_name_span) {
                    (true, Some(opening), name_span) => MarkupCursorRegion::OpeningTag {
                        name: name_span.and_then(|span| {
                            source
                                .get(span.start as usize..span.end as usize)
                                .map(str::to_string)
                        }),
                        name_start: name_span.map_or(opening.start + 1, |span| span.start),
                        name_end: name_span.map_or(opening.start + 1, |span| span.end),
                    },
                    _ => MarkupCursorRegion::Neutral,
                };
                consider(full_span.start, full_span.end, region);
            }
            MarkupNodeKind::Comment {
                opening_span,
                full_span,
                ..
            } => {
                let region = if offset >= opening_span.end {
                    MarkupCursorRegion::CommentInterior
                } else {
                    MarkupCursorRegion::Neutral
                };
                consider(full_span.start, full_span.end, region);
            }
            MarkupNodeKind::Interpolation {
                opening_span,
                full_span,
                ..
            } => {
                let region = if offset >= opening_span.end {
                    MarkupCursorRegion::InterpolationExpression
                } else {
                    MarkupCursorRegion::Neutral
                };
                consider(full_span.start, full_span.end, region);
            }
            MarkupNodeKind::Text { content_span } => {
                consider(
                    content_span.start,
                    content_span.end,
                    MarkupCursorRegion::Neutral,
                );
            }
            MarkupNodeKind::SvelteControlBlock(block) => {
                consider(
                    block.full_span.start,
                    block.full_span.end,
                    MarkupCursorRegion::Neutral,
                );
            }
            MarkupNodeKind::SvelteClause(clause) => {
                consider(
                    clause.full_span.start,
                    clause.full_span.end,
                    MarkupCursorRegion::Neutral,
                );
            }
            MarkupNodeKind::SvelteStandaloneTag(tag) => {
                consider(
                    tag.full_span.start,
                    tag.full_span.end,
                    MarkupCursorRegion::Neutral,
                );
            }
        }
    }
    best.map(|(_, region)| region)
}

/// Parser-supplied lower bound for lexing a parser-UNOWNED trailing gap: the
/// closest parsed boundary at or before `offset` — arena node ends, section
/// opening/closing ends. A cursor inside a raw-text (script/style) section is
/// never a markup gap: the bound collapses to `offset` (empty window).
pub fn markup_gap_window_start(structure: &RegisteredFileStructure, offset: u32) -> u32 {
    let inventory = structure.inventory();
    let mut floor = 0u32;
    let mut raise = |end: u32| {
        if end <= offset && end > floor {
            floor = end;
        }
    };
    for node in inventory.markup().nodes() {
        let span = node.kind().full_span();
        raise(span.end);
    }
    for block in inventory.blocks() {
        if let CarrierBlock::Section { role, syntax, .. } = block {
            let raw_text = matches!(role, SectionRole::Script { .. } | SectionRole::Style { .. });
            if raw_text && offset >= syntax.full_span.start && offset < syntax.full_span.end {
                return offset;
            }
            raise(syntax.opening_span.end);
            raise(syntax.full_span.end);
        }
    }
    floor
}

#[derive(Debug, Clone)]
pub struct OpeningTagContext {
    pub tag_name: String,
    pub tag_name_start: u32,
    pub tag_name_end: u32,
    pub attrs: Vec<ParsedAttr>,
}

/// Return the already-parsed opening-tag facts. `source` remains in the
/// signature while feature APIs are cut over, but it is never inspected.
pub fn parse_opening_tag(_source: &str, block: &CarrierBlockView) -> OpeningTagContext {
    OpeningTagContext {
        tag_name: block.tag_name.clone(),
        tag_name_start: block.opening_name_start,
        tag_name_end: block.opening_name_end,
        attrs: block.attributes.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierCursorContext {
    BlockContent { block: CarrierBlockView },
    OpeningTag { block: CarrierBlockView },
    ClosingTag { block: CarrierBlockView },
    RootLevel,
}

pub fn classify_cursor(offset: u32, blocks: &[CarrierBlockView]) -> CarrierCursorContext {
    for block in blocks {
        if offset >= block.open_tag_start && offset < block.open_tag_end {
            return CarrierCursorContext::OpeningTag {
                block: block.clone(),
            };
        }
        if offset >= block.close_tag_start && offset < block.close_tag_end {
            return CarrierCursorContext::ClosingTag {
                block: block.clone(),
            };
        }
        let (content_start, content_end) = block.content_range();
        if offset >= content_start && offset < content_end {
            return CarrierCursorContext::BlockContent {
                block: block.clone(),
            };
        }
    }
    CarrierCursorContext::RootLevel
}

pub fn project_carrier_blocks(structure: &RegisteredFileStructure) -> Vec<CarrierBlockView> {
    let inventory = structure.inventory();
    let mut blocks = inventory
        .blocks()
        .iter()
        .filter_map(|block| match block {
            CarrierBlock::Section { id, syntax, .. } => {
                project_tagged(structure, structure.block_ref(*id)?, syntax)
            }
            CarrierBlock::MarkupRoot { id, node } => {
                let node = inventory.markup().nodes().get(node.get() as usize)?;
                let MarkupNodeKind::Element(element) = node.kind() else {
                    return None;
                };
                project_element(structure, structure.block_ref(*id)?, element)
            }
        })
        .collect::<Vec<_>>();

    // Error recovery may leave a template host without a closing/content span
    // while still publishing the following top-level section. Keep ownership
    // structural: the next inventory block (or EOF), never a delimiter search,
    // bounds the incomplete template region.
    let source = inventory.source_spaces().first().map(|space| space.bytes());
    let starts = blocks
        .iter()
        .map(|block| block.open_tag_start)
        .collect::<Vec<_>>();
    for (index, block) in blocks.iter_mut().enumerate() {
        let opening_is_self_closing = source.is_some_and(|source| {
            source
                .get(block.open_tag_start as usize..block.open_tag_end as usize)
                .is_some_and(|opening| opening.ends_with("/>"))
        });
        if block.tag_name == "template"
            && block.close_tag_start == block.open_tag_end
            && block.close_tag_end == block.open_tag_end
            && !opening_is_self_closing
        {
            let boundary = starts
                .get(index + 1)
                .copied()
                .or_else(|| source.map(|source| source.len() as u32))
                .unwrap_or(block.open_tag_end);
            block.close_tag_start = boundary;
            block.close_tag_end = boundary;
        }
    }
    blocks
}

pub fn project_carrier_blocks_for_document(
    document: &super::DocumentState,
) -> Vec<CarrierBlockView> {
    document
        .feature_snapshot
        .as_ref()
        .map(|snapshot| project_carrier_blocks(snapshot.structure()))
        .unwrap_or_default()
}

fn project_tagged(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    syntax: &TaggedSyntax,
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let tag_name = inventory.normalized_name(syntax.normalized_name).ok()?;
    project_common(
        structure,
        block_ref,
        tag_name,
        syntax.opening_span.start,
        syntax.opening_span.end,
        syntax.opening_name_span.start,
        syntax.opening_name_span.end,
        syntax.attribute_insertion_anchor.start,
        syntax.content_span.end,
        syntax.closing_span.map(|span| span.start),
        syntax.closing_span.map(|span| span.end),
        &syntax.attributes,
    )
}

fn project_element(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    syntax: &MarkupElementSyntax,
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let tag_name = inventory.normalized_name(syntax.normalized_name).ok()?;
    project_common(
        structure,
        block_ref,
        tag_name,
        syntax.opening_span.start,
        syntax.opening_span.end,
        syntax.opening_name_span.start,
        syntax.opening_name_span.end,
        syntax.attribute_insertion_anchor.start,
        syntax.content_span.end,
        syntax.closing_span.map(|span| span.start),
        syntax.closing_span.map(|span| span.end),
        &syntax.attributes,
    )
}

#[allow(clippy::too_many_arguments)]
fn project_common(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    tag_name: &str,
    open_tag_start: u32,
    open_tag_end: u32,
    opening_name_start: u32,
    opening_name_end: u32,
    attribute_insertion_anchor: u32,
    content_end: u32,
    close_start: Option<u32>,
    close_end: Option<u32>,
    attributes: &[CarrierAttribute],
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let attrs_raw = inventory
        .source_spaces()
        .first()?
        .bytes()
        .get(opening_name_end as usize..attribute_insertion_anchor as usize)?
        .to_string();
    let close_tag_start = close_start.unwrap_or(content_end);
    let close_tag_end = close_end.unwrap_or(content_end);
    let attributes = attributes
        .iter()
        .filter_map(|attribute| project_attribute(structure, attribute))
        .collect();
    Some(CarrierBlockView {
        block_ref,
        tag_name: tag_name.to_string(),
        open_tag_start,
        open_tag_end,
        close_tag_start,
        close_tag_end,
        opening_name_start,
        opening_name_end,
        attribute_insertion_anchor,
        attrs_raw,
        attributes,
    })
}

fn project_attribute(
    structure: &RegisteredFileStructure,
    attribute: &CarrierAttribute,
) -> Option<ParsedAttr> {
    let CarrierAttribute::Named {
        id, name, value, ..
    } = attribute
    else {
        return None;
    };
    let inventory = structure.inventory();
    let value_slice = match value {
        AttributeValue::Static { raw, .. } => Some(*raw),
        _ => None,
    };
    Some(ParsedAttr {
        attribute_ref: structure.attribute_ref(*id)?,
        name: inventory.slice(name.authored).ok()?.to_string(),
        value: value_slice
            .and_then(|slice| inventory.slice(slice).ok())
            .map(str::to_string),
        name_start: name.name_span.start,
        name_end: name.name_span.end,
        value_start: value_slice.map(|slice| slice.span.start),
        value_end: value_slice.map(|slice| slice.span.end),
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_structure(source: &str, svelte: bool) -> RegisteredFileStructure {
    use std::sync::Arc;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = verter_session::VerterHost::new(verter_session::HostConfig::default(), workspace);
    let registry = verter_session::LanguageRegistry::global();
    let extension = registry
        .carrier_extensions()
        .iter()
        .copied()
        .find(|extension| {
            registry
                .classify_static(&format!("fixture.{extension}"))
                .static_resolution()
                .is_svelte()
                == svelte
        })
        .expect("registered carrier extension");
    let canonical = format!("/fixture.{extension}");
    let file_language = registry.classify_static(&canonical).static_resolution();
    let _ = host
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(canonical.clone()),
            input_id: canonical.clone(),
            source: Arc::from(source),
            file_language,
            aliases: vec![],
        })
        .expect("registered test carrier");
    host.registered_file_structure_snapshot(&canonical)
        .expect("registered test structure")
        .0
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_carrier_blocks(source: &str) -> Vec<CarrierBlockView> {
    project_carrier_blocks(&test_structure(source, false))
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCarrierKind {
    RawText,
    Markup,
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_carrier_blocks_with(source: &str, kind: TestCarrierKind) -> Vec<CarrierBlockView> {
    match kind {
        TestCarrierKind::RawText => test_carrier_blocks(source),
        TestCarrierKind::Markup => test_svelte_blocks(source),
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn custom_block_content_kind(language_id: Option<&str>, canonical_id: &str) -> TestCarrierKind {
    if language_id == Some("svelte")
        || verter_session::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
            .is_svelte()
    {
        TestCarrierKind::Markup
    } else {
        TestCarrierKind::RawText
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_svelte_blocks(source: &str) -> Vec<CarrierBlockView> {
    project_carrier_blocks(&test_structure(source, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_projection_preserves_duplicate_attributes_and_sealed_refs() {
        let structure = test_structure(
            "<template><div/></template><script setup lang='ts' lang=js>ok</script>",
            false,
        );
        let blocks = project_carrier_blocks(&structure);
        let script = blocks
            .iter()
            .find(|block| block.tag_name == "script")
            .expect("script");
        assert!(script.is_setup());
        assert_eq!(script.lang(), Some("ts"));
        assert_eq!(script.attributes.len(), 3, "duplicates remain ordered");
        assert_eq!(script.block_ref.artifact_id(), structure.artifact_id());
        assert!(script
            .attributes
            .iter()
            .all(|attribute| attribute.attribute_ref.artifact_id() == structure.artifact_id()));
    }

    #[test]
    fn svelte_multi_root_projects_each_inventory_owner_once() {
        let structure = test_structure("<main/><aside/><script>let x = 1</script>", true);
        let blocks = project_carrier_blocks(&structure);
        assert_eq!(
            blocks
                .iter()
                .map(|block| block.tag_name.as_str())
                .collect::<Vec<_>>(),
            ["main", "aside", "script"]
        );
        let unique = blocks
            .iter()
            .map(|block| block.block_ref.block_id())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), blocks.len());
    }

    #[test]
    fn incomplete_template_is_bounded_by_the_next_inventory_block() {
        let source = "<template>\n  <div >\n</template>\n<script setup>\n</script>";
        let blocks = test_carrier_blocks(source);
        let template = blocks
            .iter()
            .find(|block| block.tag_name == "template")
            .expect("template");
        let boundary = blocks
            .iter()
            .find(|block| block.open_tag_start > template.open_tag_start)
            .map_or(source.len() as u32, |block| block.open_tag_start);
        assert_eq!(template.close_tag_start, boundary);
        assert!(source.find("<div >").unwrap() as u32 >= template.open_tag_end);
        assert!((source.find("<div >").unwrap() as u32) < template.close_tag_start);
    }

    #[test]
    fn authored_component_attribute_context_admits_only_heads_names_and_gaps() {
        let source = "<template><DirectComp first second /></template>";
        let structure = test_structure(source, false);
        for offset in [
            source.find("<DirectComp").unwrap() + "<DirectComp".len(),
            source.find("first").unwrap() + 2,
            source.find(" second").unwrap(),
            source.find("second").unwrap() + "second".len(),
        ] {
            assert_eq!(
                authored_component_attribute_name_context(&structure, offset as u32),
                Some(AuthoredComponentAttributeNameContext::ExactAttribute {
                    tag: "DirectComp".to_string(),
                }),
                "offset {offset} must retain the parser-owned component tag"
            );
        }

        let native = test_structure("<template><div class /></template>", false);
        let native_offset = "<template><div ".len() as u32;
        assert_eq!(
            authored_component_attribute_name_context(&native, native_offset),
            None,
            "native elements do not mint component-contract authority"
        );

        let incomplete = "<script setup>\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n<DirectComp un";
        let incomplete_structure = test_structure(incomplete, false);
        assert_eq!(
            authored_component_attribute_name_context(
                &incomplete_structure,
                incomplete.len() as u32,
            ),
            Some(
                AuthoredComponentAttributeNameContext::InexactUnclosedOpening {
                    tag: "DirectComp".to_string(),
                }
            ),
            "a parser-recovered opening keeps only its authored component identity"
        );

        let gap = "<script setup>\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n<DirectComp ";
        let gap_structure = test_structure(gap, false);
        assert_eq!(
            authored_component_attribute_name_context(&gap_structure, gap.len() as u32),
            Some(
                AuthoredComponentAttributeNameContext::InexactUnclosedOpening {
                    tag: "DirectComp".to_string(),
                }
            ),
            "an unclosed parser-owned EOF gap after the component name is inexact"
        );
        let bare = "<script setup>\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n<DirectComp";
        assert_eq!(
            authored_component_attribute_name_context(
                &test_structure(bare, false),
                bare.len() as u32,
            ),
            None,
            "the bare tag-name end is not an attribute context"
        );

        for post_attribute in [
            "<template><DirectComp foo=",
            "<template><DirectComp foo= ",
            "<template><DirectComp foo ",
        ] {
            let post_structure = test_structure(post_attribute, false);
            assert_eq!(
                authored_component_attribute_name_context(
                    &post_structure,
                    post_attribute.len() as u32,
                ),
                None,
                "an unclosed post-attribute gap cannot prove whether the cursor is still in a name: {post_attribute}"
            );
        }
    }

    #[test]
    fn authored_component_attribute_context_rejects_values_directives_and_spreads() {
        for (source, marker, cursor_delta, svelte) in [
            (
                "<template><DirectComp plain=\"value\" /></template>",
                "value\"",
                "value\"".len(),
                false,
            ),
            (
                "<template><DirectComp :bound=\"value\" /></template>",
                "bound",
                2,
                false,
            ),
            (
                "<template><DirectComp :bound=\"value\" /></template>",
                "value\"",
                "value\"".len(),
                false,
            ),
            (
                "<DirectComp mixed=\"left{value}right\" />",
                "value",
                2,
                true,
            ),
            ("<DirectComp onPick={on} />", "on}", 2, true),
            ("<DirectComp {...spread} />", "spread", 2, true),
            ("<DirectComp {@attach behavior} />", "behavior", 2, true),
            (
                "<template><DirectComp plain=\"unterminated",
                "unterminated",
                4,
                false,
            ),
            (
                "<template><DirectComp :bound=\"unterminated",
                "unterminated",
                4,
                false,
            ),
            ("<DirectComp {...spread", "spread", 3, true),
            ("<DirectComp {@attach behavior", "behavior", 3, true),
        ] {
            let structure = test_structure(source, svelte);
            let offset = source.find(marker).expect("marker") + cursor_delta;
            assert_eq!(
                authored_component_attribute_name_context(&structure, offset as u32),
                None,
                "value/directive/spread offset must fail closed: {source} @ {offset}"
            );
        }
    }

    #[test]
    fn nearest_component_ancestor_uses_only_parser_parent_topology() {
        let source = "<script>import Imported from './Imported.svelte';</script>\n<Imported><div>{#snippet header()}body{/snippet}</div></Imported>";
        let structure = test_structure(source, true);
        let cursor = source.find("body").expect("snippet body") as u32;
        assert_eq!(
            nearest_component_ancestor_tag(&structure, cursor),
            Some("Imported".to_string())
        );

        let native = "<main><div>{#snippet header()}body{/snippet}</div></main>";
        let native_structure = test_structure(native, true);
        let native_cursor = native.find("body").expect("native snippet body") as u32;
        assert_eq!(
            nearest_component_ancestor_tag(&native_structure, native_cursor),
            None,
            "native ancestors must not fabricate component identity"
        );
    }
}
