// Phase 2: Go-to-definition — span-based navigation from verter_host analysis.
// Phase 3: Enhanced with type definition through generics/aliases/re-exports from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Sentinel URI used when a definition is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Attempt to provide go-to-definition at a given position.
///
/// Strategy:
/// 1. Find the word at the cursor position
/// 2. Look it up in analysis data:
///    - If it's an imported binding with `resolved_canonical_id`, navigate to the source file
///    - If it's an imported binding without resolution, navigate to the import statement
///    - If it's a script binding (in template context), navigate to its span in script
///    - If it's a macro binding name, navigate to the macro call span
pub fn definition_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;

    let word = word_at_offset(source, offset)?;

    // Check if the word is an import binding — navigate to source file or import statement
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word {
                // If we have a resolved canonical ID, navigate to the source file
                if let Some(ref canonical_id) = import.resolved_canonical_id {
                    return resolved_import_definition(canonical_id);
                }
                // Otherwise, navigate to the import statement itself using span data
                if import.span_start > 0 || import.span_end > 0 {
                    return span_definition(import.span_start, import.span_end, line_index);
                }
                return None;
            }
        }
    }

    // Check if we're in a template block
    let in_template = blocks.iter().any(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if in_template {
        // Find the binding definition using span data
        if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
            if binding.span_start > 0 || binding.span_end > 0 {
                return span_definition(binding.span_start, binding.span_end, line_index);
            }
        }
        // Check macro binding names
        for mac in &analysis.macros {
            if mac.binding_name.as_ref().is_some_and(|n| n == &word)
                && (mac.span_start > 0 || mac.span_end > 0)
            {
                return span_definition(mac.span_start, mac.span_end, line_index);
            }
        }
    }

    // In script context, check if cursor is on a binding name — navigate to its span
    let in_script = blocks.iter().any(|b| {
        b.tag_name == "script" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if in_script {
        // Check if cursor is on an import binding name — navigate to import source
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name == word {
                    if let Some(ref canonical_id) = import.resolved_canonical_id {
                        return resolved_import_definition(canonical_id);
                    }
                }
            }
        }
    }

    None
}

/// Create a definition response from a resolved canonical ID (cross-file navigation).
fn resolved_import_definition(canonical_id: &str) -> Option<GotoDefinitionResponse> {
    // Convert canonical ID back to a file:// URI
    let uri_str = if canonical_id.starts_with('/') {
        format!("file://{canonical_id}")
    } else if canonical_id.chars().nth(1) == Some(':') {
        // Windows drive letter
        format!("file:///{canonical_id}")
    } else {
        return None;
    };

    let uri: Uri = uri_str.parse().ok()?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri,
        range: Range::default(),
    }))
}

/// Create a same-file definition response from analysis span data.
fn span_definition(
    span_start: u32,
    span_end: u32,
    line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: SAME_FILE_URI.parse().unwrap(),
        range: Range { start, end },
    }))
}

/// Extract the word (identifier) at the given byte offset.
fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() || !is_ident_byte(bytes[offset]) {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

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
    fn test_go_to_definition_from_template_to_script_via_span() {
        let source =
            "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        // "const count" in script — find the byte offset of "count" in the declaration
        let script_count_offset = source.rfind("count").unwrap() as u32;
        let script_count_end = script_count_offset + 5;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span_start: script_count_offset,
                span_end: script_count_end,
            }],
            vec![],
            vec![],
        );

        // Click on "count" in template
        let template_count_offset = source.find("count").unwrap();
        let position = line_index
            .offset_to_position(template_count_offset as u32)
            .unwrap();

        let result =
            definition_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
            // Should point to the "count" declaration span in script
            assert_eq!(loc.range.start.line, 5);
            assert_eq!(loc.range.start.character, 6); // after "const "
        } else {
            panic!("expected scalar location");
        }
    }

    #[test]
    fn test_go_to_import_with_resolved_canonical_id() {
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
                span_start: 15,
                span_end: 40,
                resolved_canonical_id: Some("/usr/lib/node_modules/vue/dist/vue.d.ts".to_string()),
            }],
            vec![],
        );

        let ref_offset = source.find("ref").unwrap();
        let position = line_index.offset_to_position(ref_offset as u32).unwrap();

        let result =
            definition_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
            // Should navigate to the resolved file
            assert!(loc.uri.as_str().contains("vue.d.ts"));
        } else {
            panic!("expected scalar location");
        }
    }

    #[test]
    fn test_go_to_import_without_resolution_falls_back_to_import_span() {
        let source = "<script setup>\nimport { helper } from './utils'\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let import_start = source.find("import").unwrap() as u32;
        let import_end = source.find("'./utils'").unwrap() as u32 + 9;

        let analysis = make_analysis(
            vec![],
            vec![AnalyzedImport {
                source: "./utils".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "helper".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span_start: 0,
                    span_end: 0,
                }],
                span_start: import_start,
                span_end: import_end,
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let helper_offset = source.find("helper").unwrap();
        let position = line_index.offset_to_position(helper_offset as u32).unwrap();

        let result =
            definition_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
            // Should point to the import statement span
            let start_pos = line_index.offset_to_position(import_start).unwrap();
            assert_eq!(loc.range.start, start_pos);
        } else {
            panic!("expected scalar location");
        }
    }

    #[test]
    fn test_go_to_macro_binding_from_template() {
        let source = "<template>\n  {{ props.msg }}\n</template>\n\n<script setup>\nconst props = defineProps<{ msg: string }>()\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let macro_start = source.find("defineProps").unwrap() as u32;
        let macro_end = source.rfind("()").unwrap() as u32 + 2;

        let analysis = make_analysis(
            vec![],
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("props".to_string()),
                span_start: macro_start,
                span_end: macro_end,
            }],
        );

        // Click on "props" in template
        let props_offset = source.find("props").unwrap();
        let position = line_index.offset_to_position(props_offset as u32).unwrap();

        let result =
            definition_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
            let expected_start = line_index.offset_to_position(macro_start).unwrap();
            assert_eq!(loc.range.start, expected_start);
        } else {
            panic!("expected scalar location");
        }
    }

    #[test]
    fn test_no_definition_for_unknown_binding() {
        let source =
            "<template>\n  {{ unknown }}\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "x".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span_start: 0,
                span_end: 0,
            }],
            vec![],
            vec![],
        );

        let offset = source.find("unknown").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let result =
            definition_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_none());
    }
}
