// CSS language features for <style> blocks.
// Completions, hover, selector matching, and Vue-specific CSS intelligence.

pub(crate) mod global_classes;

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::{match_selector, MatchResult};
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

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
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Vec<CompletionItem>> {
    let offset = line_index.position_to_offset(position)? as usize;

    // Find which style block we're in
    let _style_block = blocks.iter().find(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset <= ce as usize
        }
    })?;

    let mut items = Vec::new();

    // Offer custom properties and classes from analysis
    if let Some(analysis) = analysis {
        for style in analysis.styles.iter() {
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

    // Check if we're in a property value context (after ':')
    let content_before = &source[..offset];
    let last_line = content_before.lines().last().unwrap_or("");
    // Use text after last '{' to handle single-line rules like `.foo { border: .5px; }`
    let context = if let Some(brace_pos) = last_line.rfind('{') {
        &last_line[brace_pos + 1..]
    } else {
        last_line
    };
    let in_value = context.contains(':') && !context.contains(';');

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

        // Offer template class/ID completions when typing . or # in selector context
        if let Some(analysis) = analysis {
            if let Some(template) = analysis.template.as_deref() {
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
    blocks: &[SfcBlock],
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
    style_block: &SfcBlock,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Hover> {
    let (content_start, _) = style_block.content_range();

    // Find the matching style analysis for this block
    let style = analysis
        .styles
        .iter()
        .find(|s| s.content_offset == content_start)?;
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
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_semantic::analysis::*;

    #[test]
    fn test_css_completions_in_style() {
        let source = "<style>\n.foo { \n}\n</style>";
        let blocks = scan_sfc_blocks(source);
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
        let blocks = scan_sfc_blocks(source);
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
        blocks: &[SfcBlock],
    ) -> verter_semantic::analysis::StyleBlockAnalysis {
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (content_start, content_end) = style_block.content_range();
        let css_content = &source[content_start as usize..content_end as usize];
        let scoped = style_block.attrs_raw.contains("scoped");

        verter_semantic::analysis::style::build_css_style_analysis(
            css_content,
            verter_semantic::analysis::style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_start,
        )
    }

    /// @ai-generated - Hover on CSS selector shows matched template elements
    #[test]
    fn test_hover_on_selector_shows_matches() {
        let source = "<template><div class=\"foo\"></div></template>\n<style scoped>\n.foo { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
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

    /// @ai-generated - After '.' in style offers template class names
    #[test]
    fn test_selector_completion_class() {
        let source =
            "<template><div class=\"foo bar\"></div></template>\n<style scoped>\n.\n</style>";
        let blocks = scan_sfc_blocks(source);
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
        let blocks = scan_sfc_blocks(source);
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

    /// @ai-generated - After '.' in value context (e.g., border: .5px) no class completions
    #[test]
    fn test_no_selector_completion_in_value() {
        let source = "<template><div class=\"foo\"></div></template>\n<style scoped>\n.foo { border: .5px; }\n</style>";
        let blocks = scan_sfc_blocks(source);
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

    /// @ai-generated - Hover on CSS selector with no matches shows "no matching"
    #[test]
    fn test_hover_on_selector_no_matches() {
        let source = "<template><div></div></template>\n<style scoped>\n.nonexistent { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
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
