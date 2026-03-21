//! Component metadata extraction from analysis snapshots.
//!
//! Pure analysis-domain types and extraction logic for component-meta.
//! This module does NOT depend on `verter_host` — all resolved data
//! is pre-supplied via [`ComponentMetaInput`].
//!
//! # Ownership boundary
//!
//! - All types in [`ComponentMetaInput`] are owned by `verter_analysis`
//! - The host constructs the input by projecting from its internal snapshots
//! - [`ComponentMetaAnalysis`] is the analysis-domain result (no serde)
//! - Conversion to FFI/binding DTOs happens at the `verter_ffi` boundary

use crate::type_expr::TypeExpr;
use crate::types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind,
    AnalyzedOptionsApi, AnalyzedPropField, JsdocTag, StoreUsage, VueApiCallSite,
};

/// Convenience: build `TypeExpr::Unknown { raw }` from a string.
fn unknown_type(raw: impl Into<String>) -> TypeExpr {
    TypeExpr::Unknown { raw: raw.into() }
}

// ═══════════════════════════════════════════════════════════════════════════
// Input view
// ═══════════════════════════════════════════════════════════════════════════

/// Input view for component-meta extraction.
///
/// All fields reference existing `verter_analysis` types.
/// The host constructs this by projecting from its internal snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComponentMetaFeatures {
    /// When true, extractor consumes `evaluated_types` to materialize
    /// expanded native `TypeExpr` results. When false, raw annotations are
    /// preserved but deep-expanded types stay disabled.
    pub expanded_types: bool,
}

pub struct ComponentMetaInput<'a> {
    pub macros: &'a [AnalyzedMacro],
    pub bindings: &'a [AnalyzedBinding],
    pub imports: &'a [AnalyzedImport],
    pub template: Option<&'a crate::template::TemplateAnalysisSnapshot>,
    pub options_api: Option<&'a AnalyzedOptionsApi>,
    pub analysis_flags: AnalysisFlags,
    pub features: ComponentMetaFeatures,
    pub styles: &'a [crate::style::StyleBlockAnalysis],
    pub vue_api_calls: &'a [VueApiCallSite],
    pub store_usages: &'a [StoreUsage],
    pub evaluated_types: Option<&'a crate::type_eval_build::EvaluatedComponentTypes>,
    pub file_path: &'a str,
}

// ═══════════════════════════════════════════════════════════════════════════
// Domain result types
// ═══════════════════════════════════════════════════════════════════════════

/// Analysis-domain component metadata. No serde — only used in Rust.
/// Converted to `FfiComponentMeta` at the NAPI/WASM boundary.
#[derive(Debug, Clone)]
pub struct ComponentMetaAnalysis {
    pub props: Vec<PropAnalysis>,
    pub events: Vec<EventAnalysis>,
    pub slots: Vec<SlotAnalysis>,
    pub models: Vec<ModelAnalysis>,
    pub exposed: Vec<ExposedAnalysis>,
    pub type_registry: Vec<ResolvedTypeAnalysis>,
    pub components: Vec<ComponentUsageAnalysis>,
    pub template_refs: Vec<TemplateRefAnalysis>,
    pub imports: Vec<ImportAnalysis>,
    pub bindings: Vec<BindingAnalysis>,
    pub vue_api_calls: Vec<VueApiCallAnalysis>,
    pub styles: Vec<StyleAnalysis>,
    pub flags: ComponentMetaFlags,
    pub options_api: bool,
    pub file_path: String,
}

