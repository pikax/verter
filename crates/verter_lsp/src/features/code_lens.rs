use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Generate CodeLens items for a Vue SFC file.
///
/// Produces summary annotations above each SFC block:
/// - `<script setup>` — binding count, import count, lifecycle hooks
/// - `<template>` — component count, binding occurrences
/// - `<style>` — selector count, class count
pub fn code_lenses(
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<CodeLens> {
    let analysis = match analysis {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut lenses = Vec::new();

    for block in blocks {
        let Some(position) = line_index.offset_to_position(block.open_tag_start) else {
            continue;
        };
        let range = Range::new(position, position);

        let title = match block.tag_name.as_str() {
            "script" => script_summary(analysis),
            "template" => template_summary(analysis),
            "style" => style_summary(analysis, block),
            _ => continue,
        };

        if title.is_empty() {
            continue;
        }

        lenses.push(CodeLens {
            range,
            command: Some(Command {
                title,
                command: String::new(),
                arguments: None,
            }),
            data: None,
        });
    }

    lenses
}

fn script_summary(analysis: &FileAnalysisSnapshot) -> String {
    let mut parts = Vec::new();

    let n_bindings = analysis.bindings.len();
    if n_bindings > 0 {
        parts.push(format!(
            "{n_bindings} binding{}",
            if n_bindings == 1 { "" } else { "s" }
        ));
    }

    let n_imports = analysis.imports.len();
    if n_imports > 0 {
        parts.push(format!(
            "{n_imports} import{}",
            if n_imports == 1 { "" } else { "s" }
        ));
    }

    let n_hooks = analysis
        .vue_api_calls
        .iter()
        .filter(|c| c.api.is_lifecycle())
        .count();
    if n_hooks > 0 {
        parts.push(format!(
            "{n_hooks} lifecycle hook{}",
            if n_hooks == 1 { "" } else { "s" }
        ));
    }

    let n_macros = analysis.macros.len();
    if n_macros > 0 {
        parts.push(format!(
            "{n_macros} macro{}",
            if n_macros == 1 { "" } else { "s" }
        ));
    }

    parts.join(" · ")
}

fn template_summary(analysis: &FileAnalysisSnapshot) -> String {
    let template = match analysis.template.as_ref() {
        Some(t) => t,
        None => return String::new(),
    };

    let mut parts = Vec::new();

    let n_components = template.components.len();
    if n_components > 0 {
        parts.push(format!(
            "{n_components} component{}",
            if n_components == 1 { "" } else { "s" }
        ));
    }

    let n_bindings = template.binding_occurrences.len();
    if n_bindings > 0 {
        parts.push(format!(
            "{n_bindings} binding ref{}",
            if n_bindings == 1 { "" } else { "s" }
        ));
    }

    let n_slots = template.defined_slots.len();
    if n_slots > 0 {
        parts.push(format!(
            "{n_slots} slot{}",
            if n_slots == 1 { "" } else { "s" }
        ));
    }

    parts.join(" · ")
}

fn style_summary(analysis: &FileAnalysisSnapshot, block: &SfcBlock) -> String {
    let is_scoped = block.is_scoped();

    // Find corresponding style analysis
    let style = analysis.styles.iter().find(|s| s.scoped == is_scoped);
    let style = match style {
        Some(s) => s,
        None => return String::new(),
    };

    let mut parts = Vec::new();

    if let Some(ref css) = style.css {
        let n_selectors = css.selectors.len();
        if n_selectors > 0 {
            parts.push(format!(
                "{n_selectors} selector{}",
                if n_selectors == 1 { "" } else { "s" }
            ));
        }

        let n_classes = css.classes.len();
        if n_classes > 0 {
            parts.push(format!(
                "{n_classes} class{}",
                if n_classes == 1 { "" } else { "es" }
            ));
        }
    }

    let n_vbinds = style.v_binds.len();
    if n_vbinds > 0 {
        parts.push(format!(
            "{n_vbinds} v-bind{}",
            if n_vbinds == 1 { "" } else { "s" }
        ));
    }

    if is_scoped {
        parts.push("scoped".to_string());
    }

    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::types::VueApiCallSite;
    use verter_analysis::*;

    #[test]
    fn test_script_code_lens() {
        let source = "<script setup>\nconst x = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "x".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(0, 0),
                used_in_script: false,
                used_in_style: false,
            }],
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            }],
            vue_api_calls: (vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: verter_span::Span::new(0, 5),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }])
            .into(),
            ..Default::default()
        };

        let lenses = code_lenses(&blocks, Some(&analysis), &line_index);
        assert_eq!(lenses.len(), 1);
        let title = &lenses[0].command.as_ref().unwrap().title;
        assert!(title.contains("1 binding"));
        assert!(title.contains("1 import"));
        assert!(title.contains("1 lifecycle hook"));
    }

    #[test]
    fn test_empty_analysis_no_lenses() {
        let source = "<script setup>\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot::default();
        let lenses = code_lenses(&blocks, Some(&analysis), &line_index);
        assert!(lenses.is_empty());
    }

    #[test]
    fn test_no_analysis_no_lenses() {
        let source = "<script setup>\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let lenses = code_lenses(&blocks, None, &line_index);
        assert!(lenses.is_empty());
    }
}
