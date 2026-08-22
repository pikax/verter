// CSS language features for <style> blocks.
// Completions, hover, selector matching, and Vue-specific CSS intelligence.

pub(crate) mod global_classes;

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::{match_selector, MatchResult};
use verter_session::FileAnalysisSnapshot;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;

/// Provide CSS completions at a given position in a style block.
///
/// Offers:
/// - CSS custom property names (`--xxx`) from the same file's analysis
/// - CSS class names for reference
/// - Common CSS property names (top subset)
/// - `v-bind()` completions for reactive bindings
pub fn css_completions(
    position: &Position,
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Vec<CompletionItem>> {
    let offset = line_index.position_to_offset(position)? as usize;

    // Find which style block we're in
    let style_block = blocks.iter().find(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset <= ce as usize
        }
    })?;

    let mut items = Vec::new();

    // Offer custom properties and v-bind() reactive-binding completions from analysis — ONLY
    // for the CURRENT (live) style block. Sealed full-identity join: missing or foreign producer
    // identity (a stale analysis whose style entry no longer matches the block's live
    // `block_ref` — reparsed/re-identified since `analysis` was computed) fails closed to ZERO
    // completions here, the same join `is_declaration_value_position`/`selector_hover` perform —
    // never a leak of a stale style entry's custom properties or bindings onto the live block.
    //
    // This join also stands in for `analysis.template`'s own freshness gate below:
    // `FileAnalysisSnapshot.styles`/`.template` are always built together, from the same source
    // read, in the same `get_analysis` call (see `verter_session::host_manage::analysis_io`) —
    // there is no split-generation `FileAnalysisSnapshot` where `styles` is fresh and `template`
    // is stale, or vice versa. `ArtifactBlockRef` equality proves BOTH the block id AND the
    // artifact/parse-generation identity match (`artifact_identity` is content-addressed), so a
    // matched `live_style` here proves the WHOLE snapshot — `template` included — was produced
    // from the SAME parse generation as the live block; `template` has no per-block ref of its
    // own to join against (an SFC has exactly one `<template>`), so it borrows this result.
    let mut live_style_matches = false;
    if let Some(analysis) = analysis {
        let block_ref = style_block.block_ref.artifact_block_ref();
        let live_style = analysis
            .styles
            .iter()
            .find(|style| style.block_ref.as_ref() == Some(block_ref));
        live_style_matches = live_style.is_some();

        if let Some(style) = live_style {
            if let Some(css) = &style.css {
                for prop in &css.custom_properties {
                    items.push(CompletionItem {
                        label: format!("var({})", prop.name),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some("CSS custom property".into()),
                        insert_text: Some(format!("var({})", prop.name)),
                        ..Default::default()
                    });
                }
            }

            // v-bind() completions for reactive bindings
            for binding in &analysis.bindings {
                if binding.is_reactive {
                    items.push(CompletionItem {
                        label: format!("v-bind({})", binding.name),
                        kind: Some(CompletionItemKind::SNIPPET),
                        detail: Some(format!(
                            "Bind to {} ({:?})",
                            binding.name, binding.reactivity_kind
                        )),
                        insert_text: Some(format!("v-bind({})", binding.name)),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Check if we're in a property value context (after ':')
    let in_value = is_declaration_value_position(offset, style_block, analysis);

    if !in_value {
        // Offer common CSS property names
        for prop in COMMON_CSS_PROPERTIES {
            items.push(CompletionItem {
                label: prop.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some("CSS property".into()),
                insert_text: Some(format!("{prop}: ")),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            });
        }

        // Offer template class/ID completions when typing . or # in selector context — gated on
        // `live_style_matches` (see the join comment above): a stale `analysis` must not leak
        // template class/ID names computed against an earlier version of the file.
        if live_style_matches {
            if let Some(template) = analysis.and_then(|analysis| analysis.template.as_deref()) {
                let byte_before = if offset > 0 {
                    source.as_bytes().get(offset - 1).copied()
                } else {
                    None
                };

                if byte_before == Some(b'.') {
                    // After '.' — offer template class names
                    let mut seen = std::collections::HashSet::new();
                    for el in &template.elements {
                        for cls in el.static_classes() {
                            if seen.insert(cls.to_string()) {
                                items.push(CompletionItem {
                                    label: cls.to_string(),
                                    kind: Some(CompletionItemKind::CLASS),
                                    detail: Some("template class".into()),
                                    ..Default::default()
                                });
                            }
                        }
                        for dcn in &el.dynamic_classes {
                            if seen.insert(dcn.clone()) {
                                items.push(CompletionItem {
                                    label: dcn.clone(),
                                    kind: Some(CompletionItemKind::CLASS),
                                    detail: Some("template class (dynamic)".into()),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                } else if byte_before == Some(b'#') {
                    // After '#' — offer template IDs
                    let mut seen = std::collections::HashSet::new();
                    for el in &template.elements {
                        if let Some(id) = el.static_id() {
                            if seen.insert(id.to_string()) {
                                items.push(CompletionItem {
                                    label: id.to_string(),
                                    kind: Some(CompletionItemKind::VALUE),
                                    detail: Some("template ID".into()),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// Whether `offset` sits inside a rule's declaration VALUE, read from the
/// shared style-syntax parse — never a raw brace/colon/semicolon backward
/// scan (that misclassifies e.g. a semicolon-shaped byte inside a quoted
/// string value as a statement boundary). Structural signal: `offset` falls
/// inside some selector's `rule_body_span` (is this a rule's declaration
/// block at all) AND inside a `Complete` declaration's
/// `name_span.end..=value_span.end` range (past the property name, at or
/// before the end of its — possibly empty, still-being-typed — value).
///
/// **Fail-closed**, same join `selector_hover`/`document_colors` perform:
/// the sealed `StyleBlockAnalysis.block_ref`, joined against the block's
/// live `CarrierBlockView.block_ref`. `analysis: None`, OR no
/// `analysis.styles` entry joins to the block's live `block_ref` (stale),
/// OR the offset falls inside a declaration absent from `declarations`
/// (incomplete/unparsed) all classify as "not a value position" — the safe
/// default that offers property-name completions, never a value
/// classification the parse cannot structurally confirm.
fn is_declaration_value_position(
    offset: usize,
    style_block: &CarrierBlockView,
    analysis: Option<&FileAnalysisSnapshot>,
) -> bool {
    let Some(analysis) = analysis else {
        return false;
    };
    let block_ref = style_block.block_ref.artifact_block_ref();
    let Some(style) = analysis
        .styles
        .iter()
        .find(|style| style.block_ref.as_ref() == Some(block_ref))
    else {
        return false;
    };
    let Some(css) = style.css.as_ref() else {
        return false;
    };

    let Ok(offset) = u32::try_from(offset) else {
        return false;
    };

    let in_rule_body = css.selectors.iter().any(|sel| {
        sel.rule_body_span
            .is_some_and(|body| offset >= body.start && offset <= body.end)
    });
    if !in_rule_body {
        return false;
    }

    css.declarations
        .iter()
        .any(|decl| offset >= decl.name_span.end && offset <= decl.value_span.end)
}

/// Completions INSIDE `v-bind(|)`: the setup-scope bindings by bare name.
/// Reactive bindings list first; every item carries the binding kind as its
/// native detail (upgraded to the provider-typed detail by the handler when
/// the declaration position maps). Never a word-fallback item.
pub fn v_bind_scope_completions(analysis: &FileAnalysisSnapshot) -> Option<Vec<CompletionItem>> {
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut reactive: Vec<&verter_semantic::analysis::AnalyzedBinding> = Vec::new();
    let mut plain: Vec<&verter_semantic::analysis::AnalyzedBinding> = Vec::new();
    for binding in &analysis.bindings {
        if binding.span.start == 0 && binding.span.end == 0 {
            continue;
        }
        if binding.is_reactive {
            reactive.push(binding);
        } else {
            plain.push(binding);
        }
    }
    for (order, binding) in reactive.iter().chain(plain.iter()).enumerate() {
        items.push(CompletionItem {
            label: binding.name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(if binding.is_reactive {
                format!("setup binding ({:?})", binding.reactivity_kind)
            } else {
                "setup binding".to_string()
            }),
            sort_text: Some(format!("{order:03}")),
            insert_text: Some(binding.name.clone()),
            ..Default::default()
        });
    }
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// The style `v-bind()` whose expression span contains `offset`, with its
/// root binding's DECLARATION span. The v-bind token has no TSX projection
/// (style blocks are removed from the generated surface), so provider-typed
/// display anchors on the declaration instead. `None` when the offset is not
/// on a v-bind expression or no matching script binding exists.
pub(crate) fn v_bind_decl_target_at(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<(String, verter_span::Span)> {
    for style in analysis.styles.iter() {
        for vb in &style.v_binds {
            if vb.start < vb.end && offset >= vb.start && offset <= vb.end {
                // NOTE: `expr_roots` is a sorted set, so a multi-root
                // expression (`v-bind(width ?? fallback)`) anchors on the
                // alphabetically-first root's declaration, not the first in
                // source order — a display-anchor choice only.
                let root = vb.expr_roots.first()?;
                let binding = analysis.bindings.iter().find(|b| b.name == *root)?;
                if binding.span.start == 0 && binding.span.end == 0 {
                    return None;
                }
                return Some((vb.expression.clone(), binding.span));
            }
        }
    }
    None
}

/// Provide hover information for CSS at the given position.
///
/// Shows:
/// - v-bind() binding type info
/// - :deep(), :global(), :slotted() documentation
/// - Matched template elements when hovering on a CSS selector
pub fn css_hover(
    position: &Position,
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Hover> {
    let offset = line_index.position_to_offset(position)? as usize;

    // Find which style block we're in
    let style_block = blocks.iter().find(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset <= ce as usize
        }
    })?;

    let analysis = analysis?;

    // Check v-bind() expressions and special pseudos
    for style in analysis.styles.iter() {
        for vb in &style.v_binds {
            if offset >= vb.start as usize && offset <= vb.end as usize {
                let binding = analysis.bindings.iter().find(|b| b.name == vb.expression);
                let info = if let Some(binding) = binding {
                    format!(
                        "**v-bind({})** — {:?} ({:?})\n\nBinds the CSS value to the reactive `{}` binding.",
                        vb.expression, binding.kind, binding.reactivity_kind, vb.expression,
                    )
                } else {
                    format!("**v-bind({})** — CSS reactive binding", vb.expression)
                };

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: info,
                    }),
                    range: None,
                });
            }
        }

        for pseudo in &style.special_pseudos {
            if offset >= pseudo.start as usize && offset <= pseudo.end as usize {
                let desc = match pseudo.kind {
                    verter_semantic::analysis::style::SpecialPseudoKind::Deep => {
                        "**:deep()** — Targets child component elements, bypassing scoped CSS encapsulation."
                    }
                    verter_semantic::analysis::style::SpecialPseudoKind::Global => {
                        "**:global()** — Makes this selector apply globally, ignoring scoped CSS."
                    }
                    verter_semantic::analysis::style::SpecialPseudoKind::Slotted => {
                        "**:slotted()** — Targets slotted content from the parent component."
                    }
                };

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: desc.to_string(),
                    }),
                    range: None,
                });
            }
        }
    }

    // Check if cursor is on a CSS selector — show matched template elements
    if let Some(hover) = selector_hover(offset, style_block, analysis, line_index) {
        return Some(hover);
    }

    let _ = source;

    None
}

/// Check if the cursor is on a CSS selector and show matching template elements.
fn selector_hover(
    offset: usize,
    style_block: &CarrierBlockView,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Hover> {
    // The sealed block ref is the association authority: a full-identity
    // join (artifact identity + block id). Missing or foreign producer
    // identity fails closed; offsets remain span metadata only.
    let block_ref = style_block.block_ref.artifact_block_ref();
    let style = analysis
        .styles
        .iter()
        .find(|style| style.block_ref.as_ref() == Some(block_ref))?;
    let css = style.css.as_ref()?;
    let template = analysis.template.as_deref()?;

    // Find selector at cursor position
    let selector = css
        .selectors
        .iter()
        .find(|sel| (offset as u32) >= sel.span.start && (offset as u32) <= sel.span.end)?;

    let structure = selector.structure.as_ref()?;

    // Match against all template elements
    let mut matched = Vec::new();
    let mut maybe_matched = Vec::new();

    for (idx, el) in template.elements.iter().enumerate() {
        match match_selector(structure, idx, &template.elements) {
            MatchResult::Matches => matched.push(el),
            MatchResult::MaybeMatches => maybe_matched.push(el),
            MatchResult::NoMatch => {}
        }
    }

    if matched.is_empty() && maybe_matched.is_empty() {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "**`{}`**\n\nSpecificity: `({}, {}, {})`\n\nNo matching template elements.",
                    selector.text,
                    selector.specificity.0,
                    selector.specificity.1,
                    selector.specificity.2,
                ),
            }),
            range: selector_range(selector, line_index),
        });
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "**`{}`**\n\nSpecificity: `({}, {}, {})`",
        selector.text, selector.specificity.0, selector.specificity.1, selector.specificity.2,
    ));

    if !matched.is_empty() {
        lines.push(format!("**Matches {} element(s):**", matched.len()));
        for el in &matched {
            lines.push(format_element_match(el, line_index));
        }
    }

    if !maybe_matched.is_empty() {
        lines.push(format!(
            "**May match {} element(s)** (dynamic classes):",
            maybe_matched.len()
        ));
        for el in &maybe_matched {
            lines.push(format_element_match(el, line_index));
        }
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: selector_range(selector, line_index),
    })
}

/// Format a single element match for hover display.
fn format_element_match(
    el: &verter_semantic::analysis::TemplateElement,
    line_index: &LineIndex,
) -> String {
    let line = line_index
        .offset_to_position(el.span.start)
        .map(|p| p.line + 1)
        .unwrap_or(0);
    let classes: Vec<&str> = el.static_classes().collect();
    let class_info = if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", classes.join(" "))
    };
    let id_info = el
        .static_id()
        .map(|id| format!(" id=\"{id}\""))
        .unwrap_or_default();
    format!("- `<{}{id_info}{class_info}>` (line {line})", el.tag)
}

/// Compute the LSP range for a selector's span.
fn selector_range(
    selector: &verter_semantic::analysis::style::AnalyzedSelector,
    line_index: &LineIndex,
) -> Option<Range> {
    let start = line_index.offset_to_position(selector.span.start)?;
    let end = line_index.offset_to_position(selector.span.end)?;
    Some(Range { start, end })
}

/// Common CSS property names for basic completion.
static COMMON_CSS_PROPERTIES: &[&str] = &[
    "display",
    "position",
    "width",
    "height",
    "margin",
    "padding",
    "border",
    "background",
    "color",
    "font-size",
    "font-weight",
    "font-family",
    "line-height",
    "text-align",
    "text-decoration",
    "flex",
    "flex-direction",
    "justify-content",
    "align-items",
    "gap",
    "grid-template-columns",
    "grid-template-rows",
    "overflow",
    "opacity",
    "z-index",
    "cursor",
    "transition",
    "transform",
    "box-shadow",
    "border-radius",
    "max-width",
    "min-height",
    "top",
    "right",
    "bottom",
    "left",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::carrier_structure::test_carrier_blocks;
    use verter_semantic::analysis::*;

    #[test]
    fn test_css_completions_in_style() {
        let source = "<style>\n.foo { \n}\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // Position inside the style block, on the empty property line
        let pos = line_index.offset_to_position(15).unwrap();
        let items = css_completions(&pos, source, &blocks, None, &line_index);
        assert!(items.is_some(), "should offer CSS property completions");
        let items = items.unwrap();
        assert!(items.iter().any(|i| i.label == "display"));
    }

    #[test]
    fn test_no_completions_outside_style() {
        let source = "<template>\n<div />\n</template>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let pos = line_index.offset_to_position(15).unwrap();
        let items = css_completions(&pos, source, &blocks, None, &line_index);
        assert!(items.is_none());
    }

    fn make_element(tag: &str, classes: &[&str]) -> TemplateElement {
        let mut attrs = Vec::new();
        if !classes.is_empty() {
            attrs.push(TemplateAttribute {
                name: "class".into(),
                value: Some(classes.join(" ")),
                is_dynamic: false,
                span: verter_span::Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: tag.into(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: verter_span::Span::new(0, 0),
            tag_span_end: 0,
            content_end: 0,
            ..Default::default()
        }
    }

    fn build_style(
        source: &str,
        blocks: &[CarrierBlockView],
    ) -> verter_semantic::analysis::StyleBlockAnalysis {
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        build_style_for_block(source, style_block)
    }

    fn build_style_for_block(
        source: &str,
        style_block: &CarrierBlockView,
    ) -> verter_semantic::analysis::StyleBlockAnalysis {
        let (content_start, content_end) = style_block.content_range();
        let css_content = &source[content_start as usize..content_end as usize];
        let scoped = style_block.is_scoped();

        let mut analysis = verter_semantic::analysis::style::build_css_style_analysis(
            css_content,
            verter_semantic::analysis::style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_start,
        );
        analysis.block_ref = Some(style_block.block_ref.artifact_block_ref().clone());
        analysis
    }

    /// @ai-generated - Hover on CSS selector shows matched template elements
    #[test]
    fn test_hover_on_selector_shows_matches() {
        let source = "<template><div class=\"foo\"></div></template>\n<style scoped>\n.foo { color: red; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["foo"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        // Position on ".foo" in the style block
        let selector_offset = source.find(".foo {").unwrap();
        let pos = line_index
            .offset_to_position(selector_offset as u32)
            .unwrap();
        let hover = css_hover(&pos, source, &blocks, Some(&analysis), &line_index);

        assert!(hover.is_some(), "should provide hover on selector");
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("`.foo`"), "should show selector text");
        assert!(contents.contains("Matches"), "should show match info");
        assert!(contents.contains("<div"), "should reference the element");
    }

    /// @ai-generated - Sealed block identity wins when offset metadata points
    /// at the wrong analysis in a same-scoped multi-style file.
    #[test]
    fn selector_hover_joins_multi_style_analysis_by_sealed_block_identity() {
        let source = "<template><div class=\"target\"></div></template>\n<style>.wrong {}</style>\n<style>.target {}</style>";
        let blocks = test_carrier_blocks(source);
        let style_blocks = blocks
            .iter()
            .filter(|block| block.tag_name == "style")
            .collect::<Vec<_>>();
        assert_eq!(style_blocks.len(), 2);

        let mut wrong = build_style_for_block(source, style_blocks[0]);
        let mut target = build_style_for_block(source, style_blocks[1]);
        wrong.content_offset = style_blocks[1].content_range().0;
        target.content_offset = u32::MAX;

        let analysis = FileAnalysisSnapshot {
            styles: (vec![wrong, target]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["target"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };
        let line_index = LineIndex::new_utf16(source);
        let selector_offset = source.rfind(".target").unwrap() as u32;
        let position = line_index.offset_to_position(selector_offset).unwrap();

        let hover = css_hover(&position, source, &blocks, Some(&analysis), &line_index)
            .expect("the target block's analysis must be selected by sealed identity");
        let HoverContents::Markup(contents) = hover.contents else {
            panic!("expected markup hover")
        };
        assert!(contents.value.contains("`.target`"));
    }

    /// RED fixture where ordinal/naked-local-id and sealed identity DIVERGE:
    /// a STALE analysis snapshot — built from a superseded revision whose
    /// style block carries the SAME artifact-local block id at the SAME byte
    /// offsets — must never join the current structure block. A naked-u32
    /// join mis-binds it (and hovers the superseded `.aaaaaa` selector); the
    /// sealed full-identity join fails closed.
    #[test]
    fn selector_hover_refuses_stale_artifact_analysis_with_matching_local_id() {
        let current = "<template><div class=\"target\"/></template><style>.target{}</style>";
        let stale = "<template><div class=\"target\"/></template><style>.aaaaaa{}</style>";
        let blocks = test_carrier_blocks(current);
        let stale_blocks = test_carrier_blocks(stale);
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let stale_style_block = stale_blocks.iter().find(|b| b.tag_name == "style").unwrap();
        assert_eq!(
            style_block.block_ref.block_id(),
            stale_style_block.block_ref.block_id(),
            "fixture premise: identical artifact-local block id"
        );
        assert_ne!(
            style_block.block_ref.artifact_block_ref(),
            stale_style_block.block_ref.artifact_block_ref(),
            "fixture premise: distinct sealed artifact identities"
        );

        // The stale snapshot: the superseded content's analysis, sealed to
        // the superseded artifact, still claiming the same naked local id.
        let stale_entry = build_style_for_block(stale, stale_style_block);
        let analysis = FileAnalysisSnapshot {
            styles: (vec![stale_entry]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["target"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };
        let line_index = LineIndex::new_utf16(current);
        let selector_offset = current.rfind(".target").unwrap() as u32;
        let position = line_index.offset_to_position(selector_offset).unwrap();

        let hover = css_hover(&position, current, &blocks, Some(&analysis), &line_index);
        assert!(
            hover.is_none(),
            "a stale artifact's analysis must fail closed, never mis-bind \
             through the naked local id: {hover:?}"
        );
    }

    /// @ai-generated - After '.' in style offers template class names
    #[test]
    fn test_selector_completion_class() {
        let source =
            "<template><div class=\"foo bar\"></div></template>\n<style scoped>\n.\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["foo", "bar"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        // Position right after '.'
        let dot_offset = source.rfind('.').unwrap() + 1;
        let pos = line_index.offset_to_position(dot_offset as u32).unwrap();
        let items = css_completions(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        let items = items.unwrap();
        assert!(items.iter().any(|i| i.label == "foo"), "should offer foo");
        assert!(items.iter().any(|i| i.label == "bar"), "should offer bar");
    }

    /// @ai-generated - After '#' in style offers template IDs
    #[test]
    fn test_selector_completion_id() {
        let source = "<template><div id=\"app\"></div></template>\n<style scoped>\n#\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let mut el = make_element("div", &[]);
        el.attributes.push(TemplateAttribute {
            name: "id".into(),
            value: Some("app".into()),
            is_dynamic: false,
            span: verter_span::Span::new(0, 0),
            name_end: 0,
            value_span: None,
        });

        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![el],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        let hash_offset = source.rfind('#').unwrap() + 1;
        let pos = line_index.offset_to_position(hash_offset as u32).unwrap();
        let items = css_completions(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        let items = items.unwrap();
        assert!(
            items.iter().any(|i| i.label == "app"),
            "should offer app ID"
        );
    }

    /// A23 round 3: a STALE `analysis` (same artifact-local style block id, distinct sealed
    /// artifact identity — the same fixture shape as
    /// `selector_hover_refuses_stale_artifact_analysis_with_matching_local_id`) must not leak
    /// its `template` class name into completions at a selector position after `.`. Prior to the
    /// fix, `css_completions` read `analysis.template` directly with no join against the live
    /// style block at all.
    #[test]
    fn css_completions_stale_analysis_template_class_never_leaks_into_live_completions() {
        let current =
            "<template><div class=\"live\"></div></template>\n<style scoped>\n.\n</style>";
        let stale =
            "<template><div class=\"live\"></div></template>\n<style scoped>\n.aaaaaa{}\n</style>";
        let blocks = test_carrier_blocks(current);
        let stale_blocks = test_carrier_blocks(stale);
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let stale_style_block = stale_blocks.iter().find(|b| b.tag_name == "style").unwrap();
        assert_eq!(
            style_block.block_ref.block_id(),
            stale_style_block.block_ref.block_id(),
            "fixture premise: identical artifact-local block id"
        );
        assert_ne!(
            style_block.block_ref.artifact_block_ref(),
            stale_style_block.block_ref.artifact_block_ref(),
            "fixture premise: distinct sealed artifact identities"
        );

        // The stale snapshot: a superseded style entry (sealed to the superseded artifact) paired
        // with a `template` carrying a class name ("stale-leak") that must never surface.
        let stale_entry = build_style_for_block(stale, stale_style_block);
        let analysis = FileAnalysisSnapshot {
            styles: (vec![stale_entry]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["stale-leak"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };
        let line_index = LineIndex::new_utf16(current);
        let dot_offset = current.rfind('.').unwrap() + 1;
        let pos = line_index.offset_to_position(dot_offset as u32).unwrap();

        let items = css_completions(&pos, current, &blocks, Some(&analysis), &line_index);
        let labels: Vec<String> = items
            .as_ref()
            .map(|v| v.iter().map(|i| i.label.clone()).collect())
            .unwrap_or_default();
        assert!(
            !labels.contains(&"stale-leak".to_string()),
            "a stale analysis.template must not leak its class name into live completions: {labels:?}"
        );
    }

    /// @ai-generated - After '.' in value context (e.g., border: .5px) no class completions
    #[test]
    fn test_no_selector_completion_in_value() {
        let source = "<template><div class=\"foo\"></div></template>\n<style scoped>\n.foo { border: .5px; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &["foo"])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        // Position after the '.' in ".5px" (value context)
        let dot_offset = source.find(".5px").unwrap() + 1;
        let pos = line_index.offset_to_position(dot_offset as u32).unwrap();
        let items = css_completions(&pos, source, &blocks, Some(&analysis), &line_index);
        // In value context, should not offer template class names
        if let Some(items) = items {
            assert!(
                !items
                    .iter()
                    .any(|i| i.detail.as_deref() == Some("template class")),
                "should not offer template classes in value context"
            );
        }
    }

    /// Discriminating positive (A23): a PRIOR declaration's value contains a
    /// semicolon-shaped byte inside a quoted string on the same line
    /// (`content: "a;b";`). The old backward-scan took the text after the
    /// last `{` and checked `contains(':') && !contains(';')`; the in-string
    /// `;` makes `contains(';')` true even though no real declaration
    /// boundary was crossed, so it wrongly concluded "not a value position"
    /// and would have offered property-name completions right after
    /// `color:`. Reading `rule_body_span`/`declarations[i]` structurally
    /// (never scanning the raw text for `;`) classifies this correctly as a
    /// value position — no property completions offered.
    #[test]
    fn css_completions_in_string_semicolon_before_cursor_still_classifies_value_position() {
        let source = "<style>\n.foo { content: \"a;b\"; color:  }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);
        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            ..Default::default()
        };

        // Cursor right after "color: " (between the two spaces before '}') —
        // past the property name and colon, genuinely a value position even
        // though nothing has been typed as the value yet.
        let colon_idx = source.find("color:").unwrap();
        let offset = colon_idx + "color: ".len();
        let pos = line_index.offset_to_position(offset as u32).unwrap();

        let items = css_completions(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(
            items.is_none(),
            "a genuine value position (past 'color:') must not offer property \
             completions, even though an in-string ';' precedes the cursor: {items:?}"
        );
    }

    /// Fail-closed (A23): `analysis: None` at a value-position offset must
    /// still offer property-name completions — never an empty/guessed set.
    #[test]
    fn css_completions_none_analysis_fails_closed_to_property_completions() {
        let source = "<style>\n.foo { color: red; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // A position that IS a genuine value position when analysis joins —
        // proving the fail-closed path, not an offset that merely isn't a
        // value position regardless.
        let value_offset = source.find("red").unwrap();
        let pos = line_index.offset_to_position(value_offset as u32).unwrap();

        let items = css_completions(&pos, source, &blocks, None, &line_index);
        assert!(
            items.is_some(),
            "analysis: None must fail closed to property completions"
        );
        let items = items.unwrap();
        assert!(
            items.iter().any(|i| i.label == "display"),
            "expected property completions, got {items:?}"
        );
    }

    /// Fail-closed (A23): a STALE `analysis` whose `styles[].block_ref` does
    /// not match the live block's `block_ref` must offer property-name
    /// completions, never the stale analysis's own value classification.
    /// Mirrors `selector_hover_refuses_stale_artifact_analysis_with_matching_local_id`
    /// / `document_colors_stale_analysis_block_ref_mismatch_fails_closed`.
    #[test]
    fn css_completions_stale_analysis_block_ref_mismatch_fails_closed_to_property_completions() {
        let current = "<style>.foo{color:red}</style>";
        let stale = "<style>.foo{color:blue}</style>";
        let blocks = test_carrier_blocks(current);
        let stale_blocks = test_carrier_blocks(stale);
        let stale_style_block = stale_blocks.iter().find(|b| b.tag_name == "style").unwrap();

        // The stale snapshot: the superseded content's analysis, sealed to
        // the superseded artifact's own block_ref.
        let stale_css = build_style_for_block(stale, stale_style_block);
        let analysis = FileAnalysisSnapshot {
            styles: (vec![stale_css]).into(),
            ..Default::default()
        };
        let line_index = LineIndex::new_utf16(current);

        // A genuine value position in the LIVE doc, which the stale join
        // must never confirm.
        let value_offset = current.find("red").unwrap();
        let pos = line_index.offset_to_position(value_offset as u32).unwrap();

        let items = css_completions(&pos, current, &blocks, Some(&analysis), &line_index);
        assert!(
            items.is_some(),
            "a stale analysis must fail closed to property completions, never \
             mis-bind through a naked ordinal"
        );
        let items = items.unwrap();
        assert!(
            items.iter().any(|i| i.label == "display"),
            "expected property completions, got {items:?}"
        );
    }

    /// Fail-closed (A23): a STALE `analysis` whose (non-joining) style entry has its OWN custom
    /// property must not leak `var(--stale)` into the live block's completions. The prior fix
    /// round joined `is_declaration_value_position`'s classification correctly but left the
    /// custom-property/v-bind loop unconditional over EVERY `analysis.styles` entry — this is the
    /// discriminator the pre-existing stale-fixture test above could not catch (its stale fixture
    /// has no custom properties or bindings at all).
    #[test]
    fn css_completions_stale_analysis_custom_property_never_leaks_into_live_completions() {
        let current = "<style>.foo{color:red}</style>";
        let stale = "<style>.foo{--stale:red;color:blue}</style>";
        let blocks = test_carrier_blocks(current);
        let stale_blocks = test_carrier_blocks(stale);
        let stale_style_block = stale_blocks.iter().find(|b| b.tag_name == "style").unwrap();

        // The stale snapshot: the superseded content's analysis (with its own `--stale` custom
        // property), sealed to the superseded artifact's own block_ref — never the live block's.
        let stale_css = build_style_for_block(stale, stale_style_block);
        assert!(
            stale_css
                .css
                .as_ref()
                .is_some_and(|css| css.custom_properties.iter().any(|p| p.name == "--stale")),
            "fixture sanity: the stale style entry must actually carry --stale"
        );
        let analysis = FileAnalysisSnapshot {
            styles: (vec![stale_css]).into(),
            ..Default::default()
        };
        let line_index = LineIndex::new_utf16(current);

        // A declaration VALUE position in the LIVE doc — exactly where a `var(--stale)`
        // completion would be offered if the stale entry's custom properties leaked through.
        let value_offset = current.find("red").unwrap();
        let pos = line_index.offset_to_position(value_offset as u32).unwrap();

        let items = css_completions(&pos, current, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some(), "expected property completions, got None");
        let items = items.unwrap();
        assert!(
            !items.iter().any(|i| i.label == "var(--stale)"),
            "a stale style entry's custom property must never leak into the live block's \
             completions: {items:?}"
        );
    }

    /// Fail-closed (A23): an incomplete/unparsed declaration at the offset
    /// (an unterminated function inside the value — `rgb(` never closed
    /// before the rule's `}` — marks the `Declaration` node itself
    /// `StyleCompleteness::Recovered`, absent from `declarations`) must
    /// offer property-name completions. Same discriminator fixture as
    /// `document_colors_incomplete_declaration_fails_closed`.
    #[test]
    fn css_completions_incomplete_declaration_fails_closed_to_property_completions() {
        let source = "<style>\n.foo { color: rgb( }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);
        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            ..Default::default()
        };

        let inside_offset = source.find("rgb(").unwrap() + "rgb(".len();
        let pos = line_index.offset_to_position(inside_offset as u32).unwrap();

        let items = css_completions(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(
            items.is_some(),
            "an incomplete/unparsed declaration must fail closed to property completions"
        );
        let items = items.unwrap();
        assert!(
            items.iter().any(|i| i.label == "display"),
            "expected property completions, got {items:?}"
        );
    }

    /// @ai-generated - Hover on CSS selector with no matches shows "no matching"
    #[test]
    fn test_hover_on_selector_no_matches() {
        let source = "<template><div></div></template>\n<style scoped>\n.nonexistent { color: red; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let analysis = FileAnalysisSnapshot {
            styles: (vec![css]).into(),
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![make_element("div", &[])],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        let selector_offset = source.find(".nonexistent").unwrap();
        let pos = line_index
            .offset_to_position(selector_offset as u32)
            .unwrap();
        let hover = css_hover(&pos, source, &blocks, Some(&analysis), &line_index);

        assert!(hover.is_some(), "should provide hover even with no matches");
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("No matching template elements"));
    }
}
