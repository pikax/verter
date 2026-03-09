// Call hierarchy: Vue-specific component and composable hierarchy.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Prepare a call hierarchy item at the given position.
///
/// Returns a call hierarchy item if the cursor is on:
/// - A component tag name in template (shows parent/child component relationships)
/// - A Vue API call like `onMounted()`, `watch()` in script
/// - A function/composable declaration
pub fn prepare_call_hierarchy(
    position: &Position,
    _source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    uri: &Uri,
) -> Option<Vec<CallHierarchyItem>> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)?;

    // Check if cursor is on a binding declaration
    for binding in &analysis.bindings {
        if offset >= binding.span.start && offset <= binding.span.end {
            let start = line_index.offset_to_position(binding.span.start)?;
            let end = line_index.offset_to_position(binding.span.end)?;
            let kind = match &binding.kind {
                verter_analysis::AnalyzedBindingKind::Function
                | verter_analysis::AnalyzedBindingKind::AsyncFunction => SymbolKind::FUNCTION,
                verter_analysis::AnalyzedBindingKind::Class => SymbolKind::CLASS,
                _ => SymbolKind::VARIABLE,
            };

            return Some(vec![CallHierarchyItem {
                name: binding.name.clone(),
                kind,
                tags: None,
                detail: Some(format!("{:?}", binding.kind)),
                uri: uri.clone(),
                range: Range { start, end },
                selection_range: Range { start, end },
                data: None,
            }]);
        }
    }

    // Check if cursor is on a Vue API call
    for call in &analysis.vue_api_calls {
        if offset >= call.span.start && offset <= call.span.end {
            let start = line_index.offset_to_position(call.span.start)?;
            let end = line_index.offset_to_position(call.span.end)?;

            return Some(vec![CallHierarchyItem {
                name: format!("{:?}", call.api),
                kind: SymbolKind::FUNCTION,
                tags: None,
                detail: Some("Vue API".into()),
                uri: uri.clone(),
                range: Range { start, end },
                selection_range: Range { start, end },
                data: None,
            }]);
        }
    }

    // Check template components
    if let Some(template) = &analysis.template {
        let _template_block = blocks.iter().find(|b| {
            b.tag_name == "template" && {
                let (cs, ce) = b.content_range();
                offset >= cs && offset <= ce
            }
        })?;

        for comp in &template.components {
            if offset >= comp.span.start && offset <= comp.span.end {
                let start = line_index.offset_to_position(comp.span.start)?;
                let end = line_index.offset_to_position(comp.span.end)?;

                return Some(vec![CallHierarchyItem {
                    name: comp.name.clone(),
                    kind: SymbolKind::CLASS,
                    tags: None,
                    detail: comp.import_source.clone(),
                    uri: uri.clone(),
                    range: Range { start, end },
                    selection_range: Range { start, end },
                    data: None,
                }]);
            }
        }
    }

    None
}

/// Find incoming calls to a call hierarchy item.
///
/// For components: returns parent components that use this component.
/// For functions: returns call sites within the same file.
pub fn incoming_calls(
    _item: &CallHierarchyItem,
    source: &str,
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    uri: &Uri,
) -> Vec<CallHierarchyIncomingCall> {
    let analysis = match analysis {
        Some(a) => a,
        None => return vec![],
    };

    let mut calls = Vec::new();

    // For bindings: find template occurrences that reference this binding
    if let Some(template) = &analysis.template {
        for occ in &template.binding_occurrences {
            if occ.name == _item.name {
                if let (Some(start), Some(end)) = (
                    line_index.offset_to_position(occ.span.start),
                    line_index.offset_to_position(occ.span.end),
                ) {
                    calls.push(CallHierarchyIncomingCall {
                        from: CallHierarchyItem {
                            name: format!("<template> usage: {}", occ.name),
                            kind: SymbolKind::FIELD,
                            tags: None,
                            detail: Some(format!("{:?}", occ.usage_kind)),
                            uri: uri.clone(),
                            range: Range { start, end },
                            selection_range: Range { start, end },
                            data: None,
                        },
                        from_ranges: vec![Range { start, end }],
                    });
                }
            }
        }
    }

    // For Vue API calls used in the file
    let _ = source;

    calls
}

/// Find outgoing calls from a call hierarchy item.
///
/// For components: returns child components used in this component's template.
/// For functions: returns Vue API calls and other function calls.
pub fn outgoing_calls(
    _item: &CallHierarchyItem,
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    uri: &Uri,
) -> Vec<CallHierarchyOutgoingCall> {
    let analysis = match analysis {
        Some(a) => a,
        None => return vec![],
    };

    let mut calls = Vec::new();

    // Show Vue API calls as outgoing
    for call in &analysis.vue_api_calls {
        if let (Some(start), Some(end)) = (
            line_index.offset_to_position(call.span.start),
            line_index.offset_to_position(call.span.end),
        ) {
            calls.push(CallHierarchyOutgoingCall {
                to: CallHierarchyItem {
                    name: format!("{:?}", call.api),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    detail: Some("Vue API".into()),
                    uri: uri.clone(),
                    range: Range { start, end },
                    selection_range: Range { start, end },
                    data: None,
                },
                from_ranges: vec![Range { start, end }],
            });
        }
    }

    // Show child components as outgoing calls
    if let Some(template) = &analysis.template {
        for comp in &template.components {
            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(comp.span.start),
                line_index.offset_to_position(comp.span.end),
            ) {
                calls.push(CallHierarchyOutgoingCall {
                    to: CallHierarchyItem {
                        name: comp.name.clone(),
                        kind: SymbolKind::CLASS,
                        tags: None,
                        detail: comp.import_source.clone(),
                        uri: uri.clone(),
                        range: Range { start, end },
                        selection_range: Range { start, end },
                        data: None,
                    },
                    from_ranges: vec![Range { start, end }],
                });
            }
        }
    }

    calls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::*;

    #[test]
    fn test_prepare_on_binding() {
        let source = "<script setup>\nconst foo = ref(0)\n</script>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let uri: Uri = "file:///test.vue".parse().unwrap();

        let analysis = FileAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "foo".into(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(21, 24),
                used_in_script: false,
                used_in_style: false,
            }],
            ..Default::default()
        };

        let pos = line_index.offset_to_position(22).unwrap();
        let result =
            prepare_call_hierarchy(&pos, source, &blocks, Some(&analysis), &line_index, &uri);
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0].name, "foo");
    }

    #[test]
    fn test_outgoing_calls_shows_components() {
        let source =
            "<script setup>\nimport Foo from './Foo.vue'\n</script>\n<template><Foo /></template>";
        let line_index = LineIndex::new_utf16(source);
        let uri: Uri = "file:///test.vue".parse().unwrap();

        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                components: vec![TemplateComponentUsage {
                    name: "Foo".into(),
                    import_source: Some("./Foo.vue".into()),
                    is_dynamic: false,
                    props: vec![],
                    has_spread: false,
                    slots_used: vec![],
                    static_classes: vec![],
                    has_dynamic_class: false,
                    dynamic_classes: vec![],
                    v_models: vec![],
                    span: verter_span::Span::new(60, 67),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let item = CallHierarchyItem {
            name: "App".into(),
            kind: SymbolKind::CLASS,
            tags: None,
            detail: None,
            uri: uri.clone(),
            range: Range::default(),
            selection_range: Range::default(),
            data: None,
        };

        let calls = outgoing_calls(&item, Some(&analysis), &line_index, &uri);
        assert!(!calls.is_empty());
        assert_eq!(
            calls.iter().find(|c| c.to.name == "Foo").unwrap().to.name,
            "Foo"
        );
    }
}
