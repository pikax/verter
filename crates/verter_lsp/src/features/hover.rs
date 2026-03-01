// Phase 2: Hover — binding name, kind, source location from verter_host analysis.
// Phase 3: Enhanced with full resolved type signature, JSDoc from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Attempt to provide hover information at a given position.
///
/// Strategy:
/// 1. Find which SFC block the position is in
/// 2. Extract the word at the cursor position
/// 3. Look up that word in the analysis data (bindings, imports, macros)
/// 4. Format hover content as markdown with binding kind, reactivity, type info
pub fn hover_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Hover> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;

    // Determine which block the cursor is in
    let block = blocks.iter().find(|b| {
        let (content_start, content_end) = b.content_range();
        offset >= content_start as usize && offset < content_end as usize
    })?;

    match block.tag_name.as_str() {
        "script" => hover_in_script(offset, source, analysis),
        "template" => hover_in_template(offset, source, analysis),
        "style" => crate::css::css_hover(position, source, blocks, Some(analysis), line_index),
        _ => None,
    }
}

fn hover_in_script(offset: usize, source: &str, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    // Check if the cursor is on a Vue API call site — add context if so
    if let Some(api_hover) = vue_api_hover_at_offset(offset as u32, analysis) {
        return Some(api_hover);
    }

    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

fn hover_in_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    // Don't provide hover inside HTML comments
    if crate::features::definition::is_inside_html_comment(source, offset) {
        return None;
    }

    // Check if cursor is on a template element — show matching CSS rules
    if let Some(hover) = element_css_hover(offset as u32, analysis) {
        return Some(hover);
    }

    // Check if cursor is on a component element — show prop constness info
    if let Some(hover) = component_prop_constness_hover(offset as u32, analysis) {
        return Some(hover);
    }

    // In template, look for bindings used in expressions like {{ myVar }}
    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

/// When hovering on a template element, show matching CSS rules with specificity.
fn element_css_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_ref()?;

    // Find element at cursor position (on the tag name)
    let (el_idx, element) = template
        .elements
        .iter()
        .enumerate()
        .find(|(_, el)| offset >= el.span.start && offset <= el.span.end)?;

    // Collect matching selectors from all style blocks
    let mut matches: Vec<(&str, (u32, u32, u32), verter_analysis::MatchResult)> = Vec::new();

    for style in &analysis.styles {
        if let Some(css) = &style.css {
            for sel in &css.selectors {
                if let Some(ref structure) = sel.structure {
                    let result =
                        verter_analysis::match_selector(structure, el_idx, &template.elements);
                    if !matches!(result, verter_analysis::MatchResult::NoMatch) {
                        matches.push((&sel.text, sel.specificity, result));
                    }
                }
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    // Sort by specificity (highest first)
    matches.sort_by(|a, b| b.1.cmp(&a.1));

    let classes: Vec<&str> = element.static_classes().collect();
    let class_info = if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", classes.join(" "))
    };
    let id_info = element
        .static_id()
        .map(|id| format!(" id=\"{id}\""))
        .unwrap_or_default();

    let mut lines = Vec::new();
    lines.push(format!(
        "**`<{}{id_info}{class_info}>`**\n\n**CSS rules ({}):**",
        element.tag,
        matches.len()
    ));

    for (text, spec, result) in &matches {
        let certainty = match result {
            verter_analysis::MatchResult::Matches => "",
            verter_analysis::MatchResult::MaybeMatches => " *(maybe)*",
            verter_analysis::MatchResult::NoMatch => unreachable!(),
        };
        lines.push(format!(
            "- `{}` — specificity `({}, {}, {})`{certainty}",
            text, spec.0, spec.1, spec.2,
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

/// When hovering on a component element in the template, show prop constness info.
///
/// This helps visualize cross-file optimization: which props are always const
/// (optimizable) vs dynamic (require reactive tracking).
fn component_prop_constness_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_ref()?;

    // Find component usage at cursor position
    let comp = template
        .components
        .iter()
        .find(|c| offset >= c.span.start && offset <= c.span.end)?;

    if comp.props.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    let source_info = comp
        .import_source
        .as_deref()
        .map(|s| format!(" (from `{s}`)"))
        .unwrap_or_default();
    lines.push(format!("**`<{}>`**{source_info}\n", comp.name));
    lines.push("**Props:**".to_string());

    for prop in &comp.props {
        let constness_label = match prop.constness {
            verter_analysis::template::PropValueConstness::Const => "const",
            verter_analysis::template::PropValueConstness::Dynamic => "dynamic",
            verter_analysis::template::PropValueConstness::Unknown => "unknown",
        };
        let icon = match prop.constness {
            verter_analysis::template::PropValueConstness::Const => "\u{2713}", // ✓
            verter_analysis::template::PropValueConstness::Dynamic => "\u{2197}", // ↗
            verter_analysis::template::PropValueConstness::Unknown => "?",
        };
        let bound = if prop.is_bound { ":" } else { "" };
        lines.push(format!(
            "- {icon} `{bound}{}` — *{constness_label}*",
            prop.name
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

/// Check if the offset is on a Vue API call site name, and if so return a hover
/// with Vue API context (category, sync requirement, description).
fn vue_api_hover_at_offset(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let call = analysis
        .vue_api_calls
        .iter()
        .find(|c| offset >= c.span.start && offset < c.span.end)?;

    let api = &call.api;
    let name = api.display_name();

    let mut lines = Vec::new();

    lines.push(format!("```typescript\n{name}()\n```"));

    // Category label
    let category = if api.is_lifecycle() {
        "Lifecycle Hook"
    } else if api.is_watcher() {
        "Watcher"
    } else if matches!(
        api,
        verter_analysis::VueApiClassification::Provide
            | verter_analysis::VueApiClassification::Inject
    ) {
        "Dependency Injection"
    } else if matches!(
        api,
        verter_analysis::VueApiClassification::Ref
            | verter_analysis::VueApiClassification::ShallowRef
            | verter_analysis::VueApiClassification::Reactive
            | verter_analysis::VueApiClassification::ShallowReactive
            | verter_analysis::VueApiClassification::Computed
            | verter_analysis::VueApiClassification::ToRef
            | verter_analysis::VueApiClassification::ToRefs
            | verter_analysis::VueApiClassification::Readonly
            | verter_analysis::VueApiClassification::ShallowReadonly
            | verter_analysis::VueApiClassification::CustomRef
            | verter_analysis::VueApiClassification::TriggerRef
    ) {
        "Reactivity Primitive"
    } else {
        "Vue API"
    };

    lines.push(format!("*{category}*"));

    if api.requires_sync_context() {
        lines.push("Must be called during synchronous `setup()` execution.".to_string());
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    })
}

fn hover_for_word(word: &str, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    // Check bindings
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        return Some(format_binding_hover(binding));
    }

    // Check imports
    for import in &analysis.imports {
        if let Some(binding) = import.bindings.iter().find(|b| b.name == word) {
            return Some(format_import_hover(binding, &import.source));
        }
    }

    // Check macros
    for mac in &analysis.macros {
        if mac.binding_name.as_ref().is_some_and(|name| name == word) {
            return Some(format_macro_hover(mac));
        }
    }

    None
}

fn format_binding_hover(binding: &verter_analysis::AnalyzedBinding) -> Hover {
    let mut lines = Vec::new();

    let kind_str = match binding.kind {
        verter_analysis::AnalyzedBindingKind::Const => "const",
        verter_analysis::AnalyzedBindingKind::Let => "let",
        verter_analysis::AnalyzedBindingKind::Var => "var",
        verter_analysis::AnalyzedBindingKind::Function => "function",
        verter_analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_analysis::AnalyzedBindingKind::Class => "class",
    };

    // Show type annotation if available
    let type_str = binding
        .type_annotation
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();

    lines.push(format!(
        "```typescript\n{kind_str} {}{type_str}\n```",
        binding.name
    ));

    // Show granular reactivity kind
    match binding.reactivity_kind {
        verter_analysis::ReactivityKind::None => {
            if binding.is_reactive {
                lines.push("*(reactive)*".to_string());
            }
        }
        verter_analysis::ReactivityKind::Ref => lines.push("*(ref — needs `.value`)*".to_string()),
        verter_analysis::ReactivityKind::Computed => {
            lines.push("*(computed — needs `.value`, read-only)*".to_string());
        }
        verter_analysis::ReactivityKind::Reactive => {
            lines.push("*(reactive — direct property access)*".to_string());
        }
        verter_analysis::ReactivityKind::MaybeRef => {
            lines.push("*(maybe ref — may need `.value`)*".to_string());
        }
        verter_analysis::ReactivityKind::Mutable => {
            lines.push("*(mutable — reassignable)*".to_string());
        }
    }

    if let Some(ref init) = binding.initializer {
        match init {
            verter_analysis::BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            } => {
                let source_info = callee_import_source
                    .as_ref()
                    .map(|s| format!(" (from `{s}`)"))
                    .unwrap_or_default();
                lines.push(format!("Initialized via `{callee}()`{source_info}"));
            }
            verter_analysis::BindingInitializer::Literal { kind } => {
                lines.push(format!("Literal: {kind:?}"));
            }
            verter_analysis::BindingInitializer::Reference { name } => {
                lines.push(format!("References `{name}`"));
            }
            verter_analysis::BindingInitializer::Other => {}
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_import_hover(binding: &verter_analysis::AnalyzedImportBinding, source: &str) -> Hover {
    let type_prefix = if binding.is_type_only { "type " } else { "" };
    let mut lines = vec![format!(
        "```typescript\nimport {type_prefix}{{ {} }} from '{}'\n```",
        binding.name, source
    )];

    if let Some(ref api) = binding.vue_api {
        lines.push(format!("Vue API: `{api:?}`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_macro_hover(mac: &verter_analysis::AnalyzedMacro) -> Hover {
    let macro_name = match mac.kind {
        verter_analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
    };

    let mut lines = Vec::new();

    if let Some(ref binding) = mac.binding_name {
        lines.push(format!(
            "```typescript\nconst {binding} = {macro_name}()\n```"
        ));
    } else {
        lines.push(format!("```typescript\n{macro_name}()\n```"));
    }

    if mac.is_type_based {
        let types = if mac.type_references.is_empty() {
            "inline type".to_string()
        } else {
            mac.type_references.join(", ")
        };
        lines.push(format!("Type-based: `<{types}>`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

use crate::utils::word_at_offset;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::types::VueApiCallSite;
    use verter_analysis::*;

    fn make_analysis(
        bindings: Vec<AnalyzedBinding>,
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            bindings,
            imports,
            macros,
            ..Default::default()
        }
    }

    #[test]
    fn test_word_at_offset() {
        assert_eq!(word_at_offset("const foo = 1", 6), Some("foo".to_string()));
        assert_eq!(word_at_offset("const foo = 1", 5), None); // space
        assert_eq!(word_at_offset("hello", 0), Some("hello".to_string()));
        assert_eq!(word_at_offset("hello", 4), Some("hello".to_string()));
        assert_eq!(word_at_offset("", 0), None);
    }

    #[test]
    fn test_hover_on_binding() {
        let source = "<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: Some(BindingInitializer::FunctionCall {
                    callee: "ref".to_string(),
                    callee_import_source: Some("vue".to_string()),
                    vue_api: Some(VueApiClassification::Ref),
                }),
                span: verter_span::Span::new(0, 0),
            }],
            vec![],
            vec![],
        );

        // Hover on "count" — find its offset
        let offset = source.find("count").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_some());
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("const count"));
        assert!(contents.contains("reactive"));
        assert!(contents.contains("ref()"));
    }

    #[test]
    fn test_hover_on_import() {
        let source = "<script setup>\nimport { ref } from 'vue'\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span: verter_span::Span::new(0, 0),
                }],
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let ref_offset = source.find("ref").unwrap();
        let position = line_index.offset_to_position(ref_offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_some());
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("import"));
        assert!(contents.contains("'vue'"));
    }

    #[test]
    fn test_hover_outside_blocks() {
        let source = "<!-- comment -->\n<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(vec![], vec![], vec![]);

        // Position in the comment (outside blocks)
        let position = Position {
            line: 0,
            character: 5,
        };
        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_none());
    }

    #[test]
    fn test_hover_on_template_binding() {
        let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(0, 0),
            }],
            vec![],
            vec![],
        );

        // Find "count" in the template
        let offset = source.find("count").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_some());
    }

    #[test]
    fn test_hover_on_vue_api_call_site() {
        let source =
            "<script setup>\nimport { onMounted } from 'vue'\nonMounted(() => {})\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // Offset of "onMounted(() => {})" call
        let call_offset = source.find("onMounted(() =>").unwrap();

        let analysis = FileAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: verter_span::Span::new(
                    call_offset as u32,
                    (call_offset + "onMounted".len()) as u32,
                ),
                arg_value: None,
                is_async_callback: false,
            }],
            ..Default::default()
        };

        let position = line_index.offset_to_position(call_offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_some());
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("onMounted()"));
        assert!(contents.contains("Lifecycle Hook"));
        assert!(contents.contains("synchronous"));
    }

    #[test]
    fn test_no_hover_on_unknown_word() {
        let source = "<script setup>\nconst unknownVar = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // Empty analysis — no bindings registered
        let analysis = make_analysis(vec![], vec![], vec![]);

        let offset = source.find("unknownVar").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_none());
    }

    /// @ai-generated - No hover on identifier inside HTML comment in template
    #[test]
    fn test_no_hover_inside_html_comment() {
        let source = "<template>\n  <!-- count -->\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(0, 0),
            }],
            vec![],
            vec![],
        );

        // Hover on "count" inside the comment — should return None
        let offset = source.find("count").unwrap();
        assert!(
            source[..offset].contains("<!--"),
            "should be inside comment"
        );
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_none(), "should not hover inside HTML comment");

        // Hover on "count" in the interpolation — should work
        let second_offset = source[offset + 5..].find("count").unwrap() + offset + 5;
        let position2 = line_index.offset_to_position(second_offset as u32).unwrap();

        let hover2 = hover_at_position(&position2, source, &blocks, Some(&analysis), &line_index);
        assert!(hover2.is_some(), "should hover on binding outside comment");
    }

    /// @ai-generated — Hover on component element shows prop constness info
    #[test]
    fn test_hover_on_component_shows_prop_constness() {
        let source =
            "<template>\n  <MyButton :title=\"msg\" disabled>\n  </MyButton>\n</template>\n\n<script setup>\nimport MyButton from './MyButton.vue'\nconst msg = ref('hello')\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let comp_offset = source.find("<MyButton").unwrap();

        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                components: vec![verter_analysis::template::TemplateComponentUsage {
                    name: "MyButton".into(),
                    import_source: Some("./MyButton.vue".into()),
                    is_dynamic: false,
                    props: vec![
                        verter_analysis::template::TemplatePropUsage {
                            name: "title".into(),
                            is_bound: true,
                            constness: verter_analysis::template::PropValueConstness::Dynamic,
                            referenced_bindings: vec!["msg".into()],
                            from_spread: false,
                            span: verter_span::Span::new(
                                (comp_offset + 10) as u32,
                                (comp_offset + 22) as u32,
                            ),
                        },
                        verter_analysis::template::TemplatePropUsage {
                            name: "disabled".into(),
                            is_bound: false,
                            constness: verter_analysis::template::PropValueConstness::Const,
                            referenced_bindings: vec![],
                            from_spread: false,
                            span: verter_span::Span::new(
                                (comp_offset + 23) as u32,
                                (comp_offset + 31) as u32,
                            ),
                        },
                    ],
                    has_spread: false,
                    slots_used: vec![],
                    static_classes: vec![],
                    has_dynamic_class: false,
                    dynamic_classes: vec![],
                    v_models: vec![],
                    span: verter_span::Span::new(comp_offset as u32, (comp_offset + 40) as u32),
                }],
                elements: vec![],
                ..Default::default()
            }),
            ..Default::default()
        };

        // Hover on "MyButton" tag name
        let pos = line_index
            .offset_to_position((comp_offset + 1) as u32)
            .unwrap();
        let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index);

        assert!(hover.is_some(), "should provide hover on component element");
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("MyButton"), "should show component name");
        assert!(
            contents.contains("title") && contents.contains("dynamic"),
            "should show title prop as dynamic: {}",
            contents
        );
        assert!(
            contents.contains("disabled") && contents.contains("const"),
            "should show disabled prop as const: {}",
            contents
        );
    }

    /// @ai-generated - Hover on template element shows matching CSS rules
    #[test]
    fn test_hover_on_element_shows_css_rules() {
        let source = "<template>\n  <div class=\"foo\">hello</div>\n</template>\n\n<style scoped>\n.foo { color: red; }\ndiv { font-size: 14px; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // Build style analysis from the actual CSS content
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (content_start, content_end) = style_block.content_range();
        let css_content = &source[content_start as usize..content_end as usize];

        let style = verter_analysis::style::build_css_style_analysis(
            css_content,
            verter_analysis::style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            true,
            false,
            None,
            content_start,
        );

        // Find the div element's offset in template
        let div_offset = source.find("<div class").unwrap();

        let analysis = FileAnalysisSnapshot {
            styles: vec![style],
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "div".into(),
                    is_component: false,
                    is_self_closing: false,
                    namespace: ElementNamespace::Html,
                    attributes: vec![TemplateAttribute {
                        name: "class".into(),
                        value: Some("foo".into()),
                        is_dynamic: false,
                        span: verter_span::Span::new(0, 0),
                    }],
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
                    has_element_children: false,
                    nesting_depth: 0,
                    parent_tag: None,
                    parent_index: None,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(div_offset as u32, (div_offset + 20) as u32),
                    tag_span_end: (div_offset + 20) as u32,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        // Hover on the "div" tag name
        let pos = line_index
            .offset_to_position((div_offset + 1) as u32)
            .unwrap();
        let hover = hover_at_position(&pos, source, &blocks, Some(&analysis), &line_index);

        assert!(hover.is_some(), "should provide hover on template element");
        let contents = match hover.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(
            contents.contains("CSS rules"),
            "should show CSS rules section"
        );
        assert!(contents.contains(".foo"), "should list .foo selector");
        assert!(contents.contains("div"), "should list div selector");
    }
}
