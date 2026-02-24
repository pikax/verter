// Phase 2: Document symbols from SFC structure + verter_host analysis.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Build a document symbol tree from SFC blocks and analysis data.
///
/// Returns a hierarchical structure:
/// - Top-level: SFC blocks (script, template, style)
/// - Children of script: bindings, imports, macros from analysis
///
/// ## Analysis data needed (not yet available with positions):
/// - `AnalyzedBinding` with byte span (`span_start`, `span_end`) — needed for
///   precise child symbol ranges within script blocks
/// - `AnalyzedImport` with byte span — needed for import symbol positions
/// - `AnalyzedMacro` with byte span — needed for macro symbol positions
///
/// Currently, child symbols use the entire block content range as a fallback
/// until span data is added to `verter_analysis`.
pub fn build_document_symbols(
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

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

        let children = if block.tag_name == "script" {
            analysis
                .map(|a| build_script_children(a, block, line_index))
                .unwrap_or_default()
        } else {
            None
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
    for mac in &analysis.macros {
        let name = match &mac.binding_name {
            Some(binding) => format!("{binding} = {}()", macro_kind_display(&mac.kind)),
            None => format!("{}()", macro_kind_display(&mac.kind)),
        };

        let range = span_to_range(mac.span_start, mac.span_end, line_index, fallback_range);

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
            binding.span_start,
            binding.span_end,
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
                import.span_start,
                import.span_end,
                line_index,
                fallback_range,
            );
            let selection_range =
                span_to_range(binding.span_start, binding.span_end, line_index, range);

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

fn binding_symbol_kind(kind: &verter_analysis::AnalyzedBindingKind) -> SymbolKind {
    match kind {
        verter_analysis::AnalyzedBindingKind::Const => SymbolKind::CONSTANT,
        verter_analysis::AnalyzedBindingKind::Let | verter_analysis::AnalyzedBindingKind::Var => {
            SymbolKind::VARIABLE
        }
        verter_analysis::AnalyzedBindingKind::Function
        | verter_analysis::AnalyzedBindingKind::AsyncFunction => SymbolKind::FUNCTION,
        verter_analysis::AnalyzedBindingKind::Class => SymbolKind::CLASS,
    }
}

fn build_binding_detail(binding: &verter_analysis::AnalyzedBinding) -> Option<String> {
    let mut parts = Vec::new();

    match binding.kind {
        verter_analysis::AnalyzedBindingKind::Const => parts.push("const"),
        verter_analysis::AnalyzedBindingKind::Let => parts.push("let"),
        verter_analysis::AnalyzedBindingKind::Var => parts.push("var"),
        verter_analysis::AnalyzedBindingKind::Function => parts.push("function"),
        verter_analysis::AnalyzedBindingKind::AsyncFunction => parts.push("async function"),
        verter_analysis::AnalyzedBindingKind::Class => parts.push("class"),
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

fn macro_kind_display(kind: &verter_analysis::AnalyzedMacroKind) -> &'static str {
    match kind {
        verter_analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
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
            macro_type_deps: vec![],
            script_flags: 0,
            styles: vec![],
            template: None,
        }
    }

    #[test]
    fn test_basic_sfc_structure() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>\n\n<style scoped>\n.foo {}\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

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
        let line_index = LineIndex::new(source);

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
                }),
                span_start: 0,
                span_end: 0,
            }],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span_start: 0,
                    span_end: 0,
                }],
                span_start: 0,
                span_end: 0,
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
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![],
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("props".to_string()),
                span_start: 0,
                span_end: 0,
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
        let line_index = LineIndex::new(source);

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
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(vec![], vec![], vec![]);
        let symbols = build_document_symbols(&blocks, Some(&analysis), &line_index);
        assert!(symbols[0].children.is_none());
    }
}
