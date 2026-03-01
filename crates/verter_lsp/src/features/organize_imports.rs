// Organize imports: detect unused imports and provide code action to remove them.

use std::collections::HashSet;

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;

/// Build a "remove unused imports" code action if there are unused imports.
///
/// An import binding is considered "used" if:
/// - Its name appears in a template binding occurrence
/// - Its name matches a top-level binding (re-export, alias, or consumed in script)
/// - It's a type-only import (can't reliably detect usage without full type analysis)
/// - Its Vue API classification matches a call site in `vue_api_calls`
pub fn organize_imports_actions(
    source: &str,
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    let analysis = match analysis {
        Some(a) => a,
        None => return vec![],
    };

    // Collect names that are "used" in the file
    let mut used_names: HashSet<&str> = HashSet::new();

    // 1. Template binding occurrences (used in <template>)
    if let Some(template) = &analysis.template {
        for occ in &template.binding_occurrences {
            used_names.insert(&occ.name);
        }
        // Components referenced in template
        for comp in &template.components {
            used_names.insert(&comp.name);
        }
    }

    // 2. Top-level bindings (declared in script, not from imports themselves)
    //    If a binding's initializer references an import, that import is used.
    for binding in &analysis.bindings {
        // Bindings that use an imported value as initializer
        if let Some(verter_analysis::BindingInitializer::FunctionCall { ref callee, .. }) =
            binding.initializer
        {
            used_names.insert(callee);
        }
    }

    // 3. Vue API call sites
    for call in &analysis.vue_api_calls {
        // The API name itself is from an import (e.g., "onMounted" from "vue")
        let api_name = format!("{:?}", call.api);
        // We can't easily match enum variant to import name, so mark all Vue API
        // imports as used by checking if the import has a vue_api classification
        // that matches any call site. We'll handle this below.
        let _ = api_name;
    }

    // Classify imports: fully unused vs partially unused
    let mut fully_unused_indices: Vec<usize> = Vec::new();
    let mut partial_unused: Vec<(usize, Vec<String>)> = Vec::new(); // (import_idx, unused_names)
    let mut all_unused_names: Vec<String> = Vec::new();

    for (import_idx, import) in analysis.imports.iter().enumerate() {
        // Skip type-only imports — can't detect usage without full type checking
        if import.is_type_only || import.bindings.is_empty() {
            continue;
        }

        let mut used_bindings: Vec<&str> = Vec::new();
        let mut unused_bindings: Vec<String> = Vec::new();

        for binding in &import.bindings {
            if binding.is_type_only {
                used_bindings.push(&binding.name);
                continue;
            }

            let has_vue_api_call = binding.vue_api.is_some()
                && analysis
                    .vue_api_calls
                    .iter()
                    .any(|call| Some(call.api) == binding.vue_api);

            if used_names.contains(binding.name.as_str()) || has_vue_api_call {
                used_bindings.push(&binding.name);
            } else {
                unused_bindings.push(binding.name.clone());
            }
        }

        if unused_bindings.is_empty() {
            continue;
        }

        all_unused_names.extend(unused_bindings.iter().cloned());

        if used_bindings.is_empty() {
            // All bindings unused → remove entire import
            fully_unused_indices.push(import_idx);
        } else {
            // Some bindings unused → remove only unused specifiers
            partial_unused.push((import_idx, unused_bindings));
        }
    }

    if fully_unused_indices.is_empty() && partial_unused.is_empty() {
        return vec![];
    }

    let mut edits: Vec<TextEdit> = Vec::new();

    // Build edits for fully unused imports: remove entire import line
    for &idx in &fully_unused_indices {
        let import = &analysis.imports[idx];
        let Some(start) = line_index.offset_to_position(import.span.start) else {
            continue;
        };
        let mut end = match line_index.offset_to_position(import.span.end) {
            Some(e) => e,
            None => continue,
        };

        // Extend end to consume trailing semicolon + newline
        let end_offset = import.span.end as usize;
        if end_offset < source.len() {
            let rest = &source.as_bytes()[end_offset..];
            let mut skip = 0;
            if skip < rest.len() && rest[skip] == b';' {
                skip += 1;
            }
            while skip < rest.len() && (rest[skip] == b' ' || rest[skip] == b'\t') {
                skip += 1;
            }
            if skip < rest.len() && rest[skip] == b'\r' {
                skip += 1;
            }
            if skip < rest.len() && rest[skip] == b'\n' {
                skip += 1;
            }
            if skip > 0 {
                if let Some(new_end) = line_index.offset_to_position(import.span.end + skip as u32)
                {
                    end = new_end;
                }
            }
        }

        edits.push(TextEdit {
            range: Range { start, end },
            new_text: String::new(),
        });
    }

    // Build edits for partially unused imports: replace specifier list
    for (idx, unused_names) in &partial_unused {
        let import = &analysis.imports[*idx];
        let import_text = &source[import.span.start as usize..import.span.end as usize];

        // Find `{` and `}` in the import text
        let Some(brace_open_rel) = import_text.find('{') else {
            continue;
        };
        let Some(brace_close_rel) = import_text.rfind('}') else {
            continue;
        };

        // Build new specifier list with only used specifiers
        let old_specifiers = &import_text[brace_open_rel + 1..brace_close_rel];
        let new_specifiers: Vec<&str> = old_specifiers
            .split(',')
            .map(|s| s.trim())
            .filter(|s| {
                !s.is_empty()
                    && !unused_names.iter().any(|u| {
                        // Match the specifier's local name (handles `foo as bar` → check `bar`)
                        let local = s.rsplit(" as ").next().unwrap_or(s).trim();
                        // Also strip leading `type ` keyword
                        let local = local.strip_prefix("type ").unwrap_or(local).trim();
                        local == u
                    })
            })
            .collect();

        if new_specifiers.is_empty() {
            continue; // shouldn't happen for partial, but be safe
        }

        let new_text = format!("{{ {} }}", new_specifiers.join(", "));

        // Convert brace positions to absolute offsets
        let abs_brace_open = import.span.start + brace_open_rel as u32;
        let abs_brace_close = import.span.start + brace_close_rel as u32 + 1; // include `}`

        let Some(start) = line_index.offset_to_position(abs_brace_open) else {
            continue;
        };
        let Some(end) = line_index.offset_to_position(abs_brace_close) else {
            continue;
        };

        edits.push(TextEdit {
            range: Range { start, end },
            new_text,
        });
    }

    if edits.is_empty() {
        return vec![];
    }

    let label = if all_unused_names.len() == 1 {
        format!("Remove unused import '{}'", all_unused_names[0])
    } else {
        format!("Remove {} unused imports", all_unused_names.len())
    };

    vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: label,
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    // Sentinel: caller must replace with actual URI
                    uri: "file:///placeholder".parse().unwrap(),
                    version: None,
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            }])),
            change_annotations: None,
        }),
        is_preferred: Some(true),
        ..Default::default()
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::line_index::LineIndex;
    use verter_analysis::types::VueApiCallSite;
    use verter_analysis::*;

    fn make_analysis(
        imports: Vec<AnalyzedImport>,
        bindings: Vec<AnalyzedBinding>,
        template_occurrences: Vec<TemplateBindingOccurrence>,
        vue_api_calls: Vec<VueApiCallSite>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports,
            bindings,
            vue_api_calls,
            template: Some(TemplateAnalysisSnapshot {
                binding_occurrences: template_occurrences,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_no_unused_imports() {
        let source = "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template>{{ x }}</template>";
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".into(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span: verter_span::Span::new(25, 28),
                }],
                span: verter_span::Span::new(15, 40),
                resolved_canonical_id: None,
            }],
            vec![],
            vec![TemplateBindingOccurrence {
                name: "x".into(),
                span: verter_span::Span::new(80, 81),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            vec![VueApiCallSite {
                api: VueApiClassification::Ref,
                span: verter_span::Span::new(48, 54),
                arg_value: None,
                is_async_callback: false,
            }],
        );

        let actions = organize_imports_actions(source, Some(&analysis), &line_index);
        assert!(
            actions.is_empty(),
            "should have no actions when imports are all used"
        );
    }

    #[test]
    fn test_unused_import_detected() {
        let source = "<script setup>\nimport { ref, computed } from 'vue'\nconst x = ref(0)\n</script>\n<template>{{ x }}</template>";
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![
                    AnalyzedImportBinding {
                        name: "ref".into(),
                        is_type_only: false,
                        vue_api: Some(VueApiClassification::Ref),
                        span: verter_span::Span::new(25, 28),
                    },
                    AnalyzedImportBinding {
                        name: "computed".into(),
                        is_type_only: false,
                        vue_api: Some(VueApiClassification::Computed),
                        span: verter_span::Span::new(30, 38),
                    },
                ],
                span: verter_span::Span::new(15, 51),
                resolved_canonical_id: None,
            }],
            vec![],
            vec![TemplateBindingOccurrence {
                name: "x".into(),
                span: verter_span::Span::new(90, 91),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            vec![VueApiCallSite {
                api: VueApiClassification::Ref,
                span: verter_span::Span::new(58, 64),
                arg_value: None,
                is_async_callback: false,
            }],
        );

        let actions = organize_imports_actions(source, Some(&analysis), &line_index);
        assert!(
            !actions.is_empty(),
            "should detect unused 'computed' import"
        );
    }

    #[test]
    fn test_type_only_imports_skipped() {
        let source = "<script setup>\nimport type { Props } from './types'\n</script>";
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedImport {
                source: "./types".into(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Props".into(),
                    is_type_only: false,
                    vue_api: None,
                    span: verter_span::Span::new(29, 34),
                }],
                span: verter_span::Span::new(15, 52),
                resolved_canonical_id: None,
            }],
            vec![],
            vec![],
            vec![],
        );

        let actions = organize_imports_actions(source, Some(&analysis), &line_index);
        assert!(actions.is_empty(), "type-only imports should be skipped");
    }

    /// Helper to extract TextEdits from the first code action.
    fn extract_edits(actions: &[CodeActionOrCommand]) -> Vec<TextEdit> {
        match &actions[0] {
            CodeActionOrCommand::CodeAction(action) => {
                let edit = action.edit.as_ref().unwrap();
                let changes = edit.document_changes.as_ref().unwrap();
                match changes {
                    DocumentChanges::Edits(edits) => edits[0]
                        .edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(te) => te.clone(),
                            _ => panic!("unexpected edit type"),
                        })
                        .collect(),
                    _ => panic!("unexpected changes type"),
                }
            }
            _ => panic!("expected CodeAction"),
        }
    }

    #[test]
    fn test_partial_unused_removes_only_unused_specifier() {
        // `import { ref, computed } from 'vue'` where only `ref` is used
        // → should produce `import { ref } from 'vue'`, NOT remove the entire import
        let source =
            "<script setup>\nimport { ref, computed } from 'vue'\nconst x = ref(0)\n</script>\n<template>{{ x }}</template>";
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![
                    AnalyzedImportBinding {
                        name: "ref".into(),
                        is_type_only: false,
                        vue_api: Some(VueApiClassification::Ref),
                        span: verter_span::Span::new(25, 28),
                    },
                    AnalyzedImportBinding {
                        name: "computed".into(),
                        is_type_only: false,
                        vue_api: Some(VueApiClassification::Computed),
                        span: verter_span::Span::new(30, 38),
                    },
                ],
                span: verter_span::Span::new(15, 51),
                resolved_canonical_id: None,
            }],
            vec![],
            vec![TemplateBindingOccurrence {
                name: "x".into(),
                span: verter_span::Span::new(90, 91),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            vec![VueApiCallSite {
                api: VueApiClassification::Ref,
                span: verter_span::Span::new(58, 64),
                arg_value: None,
                is_async_callback: false,
            }],
        );

        let actions = organize_imports_actions(source, Some(&analysis), &line_index);
        assert!(
            !actions.is_empty(),
            "should produce action for unused specifier"
        );

        let edits = extract_edits(&actions);
        // Apply edits to check result
        let mut result = source.to_string();
        // Apply edits in reverse order of position to avoid offset shifts
        let mut sorted_edits = edits.clone();
        sorted_edits.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in &sorted_edits {
            let start = line_index.position_to_offset(&edit.range.start).unwrap() as usize;
            let end = line_index.position_to_offset(&edit.range.end).unwrap() as usize;
            result.replace_range(start..end, &edit.new_text);
        }

        // The import should still have `ref` but not `computed`
        assert!(
            result.contains("import"),
            "import statement should still exist: {result}"
        );
        assert!(
            result.contains("ref"),
            "used specifier 'ref' should remain: {result}"
        );
        assert!(
            !result.contains("computed"),
            "unused specifier 'computed' should be removed: {result}"
        );
    }

    #[test]
    fn test_fully_unused_import_removed_entirely() {
        // `import { computed } from 'vue'` where nothing is used
        // → should remove the entire import line
        let source = "<script setup>\nimport { computed } from 'vue'\nconst x = 1\n</script>\n<template>{{ x }}</template>";
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "computed".into(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Computed),
                    span: verter_span::Span::new(25, 33),
                }],
                span: verter_span::Span::new(15, 46),
                resolved_canonical_id: None,
            }],
            vec![],
            vec![TemplateBindingOccurrence {
                name: "x".into(),
                span: verter_span::Span::new(80, 81),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            vec![],
        );

        let actions = organize_imports_actions(source, Some(&analysis), &line_index);
        assert!(!actions.is_empty(), "should detect unused import");

        let edits = extract_edits(&actions);
        // For fully unused import, the edit should remove the entire import text range
        assert_eq!(edits.len(), 1, "should have one edit for full removal");
        assert!(
            edits[0].new_text.is_empty(),
            "edit should be a deletion (empty new_text)"
        );
    }
}
