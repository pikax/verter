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
        _ => None,
    }
}

fn hover_in_script(offset: usize, source: &str, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

fn hover_in_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    // In template, look for bindings used in expressions like {{ myVar }}
    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
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

/// Extract the word (identifier) at the given byte offset.
fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return None;
    }

    // Check if the byte at offset is part of an identifier
    if !is_ident_byte(bytes[offset]) {
        return None;
    }

    // Scan backwards to find word start
    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    // Scan forwards to find word end
    let mut end = offset;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
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
        let line_index = LineIndex::new(source);

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
                span_start: 0,
                span_end: 0,
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
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![],
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
        let line_index = LineIndex::new(source);

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
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span_start: 0,
                span_end: 0,
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
    fn test_no_hover_on_unknown_word() {
        let source = "<script setup>\nconst unknownVar = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        // Empty analysis — no bindings registered
        let analysis = make_analysis(vec![], vec![], vec![]);

        let offset = source.find("unknownVar").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let hover = hover_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(hover.is_none());
    }
}
