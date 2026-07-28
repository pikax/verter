// Component usage diagnostics: unknown props, unknown v-models.
//
// Checks parent component template usages against child component definitions.
// When a parent passes a prop that the child doesn't define, a diagnostic is emitted.

use std::collections::HashSet;

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::template::{TemplateComponentUsage, TemplatePropUsage};
use verter_semantic::analysis::types::{AnalyzedMacroKind, VueApiClassification};
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;

/// `class` and `style` reach EVERY component through
/// `AllowedComponentProps` — they are merged onto the root rather than
/// consumed by fallthrough, so they stay valid on a fragment child and under
/// `inheritAttrs: false` alike. They are deliberately NOT part of the
/// inherited surface below: routing them through fallthrough would make their
/// acceptance depend on inheritance, which is not what Vue does.
const BUILTIN_ATTRS: &[&str] = &["class", "style"];

/// A child component as this lint needs it: its analysis snapshot plus the
/// attribute-fallthrough surface resolved for it.
///
/// The fallthrough half is produced ONLY by `verter_session`'s single
/// inheritance resolver (the Fallthrough / Root Inheritance CRITICAL rule).
/// This module owns no inheritance semantics of its own — the string allowlist
/// it used to decide with (`class`/`style`/`data-*`/`aria-*` by prefix, plus a
/// root-element count) was a second implementation of that rule, and it was
/// the one producing the false positives in
/// <https://github.com/pikax/verter/issues/97>.
pub struct ResolvedChildComponent {
    pub analysis: FileAnalysisSnapshot,
    /// Attribute names a parent may pass that the child does not declare,
    /// as resolved. EMPTY means nothing is inherited — `inheritAttrs: false`,
    /// a fragment, an unresolved root, or no resolver answer at all. Empty is
    /// the fail-closed value: an unresolved surface must never widen.
    pub inherited_attrs: HashSet<String>,
}

impl ResolvedChildComponent {
    /// Whether ANY attribute falls through to this child's root. Distinguishes
    /// "inherits nothing" (fragment / `inheritAttrs: false` / unresolved) from
    /// "inherits a surface", which is what decides the hyphenated-attribute
    /// case below.
    fn inherits_anything(&self) -> bool {
        !self.inherited_attrs.is_empty()
    }
}

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
/// The ONE suppressor is `useAttrs()`: a child that reads `$attrs`
/// programmatically may give meaning to any attribute the parent passes, so
/// this lint cannot prove one wrong. That is a fail-OPEN choice about a
/// component's own code, not a statement about inheritance.
///
/// `defineOptions({ inheritAttrs: false })` is deliberately NOT a suppressor.
/// It used to be, which was the exact INVERSE of the Fallthrough / Root
/// Inheritance rule: `inheritAttrs: false` means NO inherited surface, so an
/// undeclared attribute reaches nothing and is MORE wrong there, not less.
/// The resolved surface below is empty for such a child, which produces the
/// correct answer without a special case here.
fn child_suppresses_prop_checks(child: &FileAnalysisSnapshot) -> bool {
    child
        .vue_api_calls
        .iter()
        .any(|c| c.api == VueApiClassification::UseAttrs)
}

/// Get the set of defined prop names from a child's analysis (camelCase).
///
/// Sources (in priority order):
/// 1. `template.prop_definitions` — fully resolved prop definitions
/// 2. `macros[DefineProps].prop_fields` — extracted from defineProps type literal / runtime object
fn child_prop_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    // Try template.prop_definitions first
    if let Some(t) = &child.template {
        if !t.prop_definitions.is_empty() {
            return t.prop_definitions.iter().map(|p| p.name.clone()).collect();
        }
    }

    // Fall back to macro prop_fields
    child
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter().map(|f| f.name.clone()))
        .collect()
}