/// Analyzed prop from `defineProps` / Options API `props`.
#[derive(Debug, Clone)]
pub struct PropAnalysis {
    pub name: String,
    /// Resolved via priority chain: evaluated TypeExpr > raw annotation > Unknown.
    pub type_expr: TypeExpr,
    /// Original annotation text from the source.
    pub raw_type: Option<String>,
    pub required: bool,
    pub has_default: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// Analyzed event from `defineEmits`.
#[derive(Debug, Clone)]
pub struct EventAnalysis {
    pub name: String,
    pub payload: TypeExpr,
    pub raw_signature: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// Analyzed slot from `defineSlots` / template.
#[derive(Debug, Clone)]
pub struct SlotAnalysis {
    pub name: String,
    pub is_scoped: bool,
    pub bindings: Vec<SlotBindingAnalysis>,
    pub is_required: bool,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// A single binding property on a scoped slot.
#[derive(Debug, Clone)]
pub struct SlotBindingAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
    pub raw_type: Option<String>,
}

/// Analyzed model from `defineModel`.
#[derive(Debug, Clone)]
pub struct ModelAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
}

/// Analyzed exposed member from `defineExpose`.
#[derive(Debug, Clone)]
pub struct ExposedAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
    pub description: Option<String>,
}

/// A named resolved type available for schema expansion.
#[derive(Debug, Clone)]
pub struct ResolvedTypeAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
}

/// A component usage discovered in the template.
#[derive(Debug, Clone)]
pub struct ComponentUsageAnalysis {
    pub name: String,
    pub import_source: Option<String>,
    pub is_dynamic: bool,
    pub props: Vec<ComponentPropUsageAnalysis>,
    pub slots_used: Vec<String>,
    pub static_classes: Vec<String>,
    pub has_dynamic_class: bool,
    pub v_models: Vec<String>,
}

/// A single prop passed to a child component in the template.
#[derive(Debug, Clone)]
pub struct ComponentPropUsageAnalysis {
    pub name: String,
    pub is_bound: bool,
    pub constness: crate::template::PropValueConstness,
}

/// A template ref usage.
#[derive(Debug, Clone)]
pub struct TemplateRefAnalysis {
    pub name: String,
    pub is_dynamic: bool,
    pub target_tag: String,
}

/// A script import.
#[derive(Debug, Clone)]
pub struct ImportAnalysis {
    pub source: String,
    pub is_type_only: bool,
    pub bindings: Vec<ImportBindingAnalysis>,
}

/// A single imported binding.
#[derive(Debug, Clone)]
pub struct ImportBindingAnalysis {
    pub name: String,
    pub is_type_only: bool,
}

/// Declaration kind for a script binding in the component-meta result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKindAnalysis {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
}

/// A script-level binding.
#[derive(Debug, Clone)]
pub struct BindingAnalysis {
    pub name: String,
    pub kind: BindingKindAnalysis,
    pub reactivity_kind: crate::types::ReactivityKind,
    pub type_annotation: Option<String>,
    pub used_in_template: bool,
    pub used_in_style: bool,
}

/// A Vue API call site.
#[derive(Debug, Clone)]
pub struct VueApiCallAnalysis {
    pub api: crate::types::VueApiClassification,
    pub arg_value: Option<String>,
}

/// Analysis of a single style block.
#[derive(Debug, Clone)]
pub struct StyleAnalysis {
    pub lang: crate::style::StyleAnalysisLang,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,
    pub classes: Vec<String>,
    pub ids: Vec<String>,
    pub custom_properties: Vec<String>,
    pub v_binds: Vec<String>,
    pub selectors: Vec<SelectorAnalysis>,
}

/// A CSS selector plus specificity.
#[derive(Debug, Clone)]
pub struct SelectorAnalysis {
    pub text: String,
    pub specificity: (u32, u32, u32),
}

