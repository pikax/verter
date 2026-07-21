// Document symbols from SFC structure + verter_session analysis.

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Build a document symbol tree from SFC blocks and analysis data.
///
/// Returns a hierarchical structure:
/// - Top-level: SFC blocks (script, template, style)
/// - Children of script: bindings, imports, macros from analysis
/// - Children of template: component usages and root elements from analysis
/// - Children of style: CSS classes, custom properties, at-rules from analysis
pub fn build_document_symbols(
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut style_index = 0usize;

    for block in blocks {
        let open_start = line_index
            .offset_to_position(block.open_tag_start)
            .unwrap_or_default();
        let close_end = line_index
            .offset_to_position(block.close_tag_end)
            .unwrap_or_default();
        let (content_start_offset, _) = block.content_range();
        let content_start = line_index
            .offset_to_position(content_start_offset)
            .unwrap_or_default();

        let detail = build_block_detail(block);
        let kind = block_symbol_kind(&block.tag_name);

        let children = match block.tag_name.as_str() {
            "script" => analysis
                .map(|a| build_script_children(a, block, line_index))
                .unwrap_or_default(),
            "template" => analysis.and_then(|a| build_template_children(a, block, line_index)),
            "style" => {
                let result =
                    analysis.and_then(|a| build_style_children(a, style_index, block, line_index));
                style_index += 1;
                result
            }
            _ => None,
        };

        #[allow(deprecated)] // DocumentSymbol::deprecated field is deprecated itself
        symbols.push(DocumentSymbol {
            name: format_block_name(block),
            detail,
            kind,
            tags: None,
            deprecated: None,
            range: Range {
                start: open_start,
                end: close_end,
            },
            selection_range: Range {
                start: open_start,
                end: content_start,
            },
            children,
        });
    }

    symbols
}

fn format_block_name(block: &SfcBlock) -> String {
    let mut name = block.tag_name.clone();
    if block.is_setup() {
        name.push_str(" setup");
    }
    if let Some(lang) = block.lang() {
        name.push_str(&format!(" ({lang})"));
    }
    if block.is_scoped() {
        name.push_str(" scoped");
    }
    name
}

fn build_block_detail(block: &SfcBlock) -> Option<String> {
    match block.tag_name.as_str() {
        "script" if block.is_setup() => Some("script setup".to_string()),
        "script" => Some("options script".to_string()),
        "style" => {
            let mut parts = Vec::new();
            if let Some(lang) = block.lang() {
                parts.push(lang.to_string());
            }
            if block.is_scoped() {
                parts.push("scoped".to_string());
            }
            if block.is_module() {
                parts.push("module".to_string());
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(", "))
            }
        }
        _ => None,
    }
}

fn block_symbol_kind(tag_name: &str) -> SymbolKind {
    match tag_name {
        "script" => SymbolKind::MODULE,
        "template" => SymbolKind::STRUCT,
        "style" => SymbolKind::FILE,
        _ => SymbolKind::NAMESPACE, // custom blocks
    }
}

