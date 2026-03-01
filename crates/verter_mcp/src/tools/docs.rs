//! Component documentation generation.

use verter_analysis::template::TemplateAnalysisSnapshot;
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};
use verter_analysis::StyleBlockAnalysis;

/// Generate Markdown documentation for a Vue component.
pub fn generate_docs(
    path: &str,
    script: Option<&ScriptAnalysisSnapshot>,
    template: Option<&TemplateAnalysisSnapshot>,
    styles: &[StyleBlockAnalysis],
) -> String {
    let component_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Component");

    let mut doc = String::with_capacity(2048);
    doc.push_str(&format!("# {}\n\n", component_name));

    // Props table (from template analysis prop_definitions)
    if let Some(tpl) = template {
        if !tpl.prop_definitions.is_empty() {
            doc.push_str("## Props\n\n");
            doc.push_str("| Name | Type | Required | Default |\n");
            doc.push_str("|------|------|----------|---------|\n");

            for prop in &tpl.prop_definitions {
                doc.push_str(&format!(
                    "| `{}` | `{}` | {} | {} |\n",
                    prop.name,
                    prop.type_annotation.as_deref().unwrap_or("any"),
                    if prop.is_required { "Yes" } else { "No" },
                    if prop.has_default { "Yes" } else { "-" },
                ));
            }
            doc.push('\n');
        }

        // Events (from template analysis emit_definitions)
        if !tpl.emit_definitions.is_empty() {
            doc.push_str("## Events\n\n");
            doc.push_str("| Event | Declared | Validated |\n");
            doc.push_str("|-------|----------|-----------|\n");

            for emit in &tpl.emit_definitions {
                doc.push_str(&format!(
                    "| `{}` | {} | {} |\n",
                    emit.event_name,
                    if emit.is_declared { "Yes" } else { "No" },
                    if emit.has_validator { "Yes" } else { "No" },
                ));
            }
            doc.push('\n');
        }

        // Slots
        if !tpl.defined_slots.is_empty() {
            doc.push_str("## Slots\n\n");
            for slot in &tpl.defined_slots {
                doc.push_str(&format!("- `{}`", slot.name));
                if slot.has_bindings && !slot.binding_names.is_empty() {
                    let bindings: Vec<String> = slot
                        .binding_names
                        .iter()
                        .map(|b| format!("`{}`", b))
                        .collect();
                    doc.push_str(&format!(" (scoped: {})", bindings.join(", ")));
                }
                doc.push('\n');
            }
            doc.push('\n');
        }

        // Child Components
        if !tpl.components.is_empty() {
            doc.push_str("## Child Components\n\n");
            for comp in &tpl.components {
                doc.push_str(&format!("- `<{}>`", comp.name));
                if let Some(src) = &comp.import_source {
                    doc.push_str(&format!(" from `{}`", src));
                }
                if !comp.props.is_empty() {
                    doc.push_str(&format!(" ({} props)", comp.props.len()));
                }
                doc.push('\n');
            }
            doc.push('\n');
        }
    }

    // Models (from script macros)
    if let Some(s) = script {
        let models: Vec<_> = s
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineModel)
            .collect();

        if !models.is_empty() {
            doc.push_str("## Models\n\n");
            for m in &models {
                let name = m
                    .model_name
                    .as_deref()
                    .or(m.binding_name.as_deref())
                    .unwrap_or("modelValue");
                doc.push_str(&format!("- `{}`", name));
                if !m.type_references.is_empty() {
                    doc.push_str(&format!(" — `{}`", m.type_references.join(", ")));
                }
                doc.push('\n');
            }
            doc.push('\n');
        }
    }

    // Styles
    if !styles.is_empty() {
        doc.push_str("## Styles\n\n");
        for (i, style) in styles.iter().enumerate() {
            let scope = if style.scoped { "scoped" } else { "global" };
            doc.push_str(&format!("- Block {} ({})", i, scope));
            if let Some(css) = &style.css {
                doc.push_str(&format!(
                    " — {} selectors, {} classes",
                    css.selectors.len(),
                    css.classes.len()
                ));
            }
            doc.push('\n');
        }
    }

    doc
}