/// Capability flags derived from script analysis.
#[derive(Debug, Clone, Default)]
pub struct ComponentMetaFlags {
    pub async_setup: bool,
    pub has_reactive_state: bool,
    pub has_computed: bool,
    pub has_watchers: bool,
    pub has_lifecycle_hooks: bool,
    pub has_provide: bool,
    pub has_inject: bool,
    pub has_inherit_attrs_false: bool,
    pub has_store_usage: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Extraction
// ═══════════════════════════════════════════════════════════════════════════

/// Extract component metadata from pre-resolved analysis-owned inputs.
///
/// Does NOT access host/VFS/workspace — all resolved data is pre-supplied.
/// Source order of props/events/slots/exposed is preserved.
pub fn extract_component_meta(input: ComponentMetaInput<'_>) -> ComponentMetaAnalysis {
    let options_api = input.options_api.is_some();
    let flags = extract_flags(&input);
    let evaluated_types = if input.features.expanded_types {
        input.evaluated_types
    } else {
        None
    };

    let mut props = Vec::new();
    let mut events = Vec::new();
    let mut slots = Vec::new();
    let mut models = Vec::new();
    let mut exposed = Vec::new();

    // Collect defaults from all prop-bearing macro forms.
    let default_keys: std::collections::HashSet<&str> = input
        .macros
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineProps
            )
        })
        .flat_map(|m| m.default_keys.iter().map(|k| k.as_str()))
        .collect();

    // Runtime defineProps({ ... default }) stores defaults on the DefineProps macro
    // itself, while withDefaults() stores them on the WithDefaults wrapper.
    let default_values: std::collections::HashMap<&str, &str> = input
        .macros
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineProps
            )
        })
        .flat_map(|m| {
            m.default_values
                .iter()
                .map(|dv| (dv.key.as_str(), dv.value.as_str()))
        })
        .collect();

    for (macro_index, mac) in input.macros.iter().enumerate() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                extract_props_from_macro(
                    macro_index,
                    mac,
                    &default_keys,
                    &default_values,
                    evaluated_types,
                    &mut props,
                );
            }
            AnalyzedMacroKind::DefineEmits => {
                extract_events_from_macro(mac, evaluated_types, &mut events);
            }
            AnalyzedMacroKind::DefineSlots => {
                extract_slots_from_macro(mac, evaluated_types, &mut slots);
            }
            AnalyzedMacroKind::DefineModel => {
                extract_model_from_macro(mac, evaluated_types, &mut models);
            }
            AnalyzedMacroKind::DefineExpose => {
                extract_exposed_from_macro(mac, input.bindings, evaluated_types, &mut exposed);
            }
            AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineOptions => {
                // Handled above (default_keys) or flags
            }
        }
    }

    // Merge template-discovered slots with defineSlots
    if let Some(tpl) = input.template {
        merge_template_slots(&tpl.defined_slots, &mut slots);
    }

    // Options API fallback
    if let Some(opts) = input.options_api {
        if props.is_empty() {
            extract_props_from_options(opts, &mut props);
        }
        if events.is_empty() {
            extract_events_from_options(opts, &mut events);
        }
    }

    for mac in input
        .macros
        .iter()
        .filter(|mac| mac.kind == AnalyzedMacroKind::DefineModel)
    {
        synthesize_model_prop_and_event(mac, evaluated_types, &mut props, &mut events);
    }

    let type_registry = extract_type_registry(input.macros);
    let components = extract_components(input.template);
    let template_refs = extract_template_refs(input.template);
    let imports = extract_imports(input.imports);
    let bindings = extract_bindings(input.bindings, input.template);
    let vue_api_calls = extract_vue_api_calls(input.vue_api_calls);
    let styles = extract_styles(input.styles);

    ComponentMetaAnalysis {
        props,
        events,
        slots,
        models,
        exposed,
        type_registry,
        components,
        template_refs,
        imports,
        bindings,
        vue_api_calls,
        styles,
        flags,
        options_api,
        file_path: input.file_path.to_string(),
    }
}

// ── Props ──────────────────────────────────────────────────────────────────