/// Build child symbols for bindings, imports, and macros within a script block.
///
/// Uses `span_start`/`span_end` byte offsets from analysis data for precise
/// positioning of child symbols. Falls back to the entire block content range
/// when spans are zero (not yet populated).
fn build_script_children(
    analysis: &FileAnalysisSnapshot,
    block: &SfcBlock,
    line_index: &LineIndex,
) -> Option<Vec<DocumentSymbol>> {
    let mut children = Vec::new();
    let (content_start, content_end) = block.content_range();

    // Fallback range: entire block content
    let fallback_start = line_index
        .offset_to_position(content_start)
        .unwrap_or_default();
    let fallback_end = line_index
        .offset_to_position(content_end)
        .unwrap_or_default();
    let fallback_range = Range {
        start: fallback_start,
        end: fallback_end,
    };

    // Add macros
    for mac in analysis.macros.iter() {
        let name = match &mac.binding_name {
            Some(binding) => format!("{binding} = {}()", macro_kind_display(&mac.kind)),
            None => format!("{}()", macro_kind_display(&mac.kind)),
        };

        let range = span_to_range(mac.span.start, mac.span.end, line_index, fallback_range);

        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name,
            detail: Some(macro_kind_display(&mac.kind).to_string()),
            kind: SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    // Add bindings
    for binding in &analysis.bindings {
        let kind = binding_symbol_kind(&binding.kind);
        let detail = build_binding_detail(binding);
        let range = span_to_range(
            binding.span.start,
            binding.span.end,
            line_index,
            fallback_range,
        );

        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: binding.name.clone(),
            detail,
            kind,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    // Add imports
    for import in &analysis.imports {
        for binding in &import.bindings {
            let detail = Some(format!("from '{}'", import.source));
            let kind = if binding.is_type_only || import.is_type_only {
                SymbolKind::TYPE_PARAMETER
            } else {
                SymbolKind::VARIABLE
            };

            let range = span_to_range(
                import.span.start,
                import.span.end,
                line_index,
                fallback_range,
            );
            let selection_range =
                span_to_range(binding.span.start, binding.span.end, line_index, range);

            #[allow(deprecated)]
            children.push(DocumentSymbol {
                name: binding.name.clone(),
                detail,
                kind,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            });
        }
    }

    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

/// Convert analysis span offsets to an LSP Range, falling back if spans are zero.
fn span_to_range(span_start: u32, span_end: u32, line_index: &LineIndex, fallback: Range) -> Range {
    if span_start == 0 && span_end == 0 {
        return fallback;
    }
    let start = line_index
        .offset_to_position(span_start)
        .unwrap_or(fallback.start);
    let end = line_index
        .offset_to_position(span_end)
        .unwrap_or(fallback.end);
    Range { start, end }
}

fn binding_symbol_kind(kind: &verter_semantic::analysis::AnalyzedBindingKind) -> SymbolKind {
    match kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => SymbolKind::CONSTANT,
        verter_semantic::analysis::AnalyzedBindingKind::Let
        | verter_semantic::analysis::AnalyzedBindingKind::Var => SymbolKind::VARIABLE,
        verter_semantic::analysis::AnalyzedBindingKind::Function
        | verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => SymbolKind::FUNCTION,
        verter_semantic::analysis::AnalyzedBindingKind::Class => SymbolKind::CLASS,
    }
}

fn build_binding_detail(binding: &verter_semantic::analysis::AnalyzedBinding) -> Option<String> {
    let mut parts = Vec::new();

    match binding.kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => parts.push("const"),
        verter_semantic::analysis::AnalyzedBindingKind::Let => parts.push("let"),
        verter_semantic::analysis::AnalyzedBindingKind::Var => parts.push("var"),
        verter_semantic::analysis::AnalyzedBindingKind::Function => parts.push("function"),
        verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => {
            parts.push("async function")
        }
        verter_semantic::analysis::AnalyzedBindingKind::Class => parts.push("class"),
    }

    if binding.is_reactive {
        parts.push("(reactive)");
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Build child symbols for template elements and components.
///
/// Shows root-level elements (nesting_depth == 0) and component usages
/// with their props as children.
fn build_template_children(
    analysis: &FileAnalysisSnapshot,
    block: &SfcBlock,
    line_index: &LineIndex,
) -> Option<Vec<DocumentSymbol>> {
    let template = analysis.template.as_deref()?;
    let mut children = Vec::new();
    let (content_start, content_end) = block.content_range();
    let fallback_range = Range {
        start: line_index
            .offset_to_position(content_start)
            .unwrap_or_default(),
        end: line_index
            .offset_to_position(content_end)
            .unwrap_or_default(),
    };

    // Add component usages (with precise spans)
    for comp in &template.components {
        let detail = comp.import_source.as_deref().map(|s| format!("from '{s}'"));
        let range = span_to_range(comp.span.start, comp.span.end, line_index, fallback_range);

        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: format!("<{}>", comp.name),
            detail,
            kind: SymbolKind::OBJECT,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    // Add root-level native elements (depth 0, not components)
    for elem in &template.elements {
        if elem.nesting_depth != 0 || elem.is_component {
            continue;
        }
        let range = span_to_range(elem.span.start, elem.span.end, line_index, fallback_range);

        let detail = if !elem.directives.is_empty() {
            let dirs: Vec<_> = elem
                .directives
                .iter()
                .map(|d| d.raw_name.as_str())
                .collect();
            Some(dirs.join(" "))
        } else {
            None
        };

        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: format!("<{}>", elem.tag),
            detail,
            kind: SymbolKind::FIELD,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

/// Build child symbols for CSS classes, custom properties, and at-rules in a style block.
///
/// Uses the style block index to match the correct `StyleBlockAnalysis` entry.
fn build_style_children(
    analysis: &FileAnalysisSnapshot,
    style_index: usize,
    block: &SfcBlock,
    line_index: &LineIndex,
) -> Option<Vec<DocumentSymbol>> {
    let style_analysis = analysis.styles.get(style_index)?;
    let css = style_analysis.css.as_ref()?;

    let mut children = Vec::new();
    let (content_start, content_end) = block.content_range();
    let fallback_range = Range {
        start: line_index
            .offset_to_position(content_start)
            .unwrap_or_default(),
        end: line_index
            .offset_to_position(content_end)
            .unwrap_or_default(),
    };

    // CSS class selectors
    for class in &css.classes {
        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: format!(".{}", class.name),
            detail: Some("class".to_string()),
            kind: SymbolKind::STRING,
            tags: None,
            deprecated: None,
            range: fallback_range,
            selection_range: fallback_range,
            children: None,
        });
    }

    // CSS custom properties
    for prop in &css.custom_properties {
        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: prop.name.clone(),
            detail: Some("custom property".to_string()),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range: fallback_range,
            selection_range: fallback_range,
            children: None,
        });
    }

    // At-rules
    for rule in &css.at_rules {
        #[allow(deprecated)]
        children.push(DocumentSymbol {
            name: format!("@{}", rule.name),
            detail: Some(format!("{:?}", rule.kind)),
            kind: SymbolKind::NAMESPACE,
            tags: None,
            deprecated: None,
            range: fallback_range,
            selection_range: fallback_range,
            children: None,
        });
    }

    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

fn macro_kind_display(kind: &verter_semantic::analysis::AnalyzedMacroKind) -> &'static str {
    match kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_semantic::analysis::style::{AnalyzedCssClass, CssAnalysis, StyleBlockAnalysis};
    use verter_semantic::analysis::types::ImportBindingKind;
    use verter_semantic::analysis::*;

    fn make_analysis(
        bindings: Vec<AnalyzedBinding>,
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            bindings,
            imports,
            macros: macros.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_basic_sfc_structure() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>\n\n<style scoped>\n.foo {}\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let symbols = build_document_symbols(&blocks, None, &line_index);
        assert_eq!(symbols.len(), 3);
        assert!(symbols[0].name.contains("template"));
        assert!(symbols[1].name.contains("script setup"));
        assert!(symbols[2].name.contains("style"));
        assert!(symbols[2].name.contains("scoped"));
    }

    #[test]
    fn test_script_children_from_analysis() {
        let source = "<script setup lang=\"ts\">\nconst x = ref(0);\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "x".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: Some(BindingInitializer::FunctionCall {
                    callee: "ref".to_string(),
                    callee_import_source: Some("vue".to_string()),
                    vue_api: Some(VueApiClassification::Ref),
                    async_component_source: None,
                }),
                span: verter_span::Span::new(0, 0),
                used_in_script: false,
                used_in_style: false,
            }],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span: verter_span::Span::new(0, 0),
                }],
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        assert_eq!(symbols.len(), 1);

        let script = &symbols[0];
        let children = script.children.as_ref().unwrap();
        assert_eq!(children.len(), 2); // binding + import
        assert_eq!(children[0].name, "x");
        assert_eq!(children[0].kind, SymbolKind::CONSTANT);
        assert_eq!(children[1].name, "ref");
    }

    #[test]
    fn test_macro_symbols() {
        let source = "<script setup>\nconst props = defineProps<{ msg: string }>()\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![],
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                parsed_type_argument_scope: None,
                span: verter_span::Span::new(0, 0),
            }],
        );

        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert!(children[0].name.contains("props"));
        assert!(children[0].name.contains("defineProps"));
    }

    #[test]
    fn test_block_naming() {
        let source =
            "<script setup lang=\"ts\">\n</script>\n<style scoped lang=\"scss\">\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let symbols = build_document_symbols(&blocks, None, &line_index);
        assert_eq!(symbols[0].name, "script setup (ts)");
        assert!(symbols[1].name.contains("style"));
        assert!(symbols[1].name.contains("scss"));
        assert!(symbols[1].name.contains("scoped"));
    }

    #[test]
    fn test_no_children_for_empty_analysis() {
        let source = "<script setup>\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(vec![], vec![], vec![]);
        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        assert!(symbols[0].children.is_none());
    }

    #[test]
    fn test_template_children_components() {
        let source = "<template>\n<MyButton @click=\"handle\">Click</MyButton>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            template: Some(
                (TemplateAnalysisSnapshot {
                    components: vec![TemplateComponentUsage {
                        name: "MyButton".to_string(),
                        import_source: Some("./MyButton.vue".to_string()),
                        is_dynamic: false,
                        props: vec![],
                        has_spread: false,
                        slots_used: vec![],
                        static_classes: vec![],
                        has_dynamic_class: false,
                        dynamic_classes: vec![],
                        v_models: vec![],
                        bindings: vec![],
                        events: vec![],
                        span: verter_span::Span::new(11, 52),
                    }],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        let template = &symbols[0];
        let children = template.children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "<MyButton>");
        assert_eq!(children[0].detail.as_deref(), Some("from './MyButton.vue'"));
    }

    #[test]
    fn test_template_children_root_elements() {
        let source = "<template>\n<div><span>inner</span></div>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements: vec![
                        TemplateElement {
                            tag: "div".to_string(),
                            nesting_depth: 0,
                            dynamic_classes: vec![],
                            span: verter_span::Span::new(11, 42),
                            content_end: 0,
                            ..Default::default()
                        },
                        TemplateElement {
                            tag: "span".to_string(),
                            nesting_depth: 1,
                            dynamic_classes: vec![],
                            span: verter_span::Span::new(16, 35),
                            content_end: 0,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        };

        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        let template = &symbols[0];
        let children = template.children.as_ref().unwrap();
        // Only root element (depth 0) shown
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "<div>");
    }

    #[test]
    fn test_style_children_classes() {
        let source =
            "<style scoped>\n.container { padding: 16px; }\n.title { font-size: 2rem; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            styles: (vec![StyleBlockAnalysis {
                scoped: true,
                css: Some(CssAnalysis {
                    classes: vec![
                        AnalyzedCssClass {
                            name: "container".to_string(),
                            span: verter_span::Span::new(0, 0),
                            selector_index: None,
                        },
                        AnalyzedCssClass {
                            name: "title".to_string(),
                            span: verter_span::Span::new(0, 0),
                            selector_index: None,
                        },
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            }])
            .into(),
            ..Default::default()
        };

        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        let style = &symbols[0];
        let children = style.children.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, ".container");
        assert_eq!(children[1].name, ".title");
    }
}
