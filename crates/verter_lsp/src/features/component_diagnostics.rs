// Component usage diagnostics: unknown props, unknown v-models.
//
// Checks parent component template usages against child component definitions.
// When a parent passes a prop that the child doesn't define, a diagnostic is emitted.

use std::collections::HashSet;

use tower_lsp_server::lsp_types::*;
use verter_analysis::template::{TemplateComponentUsage, TemplatePropUsage};
use verter_analysis::types::{AnalysisFlags, AnalyzedMacroKind, VueApiClassification};
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;

/// Attributes that are always valid on any component (Vue fallthrough attrs).
const BUILTIN_ATTRS: &[&str] = &["class", "style"];

/// Information about an unknown prop found on a component usage.
pub struct UnknownPropInfo {
    pub component_name: String,
    pub prop_name: String,
    pub import_source: String,
    pub span: verter_span::Span,
}

/// Convert kebab-case to camelCase for prop name comparison.
fn kebab_to_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for uc in ch.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Check if a child component suppresses unknown prop diagnostics.
///
/// Returns true if:
/// - The child calls `useAttrs()` (accessing fallthrough attrs)
/// - The child has `defineOptions({ inheritAttrs: false })`
fn child_suppresses_prop_checks(child: &FileAnalysisSnapshot) -> bool {
    // Check useAttrs()
    let has_use_attrs = child
        .vue_api_calls
        .iter()
        .any(|c| c.api == VueApiClassification::UseAttrs);
    if has_use_attrs {
        return true;
    }

    // Check inheritAttrs: false
    let flags = AnalysisFlags::from_bits_truncate(child.script_flags);
    if flags.contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE) {
        return true;
    }

    false
}

/// Get the set of defined prop names from a child's analysis (camelCase).
fn child_prop_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    child
        .template
        .as_ref()
        .map(|t| t.prop_definitions.iter().map(|p| p.name.clone()).collect())
        .unwrap_or_default()
}

/// Find unknown props across all component usages.
pub fn find_unknown_props(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<UnknownPropInfo> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for comp in &template.components {
        if let Some(infos) = check_component_props(comp, resolve_child) {
            results.extend(infos);
        }
    }

    results
}

/// Check a single component usage for unknown props.
fn check_component_props(
    comp: &TemplateComponentUsage,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Option<Vec<UnknownPropInfo>> {
    // Skip dynamic components (<component :is="...">)
    if comp.is_dynamic {
        return None;
    }

    // Skip if component has v-bind spread (can't validate individual props)
    if comp.has_spread {
        return None;
    }

    // Need import source to resolve child
    let import_source = comp.import_source.as_deref()?;

    // Resolve child component analysis
    let child = resolve_child(import_source)?;

    // Check if child suppresses prop checks
    if child_suppresses_prop_checks(&child) {
        return None;
    }

    let defined_props = child_prop_names(&child);
    let mut unknowns = Vec::new();

    for prop in &comp.props {
        if is_unknown_prop(prop, &defined_props) {
            unknowns.push(UnknownPropInfo {
                component_name: comp.name.clone(),
                prop_name: prop.name.clone(),
                import_source: import_source.to_string(),
                span: prop.span,
            });
        }
    }

    Some(unknowns)
}

/// Check if a single prop is unknown (not defined by the child).
fn is_unknown_prop(prop: &TemplatePropUsage, defined_props: &HashSet<String>) -> bool {
    // Skip spread entries
    if prop.from_spread {
        return false;
    }

    // Skip builtin attributes
    if BUILTIN_ATTRS.contains(&prop.name.as_str()) {
        return false;
    }

    // Normalize to camelCase for comparison
    let camel_name = kebab_to_camel(&prop.name);

    // Check against defined props
    !defined_props.contains(&camel_name)
}

/// Information about an unknown v-model found on a component usage.
pub struct UnknownModelInfo {
    pub component_name: String,
    pub model_name: String,
    pub import_source: String,
    pub span: verter_span::Span,
}

/// Get the set of defined model names from a child's macros.
///
/// Each `defineModel('name')` contributes a name. `defineModel()` without
/// arguments contributes `"modelValue"`.
fn child_model_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    child
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineModel)
        .map(|m| {
            m.model_name
                .clone()
                .unwrap_or_else(|| "modelValue".to_string())
        })
        .collect()
}

/// Find unknown v-models across all component usages.
pub fn find_unknown_models(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<UnknownModelInfo> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for comp in &template.components {
        if comp.is_dynamic || comp.v_models.is_empty() {
            continue;
        }

        let import_source = match &comp.import_source {
            Some(s) => s.as_str(),
            None => continue,
        };

        let child = match resolve_child(import_source) {
            Some(c) => c,
            None => continue,
        };

        let defined_models = child_model_names(&child);

        for vmodel in &comp.v_models {
            if !defined_models.contains(&vmodel.binding_name) {
                results.push(UnknownModelInfo {
                    component_name: comp.name.clone(),
                    model_name: vmodel.binding_name.clone(),
                    import_source: import_source.to_string(),
                    span: vmodel.span,
                });
            }
        }
    }

    results
}

/// Generate LSP diagnostics for unknown props and v-models on component usages.
pub fn component_usage_diagnostics(
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<Diagnostic> {
    let unknowns = find_unknown_props(analysis, resolve_child);
    let mut diagnostics = Vec::new();

    for info in &unknowns {
        let start = line_index
            .offset_to_position(info.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(info.span.end)
            .unwrap_or(start);

        diagnostics.push(Diagnostic {
            range: Range { start, end },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("verter/unknown-prop".into())),
            source: Some("verter".into()),
            message: format!(
                "Unknown prop '{}' on component <{}>",
                info.prop_name, info.component_name
            ),
            ..Default::default()
        });
    }

    // V-model diagnostics
    let unknown_models = find_unknown_models(analysis, resolve_child);
    for info in &unknown_models {
        let start = line_index
            .offset_to_position(info.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(info.span.end)
            .unwrap_or(start);

        diagnostics.push(Diagnostic {
            range: Range { start, end },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("verter/unknown-model".into())),
            source: Some("verter".into()),
            message: format!(
                "Unknown v-model '{}' on component <{}>",
                info.model_name, info.component_name
            ),
            ..Default::default()
        });
    }

    diagnostics
}

#[cfg(test)]
#[path = "component_diagnostics_tests.rs"]
mod component_diagnostics_tests;
