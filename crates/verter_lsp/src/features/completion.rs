// Phase 2: Completion — template bindings, component names, props from verter_host analysis.
// Phase 3: Enhanced with typed member access, generic inference from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Provide completions at a given position.
///
/// Strategy:
/// 1. Find which SFC block the position is in
/// 2. For script blocks: offer bindings, imports, Vue APIs
/// 3. For template blocks: offer all available bindings from script setup
///
/// ## Analysis data needed (not yet exposed via FileAnalysisSnapshot):
/// - `TemplateAnalysisSnapshot.components` — for `<ComponentName>` completions in template
/// - Template-specific context (current element, attribute context, directive context)
///   for targeted completions (requires TemplateElement/TemplateDirective exposure)
/// - `AnalyzedPropDefinition` from child components — for attribute completions on `<ChildComp :prop>`
pub fn completions_at_position(
    position: &Position,
    _source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Vec<CompletionItem>> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;

    // Determine which block the cursor is in
    let block = blocks.iter().find(|b| {
        let (content_start, content_end) = b.content_range();
        offset >= content_start as usize && offset <= content_end as usize
    })?;

    match block.tag_name.as_str() {
        "script" => Some(script_completions(analysis)),
        "template" => Some(template_completions(analysis)),
        _ => None,
    }
}

/// Completions available in `<script setup>` context.
fn script_completions(analysis: &FileAnalysisSnapshot) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Offer existing bindings
    for binding in &analysis.bindings {
        items.push(CompletionItem {
            label: binding.name.clone(),
            kind: Some(binding_completion_kind(&binding.kind)),
            detail: Some(binding_detail(binding)),
            ..Default::default()
        });
    }

    // Offer imports
    for import in &analysis.imports {
        for binding in &import.bindings {
            items.push(CompletionItem {
                label: binding.name.clone(),
                kind: Some(if binding.is_type_only || import.is_type_only {
                    CompletionItemKind::TYPE_PARAMETER
                } else {
                    CompletionItemKind::MODULE
                }),
                detail: Some(format!("from '{}'", import.source)),
                ..Default::default()
            });
        }
    }

    // Filter out ___VERTER___ internal symbols
    items.retain(|item| !item.label.starts_with("___VERTER___"));

    items
}

/// Completions available in `<template>` context.
///
/// Offers all script-setup bindings that are available in the template scope.
fn template_completions(analysis: &FileAnalysisSnapshot) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // All bindings from script setup are available in template
    for binding in &analysis.bindings {
        let mut item = CompletionItem {
            label: binding.name.clone(),
            kind: Some(binding_completion_kind(&binding.kind)),
            detail: Some(binding_detail(binding)),
            ..Default::default()
        };

        // Add reactivity indicator
        let reactivity_tag = match binding.reactivity_kind {
            verter_analysis::ReactivityKind::Ref => Some("ref"),
            verter_analysis::ReactivityKind::Computed => Some("computed"),
            verter_analysis::ReactivityKind::Reactive => Some("reactive"),
            verter_analysis::ReactivityKind::MaybeRef => Some("maybe-ref"),
            verter_analysis::ReactivityKind::Mutable => Some("mutable"),
            verter_analysis::ReactivityKind::None => {
                if binding.is_reactive {
                    Some("reactive")
                } else {
                    None
                }
            }
        };
        if let Some(tag) = reactivity_tag {
            item.detail = Some(format!("{} ({tag})", item.detail.unwrap_or_default()));
        }

        items.push(item);
    }

    // Non-type imports are also available in template
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        for binding in &import.bindings {
            if binding.is_type_only {
                continue;
            }
            items.push(CompletionItem {
                label: binding.name.clone(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("from '{}'", import.source)),
                ..Default::default()
            });
        }
    }

    // Macro result bindings are available too
    for mac in &analysis.macros {
        if let Some(ref name) = mac.binding_name {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("{}()", macro_kind_label(&mac.kind))),
                ..Default::default()
            });
        }
    }

    // Filter out internal symbols
    items.retain(|item| !item.label.starts_with("___VERTER___"));

    // Deduplicate by label
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);

    items
}

fn binding_completion_kind(kind: &verter_analysis::AnalyzedBindingKind) -> CompletionItemKind {
    match kind {
        verter_analysis::AnalyzedBindingKind::Const => CompletionItemKind::CONSTANT,
        verter_analysis::AnalyzedBindingKind::Let | verter_analysis::AnalyzedBindingKind::Var => {
            CompletionItemKind::VARIABLE
        }
        verter_analysis::AnalyzedBindingKind::Function
        | verter_analysis::AnalyzedBindingKind::AsyncFunction => CompletionItemKind::FUNCTION,
        verter_analysis::AnalyzedBindingKind::Class => CompletionItemKind::CLASS,
    }
}

fn binding_detail(binding: &verter_analysis::AnalyzedBinding) -> String {
    let kind = match binding.kind {
        verter_analysis::AnalyzedBindingKind::Const => "const",
        verter_analysis::AnalyzedBindingKind::Let => "let",
        verter_analysis::AnalyzedBindingKind::Var => "var",
        verter_analysis::AnalyzedBindingKind::Function => "function",
        verter_analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_analysis::AnalyzedBindingKind::Class => "class",
    };
    kind.to_string()
}

fn macro_kind_label(kind: &verter_analysis::AnalyzedMacroKind) -> &'static str {
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
    fn test_template_completions_include_bindings() {
        let source = "<template>\n  {{ | }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
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

        // Position inside template
        let position = Position {
            line: 1,
            character: 5,
        };
        let items =
            completions_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        let items = items.unwrap();
        assert!(items.iter().any(|i| i.label == "count"));
    }

    #[test]
    fn test_script_completions_include_imports() {
        let source = "<script setup>\nimport { ref } from 'vue'\n\n</script>\n";
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

        // Position inside script (line 2)
        let position = Position {
            line: 2,
            character: 0,
        };
        let items =
            completions_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        let items = items.unwrap();
        assert!(items.iter().any(|i| i.label == "ref"));
    }

    #[test]
    fn test_filters_internal_symbols() {
        let source = "<script setup>\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "___VERTER___internal".to_string(),
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

        let position = Position {
            line: 1,
            character: 0,
        };
        let items =
            completions_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        assert!(items.unwrap().is_empty());
    }

    #[test]
    fn test_no_completions_in_style() {
        let source = "<style>\n.foo {}\n</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(vec![], vec![], vec![]);

        let position = Position {
            line: 1,
            character: 0,
        };
        let items =
            completions_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_none());
    }

    #[test]
    fn test_template_excludes_type_only_imports() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nimport type { Props } from './types'\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![],
            vec![AnalyzedImport {
                source: "./types".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Props".to_string(),
                    is_type_only: true,
                    vue_api: None,
                    span_start: 0,
                    span_end: 0,
                }],
                span_start: 0,
                span_end: 0,
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let position = Position {
            line: 1,
            character: 3,
        };
        let items =
            completions_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(items.is_some());
        // Type-only imports should not appear in template completions
        assert!(!items.unwrap().iter().any(|i| i.label == "Props"));
    }
}