fn extract_props_from_macro(
    macro_index: usize,
    mac: &AnalyzedMacro,
    default_keys: &std::collections::HashSet<&str>,
    default_values: &std::collections::HashMap<&str, &str>,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    out: &mut Vec<PropAnalysis>,
) {
    let mut seen = std::collections::HashSet::new();

    for field in &mac.prop_fields {
        let type_expr = resolve_prop_type(field, evaluated);
        let has_default = default_keys.contains(field.name.as_str());
        let default_value = default_values
            .get(field.name.as_str())
            .map(|v| v.to_string());
        seen.insert(field.name.clone());

        out.push(PropAnalysis {
            name: field.name.clone(),
            type_expr,
            raw_type: field.type_annotation.clone(),
            required: !field.is_optional && !has_default,
            has_default,
            default_value,
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }

    if let Some(eval_fields) = evaluated_define_props_fields(evaluated, macro_index) {
        for field in eval_fields {
            if !seen.insert(field.name.clone()) {
                continue;
            }

            let has_default = default_keys.contains(field.name.as_str());
            let default_value = default_values
                .get(field.name.as_str())
                .map(|v| v.to_string());

            out.push(PropAnalysis {
                name: field.name.clone(),
                type_expr: field.r#type.clone(),
                raw_type: None,
                required: !field.optional && !has_default,
                has_default,
                default_value,
                description: None,
                tags: Vec::new(),
            });
        }
    }
}

fn evaluated_define_props_fields<'a>(
    evaluated: Option<&'a crate::type_eval_build::EvaluatedComponentTypes>,
    macro_index: usize,
) -> Option<&'a [crate::type_eval_build::EvaluatedField]> {
    evaluated?
        .define_props
        .iter()
        .find(|entry| entry.macro_index == macro_index)
        .map(|entry| entry.fields.as_slice())
}

/// Resolve prop type via priority chain:
/// 1. Evaluated TypeExpr (preferred)
/// 2. Raw annotation text → TypeExpr::Unknown
fn resolve_prop_type(
    field: &AnalyzedPropField,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
) -> TypeExpr {
    if let Some(eval) = evaluated {
        if let Some(ef) = eval.props.iter().find(|f| f.name == field.name) {
            return ef.r#type.clone();
        }
    }
    match &field.type_annotation {
        Some(raw) => unknown_type(raw.clone()),
        None => unknown_type("unknown".to_string()),
    }
}

// ── Events ─────────────────────────────────────────────────────────────────