/// Find unknown props across all component usages.
pub fn find_unknown_props(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<ResolvedChildComponent>,
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
    resolve_child: &dyn Fn(&str) -> Option<ResolvedChildComponent>,
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
    if child_suppresses_prop_checks(&child.analysis) {
        return None;
    }

    let defined_props = child_prop_names(&child.analysis);

    // If no prop definitions could be resolved (e.g., external type refs like
    // `defineProps<Props>()` where Props is imported), skip checking entirely
    // to avoid false positives on every prop.
    if defined_props.is_empty() {
        return None;
    }

    let mut unknowns = Vec::new();

    for prop in &comp.props {
        if is_unknown_prop(prop, &defined_props, &child) {
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

/// Whether an attribute name is a valid JS/TS identifier.
///
/// The distinction is load-bearing rather than cosmetic: TypeScript skips
/// excess-property checking on JSX attributes whose names are not valid
/// identifiers, so the generated-TSX producer cannot check `data-foo` on a
/// component either. Reporting it here would make the Verter-owned lint and
/// the type-checked carrier disagree about the same markup.
fn is_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Check if a single prop is unknown (neither declared by the child nor
/// reaching its root element through attribute fallthrough).
fn is_unknown_prop(
    prop: &TemplatePropUsage,
    defined_props: &HashSet<String>,
    child: &ResolvedChildComponent,
) -> bool {
    // Skip spread entries
    if prop.from_spread {
        return false;
    }

    // class/style reach every component through AllowedComponentProps.
    if BUILTIN_ATTRS.contains(&prop.name.as_str()) {
        return false;
    }

    // Normalize to camelCase for comparison (`my-prop` binds `myProp`).
    let camel_name = kebab_to_camel(&prop.name);

    // Declared by the child.
    if defined_props.contains(&camel_name) {
        return false;
    }

    // On the resolved fallthrough surface — the attribute genuinely reaches
    // the child's root element. Both spellings are checked because the
    // resolver names members as the element types them (`aria-label` stays
    // hyphenated, `tabindex` does not).
    if child.inherited_attrs.contains(&prop.name) || child.inherited_attrs.contains(&camel_name) {
        return false;
    }

    // A non-identifier attribute name (`data-foo`) on a child that inherits
    // SOMETHING: the carrier cannot check it either (see `is_identifier_name`),
    // so neither does this lint. A child that inherits nothing — a fragment, or
    // `inheritAttrs: false` — still reports it, because there it reaches
    // nothing at all.
    if !is_identifier_name(&prop.name) && child.inherits_anything() {
        return false;
    }

    true
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

/// Get the set of declared event names from a child's analysis.
///
/// Sources: `macros[DefineEmits].emit_fields` (type-based and runtime) plus
/// `template.emit_definitions`. The template projection drops self-consuming
/// `defineModel` events and is suppressed entirely when the emit binding
/// escapes, so neither source subsumes the other.
fn child_emit_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    let mut names: HashSet<String> = child
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .flat_map(|m| m.emit_fields.iter().map(|f| f.name.clone()))
        .collect();
    if let Some(t) = &child.template {
        names.extend(t.emit_definitions.iter().map(|e| e.event_name.clone()));
    }
    names
}

/// Check whether the child declares the model `name` used by `v-model:name`.
///
/// `v-model:foo` is sugar for the pair `foo` prop + `update:foo` emit;
/// `defineModel` is itself sugar over that same pair. Both spellings are
/// therefore declarations of the model — recognising only the macro would
/// flag every classically-written model as unknown.
fn child_declares_model(
    name: &str,
    defined_models: &HashSet<String>,
    defined_props: &HashSet<String>,
    defined_emits: &HashSet<String>,
) -> bool {
    if defined_models.contains(name) {
        return true;
    }

    // Template arguments are authored in either casing (`v-model:my-value`
    // binds the `myValue` prop), so compare on the camelCase normalization
    // used for prop matching and accept either authored emit spelling.
    let camel_name = kebab_to_camel(name);
    defined_props.contains(&camel_name)
        && (defined_emits.contains(&format!("update:{camel_name}"))
            || defined_emits.contains(&format!("update:{name}")))
}

/// Find unknown v-models across all component usages.
pub fn find_unknown_models(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<ResolvedChildComponent>,
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

        let defined_models = child_model_names(&child.analysis);
        let defined_props = child_prop_names(&child.analysis);
        let defined_emits = child_emit_names(&child.analysis);

        for vmodel in &comp.v_models {
            if !child_declares_model(
                &vmodel.binding_name,
                &defined_models,
                &defined_props,
                &defined_emits,
            ) {
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

/// Information about a missing required slot on a component usage.
pub struct MissingRequiredSlotInfo {
    pub component_name: String,
    pub slot_name: String,
    pub import_source: String,
    pub span: verter_span::Span,
}

/// Get required slot names from a child component's defineSlots.
fn child_required_slot_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    child
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| {
            m.slot_fields
                .iter()
                .filter(|f| f.is_required)
                .map(|f| f.name.clone())
        })
        .collect()
}

/// Find missing required slots across all component usages.
pub fn find_missing_required_slots(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<ResolvedChildComponent>,
) -> Vec<MissingRequiredSlotInfo> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for comp in &template.components {
        if comp.is_dynamic {
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

        let required = child_required_slot_names(&child.analysis);
        if required.is_empty() {
            continue;
        }

        let provided: HashSet<&str> = comp.slots_used.iter().map(|s| s.as_str()).collect();

        for slot_name in &required {
            if !provided.contains(slot_name.as_str()) {
                results.push(MissingRequiredSlotInfo {
                    component_name: comp.name.clone(),
                    slot_name: slot_name.clone(),
                    import_source: import_source.to_string(),
                    span: comp.span,
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
    resolve_child: &dyn Fn(&str) -> Option<ResolvedChildComponent>,
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

    // Missing required slot diagnostics
    let missing_slots = find_missing_required_slots(analysis, resolve_child);
    for info in &missing_slots {
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
            code: Some(NumberOrString::String(
                "verter/missing-required-slot".into(),
            )),
            source: Some("verter".into()),
            message: format!(
                "Required slot '{}' is not provided on component <{}>",
                info.slot_name, info.component_name
            ),
            ..Default::default()
        });
    }

    diagnostics
}

#[cfg(test)]
#[path = "component_diagnostics_tests.rs"]
mod component_diagnostics_tests;