fn extract_events_from_macro(
    mac: &AnalyzedMacro,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    out: &mut Vec<EventAnalysis>,
) {
    for field in &mac.emit_fields {
        let payload = if let Some(eval) = evaluated {
            eval.emits
                .iter()
                .find(|f| f.name == field.name)
                .map(|f| f.r#type.clone())
                .unwrap_or_else(|| match &field.payload_type {
                    Some(raw) => unknown_type(raw.clone()),
                    None => unknown_type("unknown".to_string()),
                })
        } else {
            match &field.payload_type {
                Some(raw) => unknown_type(raw.clone()),
                None => unknown_type("unknown".to_string()),
            }
        };

        out.push(EventAnalysis {
            name: field.name.clone(),
            payload,
            raw_signature: field.payload_type.clone(),
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

// ── Slots ──────────────────────────────────────────────────────────────────

fn extract_slots_from_macro(
    mac: &AnalyzedMacro,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    out: &mut Vec<SlotAnalysis>,
) {
    for field in &mac.slot_fields {
        let bindings: Vec<SlotBindingAnalysis> = field
            .bindings
            .iter()
            .map(|b| {
                let type_expr = if let Some(eval) = evaluated {
                    // Slot bindings are keyed as "slotName.bindingName" in EvaluatedComponentTypes
                    let key = format!("{}.{}", field.name, b.name);
                    eval.slot_bindings
                        .iter()
                        .find(|f| f.name == key)
                        .map(|f| f.r#type.clone())
                        .unwrap_or_else(|| match &b.type_annotation {
                            Some(raw) => unknown_type(raw.clone()),
                            None => unknown_type("unknown".to_string()),
                        })
                } else {
                    match &b.type_annotation {
                        Some(raw) => unknown_type(raw.clone()),
                        None => unknown_type("unknown".to_string()),
                    }
                };
                SlotBindingAnalysis {
                    name: b.name.clone(),
                    type_expr,
                    raw_type: b.type_annotation.clone(),
                }
            })
            .collect();

        out.push(SlotAnalysis {
            name: field.name.clone(),
            is_scoped: !field.bindings.is_empty(),
            bindings,
            is_required: field.is_required,
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

fn merge_template_slots(
    template_slots: &[crate::template::DefinedSlot],
    out: &mut Vec<SlotAnalysis>,
) {
    for tslot in template_slots {
        if !out.iter().any(|s| s.name == tslot.name) {
            out.push(SlotAnalysis {
                name: tslot.name.clone(),
                is_scoped: tslot.has_bindings,
                bindings: Vec::new(),
                is_required: false,
                description: None,
                tags: Vec::new(),
            });
        }
    }
}

// ── Models ─────────────────────────────────────────────────────────────────

fn extract_model_from_macro(
    mac: &AnalyzedMacro,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    out: &mut Vec<ModelAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());

    // Try evaluated type from props (model generates a prop with the model name)
    let type_expr = if let Some(eval) = evaluated {
        eval.props
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.r#type.clone())
            .unwrap_or_else(|| unknown_type("unknown".to_string()))
    } else {
        // Fall back to prop_fields on the macro itself
        mac.prop_fields
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.type_annotation.as_ref())
            .map(|raw| unknown_type(raw.clone()))
            .unwrap_or_else(|| unknown_type("unknown".to_string()))
    };

    out.push(ModelAnalysis { name, type_expr });
}

// ── Exposed ────────────────────────────────────────────────────────────────

fn synthesize_model_prop_and_event(
    mac: &AnalyzedMacro,
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    props: &mut Vec<PropAnalysis>,
    events: &mut Vec<EventAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());
    let has_default = mac.default_keys.iter().any(|key| key == &name);
    let raw_type = mac
        .prop_fields
        .iter()
        .find(|field| field.name == name)
        .and_then(|field| field.type_annotation.clone());

    if !props.iter().any(|prop| prop.name == name) {
        let mut type_expr = evaluated
            .and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(|field| field.r#type.clone())
            })
            .or_else(|| raw_type.as_ref().map(|raw| unknown_type(raw.clone())))
            .unwrap_or_else(|| unknown_type("unknown".to_string()));

        let prop_raw_type = if has_default {
            raw_type.as_ref().map(|raw| format!("{raw} | undefined"))
        } else {
            raw_type.clone()
        };

        if has_default {
            type_expr = match type_expr {
                TypeExpr::Unknown { .. } => type_expr,
                other => TypeExpr::Union(vec![
                    other,
                    TypeExpr::Primitive(crate::type_expr::PrimitiveName::Undefined),
                ]),
            };
        }

        props.push(PropAnalysis {
            name: name.clone(),
            type_expr,
            raw_type: prop_raw_type,
            required: !has_default,
            has_default,
            default_value: None,
            description: None,
            tags: Vec::new(),
        });
    }

    let event_name = format!("update:{name}");
    if events.iter().any(|event| event.name == event_name) {
        return;
    }

    let raw_signature = raw_type.as_ref().map(|raw| format!("[value: {raw}]"));
    let payload = evaluated
        .and_then(|eval| {
            eval.emits
                .iter()
                .find(|field| field.name == event_name)
                .map(|field| field.r#type.clone())
        })
        .or_else(|| raw_signature.as_ref().map(|raw| unknown_type(raw.clone())))
        .unwrap_or_else(|| unknown_type("unknown".to_string()));

    events.push(EventAnalysis {
        name: event_name,
        payload,
        raw_signature,
        description: None,
        tags: Vec::new(),
    });
}

fn extract_exposed_from_macro(
    mac: &AnalyzedMacro,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
    out: &mut Vec<ExposedAnalysis>,
) {
    for field in &mac.expose_fields {
        let type_expr = resolve_exposed_type(&field.name, bindings, evaluated);
        out.push(ExposedAnalysis {
            name: field.name.clone(),
            type_expr,
            description: None,
        });
    }
}

fn resolve_exposed_type(
    name: &str,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::type_eval_build::EvaluatedComponentTypes>,
) -> TypeExpr {
    if let Some(eval) = evaluated {
        if let Some(f) = eval.bindings.iter().find(|f| f.name == name) {
            return f.r#type.clone();
        }
    }
    // Fall back to binding type annotation if available
    if let Some(binding) = bindings.iter().find(|b| b.name == name) {
        if let Some(ref ann) = binding.type_annotation {
            return unknown_type(ann.clone());
        }
    }
    unknown_type("unknown".to_string())
}

// ── Options API fallback ───────────────────────────────────────────────────

fn extract_props_from_options(opts: &AnalyzedOptionsApi, out: &mut Vec<PropAnalysis>) {
    for prop in &opts.props {
        let raw_type = prop
            .type_annotation
            .clone()
            .or_else(|| prop.type_constructor.clone());
        out.push(PropAnalysis {
            name: prop.name.clone(),
            type_expr: prop
                .type_annotation
                .as_ref()
                .map(|raw| unknown_type(raw.clone()))
                .or_else(|| {
                    prop.type_constructor.as_ref().map(|rt| match rt.as_str() {
                        "String" => TypeExpr::Primitive(crate::type_expr::PrimitiveName::String),
                        "Number" => TypeExpr::Primitive(crate::type_expr::PrimitiveName::Number),
                        "Boolean" => TypeExpr::Primitive(crate::type_expr::PrimitiveName::Boolean),
                        "Function" => unknown_type("Function".to_string()),
                        "Array" => unknown_type("Array".to_string()),
                        "Object" => unknown_type("Object".to_string()),
                        other => unknown_type(other.to_string()),
                    })
                })
                .unwrap_or_else(|| unknown_type("unknown".to_string())),
            raw_type,
            required: prop.is_required,
            has_default: prop.has_default,
            default_value: prop.default_value.clone(),
            description: prop.description.clone(),
            tags: prop.tags.clone(),
        });
    }
}

fn extract_events_from_options(opts: &AnalyzedOptionsApi, out: &mut Vec<EventAnalysis>) {
    for field in &opts.emits {
        out.push(EventAnalysis {
            name: field.name.clone(),
            payload: match &field.payload_type {
                Some(raw) => unknown_type(raw.clone()),
                None => unknown_type("unknown".to_string()),
            },
            raw_signature: field.payload_type.clone(),
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

// ── Flags ──────────────────────────────────────────────────────────────────

fn extract_type_registry(macros: &[AnalyzedMacro]) -> Vec<ResolvedTypeAnalysis> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut registry = Vec::new();

    for mac in macros {
        for resolved in &mac.resolved_local_types {
            if seen.insert(resolved.name.clone()) {
                registry.push(ResolvedTypeAnalysis {
                    name: resolved.name.clone(),
                    type_expr: crate::type_expr_lower::parse_type_annotation(&resolved.expanded),
                });
            }
        }
    }

    registry
}

fn extract_components(
    template: Option<&crate::template::TemplateAnalysisSnapshot>,
) -> Vec<ComponentUsageAnalysis> {
    let Some(template) = template else {
        return Vec::new();
    };

    template
        .components
        .iter()
        .map(|component| ComponentUsageAnalysis {
            name: component.name.clone(),
            import_source: component.import_source.clone(),
            is_dynamic: component.is_dynamic,
            props: component
                .props
                .iter()
                .map(|prop| ComponentPropUsageAnalysis {
                    name: prop.name.clone(),
                    is_bound: prop.is_bound,
                    constness: prop.constness,
                })
                .collect(),
            slots_used: component.slots_used.clone(),
            static_classes: component.static_classes.clone(),
            has_dynamic_class: component.has_dynamic_class,
            v_models: component
                .v_models
                .iter()
                .map(|model| model.binding_name.clone())
                .collect(),
        })
        .collect()
}

fn extract_template_refs(
    template: Option<&crate::template::TemplateAnalysisSnapshot>,
) -> Vec<TemplateRefAnalysis> {
    let Some(template) = template else {
        return Vec::new();
    };

    template
        .template_refs
        .iter()
        .map(|template_ref| TemplateRefAnalysis {
            name: template_ref.name.clone(),
            is_dynamic: template_ref.is_dynamic,
            target_tag: template_ref.target_tag.clone(),
        })
        .collect()
}

fn extract_imports(imports: &[AnalyzedImport]) -> Vec<ImportAnalysis> {
    imports
        .iter()
        .map(|import| ImportAnalysis {
            source: import.source.clone(),
            is_type_only: import.is_type_only,
            bindings: import
                .bindings
                .iter()
                .map(|binding| ImportBindingAnalysis {
                    name: binding.name.clone(),
                    is_type_only: binding.is_type_only,
                })
                .collect(),
        })
        .collect()
}

fn extract_bindings(
    bindings: &[AnalyzedBinding],
    template: Option<&crate::template::TemplateAnalysisSnapshot>,
) -> Vec<BindingAnalysis> {
    let template_bindings: std::collections::HashSet<&str> = template
        .map(|template| {
            template
                .binding_occurrences
                .iter()
                .map(|occurrence| occurrence.name.as_str())
                .collect()
        })
        .unwrap_or_default();

    bindings
        .iter()
        .map(|binding| BindingAnalysis {
            name: binding.name.clone(),
            kind: match binding.kind {
                crate::types::AnalyzedBindingKind::Const => BindingKindAnalysis::Const,
                crate::types::AnalyzedBindingKind::Let => BindingKindAnalysis::Let,
                crate::types::AnalyzedBindingKind::Var => BindingKindAnalysis::Var,
                crate::types::AnalyzedBindingKind::Function => BindingKindAnalysis::Function,
                crate::types::AnalyzedBindingKind::AsyncFunction => {
                    BindingKindAnalysis::AsyncFunction
                }
                crate::types::AnalyzedBindingKind::Class => BindingKindAnalysis::Class,
            },
            reactivity_kind: binding.reactivity_kind,
            type_annotation: binding.type_annotation.clone(),
            used_in_template: template_bindings.contains(binding.name.as_str()),
            used_in_style: binding.used_in_style,
        })
        .collect()
}

fn extract_vue_api_calls(calls: &[VueApiCallSite]) -> Vec<VueApiCallAnalysis> {
    calls
        .iter()
        .map(|call| VueApiCallAnalysis {
            api: call.api,
            arg_value: call.arg_value.clone(),
        })
        .collect()
}

fn extract_styles(styles: &[crate::style::StyleBlockAnalysis]) -> Vec<StyleAnalysis> {
    styles
        .iter()
        .map(|style| {
            let css = style.css.as_ref();

            StyleAnalysis {
                lang: style.lang.clone(),
                scoped: style.scoped,
                is_module: style.is_module,
                module_name: style.module_name.clone(),
                classes: css
                    .map(|css| css.classes.iter().map(|class| class.name.clone()).collect())
                    .unwrap_or_default(),
                ids: css
                    .map(|css| css.ids.iter().map(|id| id.name.clone()).collect())
                    .unwrap_or_default(),
                custom_properties: css
                    .map(|css| {
                        css.custom_properties
                            .iter()
                            .map(|property| property.name.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                v_binds: style
                    .v_binds
                    .iter()
                    .map(|v_bind| v_bind.expression.clone())
                    .collect(),
                selectors: css
                    .map(|css| {
                        css.selectors
                            .iter()
                            .map(|selector| SelectorAnalysis {
                                text: selector.text.clone(),
                                specificity: selector.specificity,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn extract_flags(input: &ComponentMetaInput<'_>) -> ComponentMetaFlags {
    let has_inherit_attrs_false = input
        .macros
        .iter()
        .any(|m| m.kind == AnalyzedMacroKind::DefineOptions && m.has_inherit_attrs_false);

    let flags = input.analysis_flags;

    ComponentMetaFlags {
        async_setup: flags.contains(AnalysisFlags::ASYNC_SETUP),
        has_reactive_state: flags.contains(AnalysisFlags::HAS_REACTIVE_STATE),
        has_computed: flags.contains(AnalysisFlags::HAS_COMPUTED),
        has_watchers: flags.contains(AnalysisFlags::HAS_WATCHERS),
        has_lifecycle_hooks: flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS),
        has_provide: flags.contains(AnalysisFlags::HAS_PROVIDE),
        has_inject: flags.contains(AnalysisFlags::HAS_INJECT),
        has_inherit_attrs_false: flags.contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE)
            || has_inherit_attrs_false,
        has_store_usage: !input.store_usages.is_empty(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "component_meta_tests.rs"]
mod component_meta_tests;
