//! Extraction of semantic facts from analysis snapshots.
//!
//! Converts `verter_semantic::analysis` types into `verter_semantic` fact types.
//! This is the bridge between the raw analysis layer and the semantic DB.

use crate::input::{AnalyzedMacro, AnalyzedMacroKind, ReactivityKind, ScriptAnalysisSnapshot};

use crate::facts::binding::{BindingDeclaration, BindingKind, BindingUsage, UsageBlock, UsageKind};
use crate::facts::boundary::ComponentInstanceEdge;
use crate::facts::component::{
    ComponentSurface, DeclaredSurface, EventFact, ExposeFact, ModelFact, PropFact, SlotBindingFact,
    SlotFact,
};
use crate::facts::reactivity::{ReactivityFact, ReactivitySource, ReactivityStatus};
use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

/// Extract the declared component surface from a script analysis snapshot.
///
/// Reads `defineProps`, `defineEmits`, `defineSlots`, `defineModel`, and
/// `defineExpose` macros from the analysis to build the declared surface.
/// Does not include inherited/fallthrough surface (that requires cross-file
/// resolution via the session).
pub fn extract_declared_surface(analysis: &ScriptAnalysisSnapshot) -> DeclaredSurface {
    let mut surface = DeclaredSurface::default();

    for mac in analysis.macros.iter() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                extract_props_from_macro(mac, &mut surface.props);
            }
            AnalyzedMacroKind::DefineEmits => {
                extract_events_from_macro(mac, &mut surface.events);
            }
            AnalyzedMacroKind::DefineSlots => {
                extract_slots_from_macro(mac, &mut surface.slots);
            }
            AnalyzedMacroKind::DefineModel => {
                extract_model_from_macro(mac, &mut surface.models);
            }
            AnalyzedMacroKind::DefineExpose => {
                extract_expose_from_macro(mac, &mut surface.expose);
            }
            AnalyzedMacroKind::DefineOptions => {
                // DefineOptions doesn't contribute to the declared surface directly.
            }
        }
    }

    surface
}

/// Extract the full component surface for a single file (no cross-file resolution).
///
/// Returns a `ComponentSurface` with declared props/events/slots/models and
/// accepted surfaces equal to declared (since no fallthrough is resolved here).
pub fn extract_component_surface(analysis: &ScriptAnalysisSnapshot) -> ComponentSurface {
    let declared = extract_declared_surface(analysis);

    // Without cross-file resolution, accepted = declared
    let accepted_props = declared.props.clone();
    let accepted_events = declared.events.clone();

    let inherit_attrs_disabled = analysis.macros.iter().any(|m| m.has_inherit_attrs_false);

    ComponentSurface {
        declared,
        accepted_props,
        accepted_events,
        completeness: None, // Not resolved yet
        inherit_attrs_disabled,
    }
}

fn extract_props_from_macro(mac: &AnalyzedMacro, props: &mut Vec<PropFact>) {
    for field in &mac.prop_fields {
        // Skip if already present (withDefaults can duplicate)
        if props.iter().any(|p| p.name == field.name) {
            // Update default value from withDefaults
            if mac.kind == AnalyzedMacroKind::WithDefaults {
                if let Some(existing) = props.iter_mut().find(|p| p.name == field.name) {
                    if existing.default_value.is_none() {
                        if let Some(dv) = mac.default_values.iter().find(|dv| dv.key == field.name)
                        {
                            existing.default_value = Some(dv.value.clone());
                            existing.is_optional = true;
                        }
                    }
                }
            }
            continue;
        }

        let mut default_value = None;
        let mut is_optional = field.is_optional;

        // Check withDefaults for this prop
        if mac.kind == AnalyzedMacroKind::WithDefaults {
            if let Some(dv) = mac.default_values.iter().find(|dv| dv.key == field.name) {
                default_value = Some(dv.value.clone());
                is_optional = true;
            }
        }

        props.push(PropFact {
            name: field.name.clone(),
            is_optional,
            type_text: field.type_annotation.clone(),
            default_value,
            description: field.description.clone(),
            span: field.span,
        });
    }
}

fn extract_events_from_macro(mac: &AnalyzedMacro, events: &mut Vec<EventFact>) {
    for field in &mac.emit_fields {
        events.push(EventFact {
            name: field.name.clone(),
            payload_type: field.payload_type.clone(),
            description: field.description.clone(),
            span: field.span,
        });
    }
}

fn extract_slots_from_macro(mac: &AnalyzedMacro, slots: &mut Vec<SlotFact>) {
    for field in &mac.slot_fields {
        let bindings = field
            .bindings
            .iter()
            .map(|b| SlotBindingFact {
                name: b.name.clone(),
                type_text: b.type_annotation.clone(),
            })
            .collect();

        slots.push(SlotFact {
            name: field.name.clone(),
            is_required: field.is_required,
            bindings,
            description: field.description.clone(),
            span: field.span,
        });
    }
}

fn extract_model_from_macro(mac: &AnalyzedMacro, models: &mut Vec<ModelFact>) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());

    // Get the type from the first prop field (defineModel creates a prop)
    let type_text = mac
        .prop_fields
        .first()
        .and_then(|f| f.type_annotation.clone());

    models.push(ModelFact {
        name,
        type_text,
        span: mac.span,
    });
}

fn extract_expose_from_macro(mac: &AnalyzedMacro, expose: &mut Vec<ExposeFact>) {
    for field in &mac.expose_fields {
        expose.push(ExposeFact {
            name: field.name.clone(),
            span: field.span,
        });
    }
}

// ── Binding and reactivity extraction ──────────────────────────────────────

/// Extract binding declarations with reactivity facts from a script analysis.
///
/// Each `AnalyzedBinding` is converted to a `BindingDeclaration` with an
/// associated `ReactivityFact`. Usage tracking is populated from the
/// analysis snapshot's `used_in_script` / `used_in_style` flags and
/// template-side binding occurrences.
pub fn extract_bindings(
    analysis: &ScriptAnalysisSnapshot,
) -> Vec<(BindingDeclaration, ReactivityFact)> {
    analysis
        .bindings
        .iter()
        .map(|b| {
            let kind = match b.kind {
                crate::input::AnalyzedBindingKind::Const => BindingKind::Const,
                crate::input::AnalyzedBindingKind::Let => BindingKind::Let,
                crate::input::AnalyzedBindingKind::Var => BindingKind::Var,
                crate::input::AnalyzedBindingKind::Function => BindingKind::Function,
                crate::input::AnalyzedBindingKind::AsyncFunction => BindingKind::AsyncFunction,
                crate::input::AnalyzedBindingKind::Class => BindingKind::Class,
            };

            let mut usages = Vec::new();
            if b.used_in_script {
                usages.push(BindingUsage {
                    kind: UsageKind::Read,
                    span: b.span,
                    block: UsageBlock::Script,
                });
            }
            if b.used_in_style {
                usages.push(BindingUsage {
                    kind: UsageKind::StyleVBind,
                    span: b.span,
                    block: UsageBlock::Style,
                });
            }

            let decl = BindingDeclaration {
                name: b.name.clone(),
                kind,
                span: b.span,
                usages,
            };

            let reactivity = classify_reactivity(b);

            (decl, reactivity)
        })
        .collect()
}

/// Classify the reactivity of an analyzed binding.
fn classify_reactivity(binding: &crate::input::AnalyzedBinding) -> ReactivityFact {
    let (status, source) = match binding.reactivity_kind {
        ReactivityKind::Ref => (ReactivityStatus::Reactive, Some(ReactivitySource::Ref)),
        ReactivityKind::Computed => (ReactivityStatus::Reactive, Some(ReactivitySource::Computed)),
        ReactivityKind::Reactive => (ReactivityStatus::Reactive, Some(ReactivitySource::Reactive)),
        ReactivityKind::MaybeRef => (ReactivityStatus::MaybeReactive, None),
        ReactivityKind::Mutable => (ReactivityStatus::NonReactive, None),
        ReactivityKind::None => (ReactivityStatus::NonReactive, None),
    };

    // Enrich source from initializer if available
    let source = source.or_else(|| classify_source_from_initializer(binding));

    ReactivityFact {
        status,
        source,
        trace: Vec::new(), // Trace is populated by deeper analysis
    }
}

/// Try to determine reactivity source from the binding's initializer.
fn classify_source_from_initializer(
    binding: &crate::input::AnalyzedBinding,
) -> Option<ReactivitySource> {
    use crate::input::BindingInitializer;

    match &binding.initializer {
        Some(BindingInitializer::FunctionCall { vue_api, .. }) => {
            use crate::input::VueApiClassification;
            match vue_api.as_ref()? {
                VueApiClassification::Ref
                | VueApiClassification::ShallowRef
                | VueApiClassification::CustomRef => Some(ReactivitySource::Ref),
                VueApiClassification::Reactive | VueApiClassification::ShallowReactive => {
                    Some(ReactivitySource::Reactive)
                }
                VueApiClassification::Computed => Some(ReactivitySource::Computed),
                VueApiClassification::Readonly | VueApiClassification::ShallowReadonly => {
                    Some(ReactivitySource::Readonly)
                }
                VueApiClassification::ToRef | VueApiClassification::ToRefs => {
                    Some(ReactivitySource::ToRef)
                }
                VueApiClassification::Inject => Some(ReactivitySource::Inject),
                _ => None,
            }
        }
        _ => None,
    }
}

// ── Import graph extraction ────────────────────────────────────────────────

/// Extract the file's import graph from a script analysis snapshot.
///
/// Converts `AnalyzedImport` entries into semantic `ImportedSymbol` facts
/// with resolved canonical file IDs (when available from the host).
pub fn extract_import_graph(analysis: &ScriptAnalysisSnapshot) -> FileImportGraph {
    let mut imports = Vec::new();
    let mut source_set = rustc_hash::FxHashSet::default();

    for imp in &analysis.imports {
        if let Some(ref cid) = imp.resolved_canonical_id {
            source_set.insert(cid.clone());
        }

        for binding in &imp.bindings {
            let kind = match binding.kind {
                crate::input::ImportBindingKind::Named => ImportKind::Named,
                crate::input::ImportBindingKind::Default => ImportKind::Default,
                crate::input::ImportBindingKind::Namespace => ImportKind::Namespace,
            };

            let exported_name = match binding.kind {
                crate::input::ImportBindingKind::Default => "default".to_string(),
                _ => binding
                    .imported_name
                    .clone()
                    .unwrap_or_else(|| binding.name.clone()),
            };

            imports.push(ImportedSymbol {
                local_name: binding.name.clone(),
                source_specifier: imp.source.clone(),
                resolved_file_id: imp.resolved_canonical_id.clone(),
                exported_name,
                kind,
                is_type_only: imp.is_type_only || binding.is_type_only,
                span: binding.span,
            });
        }
    }

    FileImportGraph {
        imports,
        import_sources: source_set.into_iter().collect(),
    }
}

// ── Boundary fact extraction ───────────────────────────────────────────────

/// Extract component-instance edges from a template analysis snapshot.
///
/// Each `TemplateComponentUsage` in the template becomes a
/// `ComponentInstanceEdge` fact, linking the parent file to the child
/// component with the passed props/events/slots.
pub fn extract_boundary_edges(
    parent_file_id: &str,
    template: &TemplateAnalysisSnapshot,
    import_graph: &FileImportGraph,
) -> Vec<ComponentInstanceEdge> {
    template
        .components
        .iter()
        .map(|usage| {
            // Resolve child file ID from import graph
            let child_file_id = import_graph
                .find_by_local_name(&usage.name)
                .and_then(|sym| sym.resolved_file_id.clone());

            let passed_props: Vec<String> = usage.props.iter().map(|p| p.name.clone()).collect();

            let passed_slots: Vec<String> = usage.slots_used.clone();

            // Events come from template event handlers, not directly from component usage.
            // For now, extract from v-model bindings as update:xxx events.
            let mut passed_events: Vec<String> = Vec::new();
            for model in &usage.v_models {
                passed_events.push(format!("update:{}", model.binding_name));
            }

            ComponentInstanceEdge {
                parent_file_id: parent_file_id.to_string(),
                child_file_id,
                tag_name: usage.name.clone(),
                usage_span: usage.span,
                passed_props,
                passed_events,
                passed_slots,
                has_spread: usage.has_spread,
                has_event_spread: false,
            }
        })
        .collect()
}

// ── Prop constness extraction ──────────────────────────────────────────────

use crate::facts::component::{PropConstness, PropConstnessFact};
use crate::input::{PropValueConstness, TemplateAnalysisSnapshot};

/// Compute per-prop constness for a child component across all call sites
/// in a single parent template.
///
/// Given a child component name and the parent's template analysis, examines
/// all usage sites of that component and classifies each prop as
/// `AlwaysConst`, `SometimesDynamic`, or `Unknown`.
pub fn compute_prop_constness_for_child(
    child_component_name: &str,
    parent_template: &TemplateAnalysisSnapshot,
) -> Vec<PropConstnessFact> {
    let usages: Vec<_> = parent_template
        .components
        .iter()
        .filter(|c| c.name == child_component_name)
        .collect();

    if usages.is_empty() {
        return Vec::new();
    }

    // Collect all prop names across all usages
    let mut prop_names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for usage in &usages {
        for prop in &usage.props {
            prop_names.insert(prop.name.clone());
        }
    }

    // For each prop name, determine constness across all call sites
    prop_names
        .into_iter()
        .map(|name| {
            let mut all_const = true;
            let mut any_dynamic = false;
            let mut call_site_count = 0;

            for usage in &usages {
                if let Some(prop) = usage.props.iter().find(|p| p.name == name) {
                    call_site_count += 1;
                    match prop.constness {
                        PropValueConstness::Const => {}
                        PropValueConstness::Dynamic => {
                            all_const = false;
                            any_dynamic = true;
                        }
                        PropValueConstness::Unknown => {
                            all_const = false;
                        }
                    }
                }
                // If a usage doesn't pass this prop at all, it's not counted
            }

            let constness = if call_site_count == 0 {
                PropConstness::Unknown
            } else if all_const {
                PropConstness::AlwaysConst
            } else if any_dynamic {
                PropConstness::SometimesDynamic
            } else {
                PropConstness::Unknown
            };

            PropConstnessFact {
                prop_name: name,
                constness,
                call_site_count,
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::input::{
        AnalyzedEmitField, AnalyzedExposeField, AnalyzedPropField, AnalyzedSlotField,
        AnalyzedSlotFieldBinding, TypeResolutionSource,
    };
    use verter_span::Span;

    fn make_snapshot(macros: Vec<AnalyzedMacro>) -> ScriptAnalysisSnapshot {
        let mut snapshot = ScriptAnalysisSnapshot::default();
        snapshot.macros = macros;
        snapshot.is_typescript = true;
        snapshot
    }

    fn make_props_macro(props: Vec<AnalyzedPropField>) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: props,
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 50),
        }
    }

    fn make_prop(name: &str, optional: bool) -> AnalyzedPropField {
        AnalyzedPropField {
            name: name.to_string(),
            is_optional: optional,
            span: Span::new(10, 20),
            type_annotation: Some("string".to_string()),
            description: None,
            tags: Vec::new(),
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
            type_expr: None,
            type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }
    }

    #[test]
    fn empty_analysis_produces_empty_surface() {
        let snapshot = make_snapshot(vec![]);
        let surface = extract_declared_surface(&snapshot);

        assert!(surface.props.is_empty());
        assert!(surface.events.is_empty());
        assert!(surface.slots.is_empty());
        assert!(surface.models.is_empty());
        assert!(surface.expose.is_empty());
    }

    #[test]
    fn extracts_props_from_define_props() {
        let mac = make_props_macro(vec![make_prop("color", true), make_prop("size", false)]);
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        // Positive: both props extracted
        assert_eq!(surface.props.len(), 2);
        assert_eq!(surface.props[0].name, "color");
        assert!(surface.props[0].is_optional);
        assert_eq!(surface.props[1].name, "size");
        assert!(!surface.props[1].is_optional);

        // Negative: no events, slots, models
        assert!(surface.events.is_empty());
        assert!(surface.slots.is_empty());
        assert!(surface.models.is_empty());
    }

    #[test]
    fn extracts_events_from_define_emits() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            emit_fields: vec![
                AnalyzedEmitField {
                    name: "update".to_string(),
                    span: Span::new(10, 20),
                    payload_type: Some("[value: string]".to_string()),
                    description: None,
                    tags: Vec::new(),
                    payload_expr: None,
                    payload_expr_scope: None,
                },
                AnalyzedEmitField {
                    name: "close".to_string(),
                    span: Span::new(30, 40),
                    payload_type: None,
                    description: None,
                    tags: Vec::new(),
                    payload_expr: None,
                    payload_expr_scope: None,
                },
            ],
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        // Positive: both events extracted
        assert_eq!(surface.events.len(), 2);
        assert_eq!(surface.events[0].name, "update");
        assert_eq!(
            surface.events[0].payload_type.as_deref(),
            Some("[value: string]")
        );
        assert_eq!(surface.events[1].name, "close");
        assert!(surface.events[1].payload_type.is_none());

        // Negative: no props
        assert!(surface.props.is_empty());
    }

    #[test]
    fn extracts_model_with_default_name() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineModel,
            model_name: None,
            prop_fields: vec![make_prop("modelValue", false)],
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        assert_eq!(surface.models.len(), 1);
        assert_eq!(surface.models[0].name, "modelValue");
        assert_eq!(surface.models[0].type_text.as_deref(), Some("string"));
    }

    #[test]
    fn extracts_model_with_custom_name() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineModel,
            model_name: Some("checked".to_string()),
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        assert_eq!(surface.models.len(), 1);
        assert_eq!(surface.models[0].name, "checked");
    }

    #[test]
    fn extracts_slots() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            slot_fields: vec![AnalyzedSlotField {
                name: "header".to_string(),
                is_required: true,
                span: Span::new(10, 20),
                bindings: vec![AnalyzedSlotFieldBinding {
                    name: "title".to_string(),
                    type_annotation: Some("string".to_string()),
                    span: Span::new(12, 17),
                    binding_expr: None,
                    binding_expr_scope: None,
                }],
                return_type: None,
                description: None,
                tags: Vec::new(),
                return_expr: None,
                return_expr_scope: None,
            }],
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        assert_eq!(surface.slots.len(), 1);
        assert_eq!(surface.slots[0].name, "header");
        assert!(surface.slots[0].is_required);
        assert_eq!(surface.slots[0].bindings.len(), 1);
        assert_eq!(surface.slots[0].bindings[0].name, "title");
    }

    #[test]
    fn extracts_expose() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineExpose,
            expose_fields: vec![
                AnalyzedExposeField {
                    name: "open".to_string(),
                    span: Span::new(10, 14),
                },
                AnalyzedExposeField {
                    name: "close".to_string(),
                    span: Span::new(16, 21),
                },
            ],
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        assert_eq!(surface.expose.len(), 2);
        assert_eq!(surface.expose[0].name, "open");
        assert_eq!(surface.expose[1].name, "close");
    }

    #[test]
    fn component_surface_sets_inherit_attrs() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineOptions,
            has_inherit_attrs_false: true,
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_component_surface(&snapshot);

        assert!(surface.inherit_attrs_disabled);
    }

    #[test]
    fn component_surface_accepted_equals_declared_without_resolution() {
        let mac = make_props_macro(vec![make_prop("msg", false)]);
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_component_surface(&snapshot);

        // Positive: accepted mirrors declared
        assert_eq!(surface.accepted_props.len(), 1);
        assert_eq!(surface.accepted_props[0].name, "msg");

        // Negative: completeness not yet resolved
        assert!(surface.completeness.is_none());
    }

    #[test]
    fn with_defaults_merges_default_values() {
        use crate::input::AnalyzedDefaultValue;

        let props_mac = make_props_macro(vec![
            make_prop("color", false), // required
            make_prop("size", true),   // optional
        ]);
        let defaults_mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::WithDefaults,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![make_prop("color", false)],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: vec!["color".into()],
            default_values: vec![AnalyzedDefaultValue {
                key: "color".into(),
                value: "'blue'".into(),
                span: Span::new(40, 46),
            }],
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(30, 60),
        };

        let snapshot = make_snapshot(vec![props_mac, defaults_mac]);
        let surface = extract_declared_surface(&snapshot);

        // Positive: color now has default and became optional
        let color = surface.props.iter().find(|p| p.name == "color").unwrap();
        assert_eq!(color.default_value.as_deref(), Some("'blue'"));
        assert!(
            color.is_optional,
            "prop with default should become optional"
        );

        // Negative: size unchanged
        let size = surface.props.iter().find(|p| p.name == "size").unwrap();
        assert!(size.default_value.is_none());
    }

    #[test]
    fn multiple_define_emits_merged() {
        let mac = AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            emit_fields: vec![
                AnalyzedEmitField {
                    name: "change".into(),
                    span: Span::new(10, 16),
                    payload_type: Some("[val: string]".into()),
                    description: None,
                    tags: Vec::new(),
                    payload_expr: None,
                    payload_expr_scope: None,
                },
                AnalyzedEmitField {
                    name: "submit".into(),
                    span: Span::new(20, 26),
                    payload_type: None,
                    description: None,
                    tags: Vec::new(),
                    payload_expr: None,
                    payload_expr_scope: None,
                },
            ],
            ..make_props_macro(vec![])
        };
        let snapshot = make_snapshot(vec![mac]);
        let surface = extract_declared_surface(&snapshot);

        assert_eq!(surface.events.len(), 2);
        assert_eq!(surface.events[0].name, "change");
        assert_eq!(surface.events[1].name, "submit");
        // Negative: no props from emits
        assert!(surface.props.is_empty());
    }

    // ── Binding and reactivity tests ───────────────────────────────────────

    fn make_binding(
        name: &str,
        kind: crate::input::AnalyzedBindingKind,
        reactivity: ReactivityKind,
    ) -> crate::input::AnalyzedBinding {
        crate::input::AnalyzedBinding {
            name: name.to_string(),
            kind,
            is_reactive: !matches!(reactivity, ReactivityKind::None | ReactivityKind::Mutable),
            reactivity_kind: reactivity,
            type_annotation: None,
            initializer: None,
            span: verter_span::Span::new(0, 10),
            used_in_script: false,
            used_in_style: false,
        }
    }

    #[test]
    fn extract_bindings_from_empty_analysis() {
        let snapshot = make_snapshot(vec![]);
        let bindings = extract_bindings(&snapshot);
        assert!(bindings.is_empty());
    }

    #[test]
    fn extract_bindings_classifies_reactivity() {
        use crate::input::AnalyzedBindingKind;

        let mut snapshot = make_snapshot(vec![]);
        snapshot.bindings = vec![
            make_binding("count", AnalyzedBindingKind::Const, ReactivityKind::Ref),
            make_binding(
                "state",
                AnalyzedBindingKind::Const,
                ReactivityKind::Reactive,
            ),
            make_binding(
                "doubled",
                AnalyzedBindingKind::Const,
                ReactivityKind::Computed,
            ),
            make_binding("name", AnalyzedBindingKind::Const, ReactivityKind::None),
            make_binding("idx", AnalyzedBindingKind::Let, ReactivityKind::Mutable),
            make_binding(
                "result",
                AnalyzedBindingKind::Const,
                ReactivityKind::MaybeRef,
            ),
        ];

        let bindings = extract_bindings(&snapshot);
        assert_eq!(bindings.len(), 6);

        // Positive: ref → Reactive with Ref source
        assert_eq!(bindings[0].0.name, "count");
        assert_eq!(bindings[0].1.status, ReactivityStatus::Reactive);
        assert_eq!(bindings[0].1.source, Some(ReactivitySource::Ref));

        // Positive: reactive → Reactive with Reactive source
        assert_eq!(bindings[1].0.name, "state");
        assert_eq!(bindings[1].1.status, ReactivityStatus::Reactive);
        assert_eq!(bindings[1].1.source, Some(ReactivitySource::Reactive));

        // Positive: computed → Reactive with Computed source
        assert_eq!(bindings[2].1.status, ReactivityStatus::Reactive);
        assert_eq!(bindings[2].1.source, Some(ReactivitySource::Computed));

        // Positive: plain const → NonReactive
        assert_eq!(bindings[3].1.status, ReactivityStatus::NonReactive);
        assert!(bindings[3].1.source.is_none());

        // Positive: let → NonReactive (mutable but not reactive)
        assert_eq!(bindings[4].0.kind, BindingKind::Let);
        assert_eq!(bindings[4].1.status, ReactivityStatus::NonReactive);

        // Positive: composable → MaybeReactive
        assert_eq!(bindings[5].1.status, ReactivityStatus::MaybeReactive);
    }

    #[test]
    fn extract_bindings_tracks_usage_flags() {
        use crate::input::AnalyzedBindingKind;

        let mut snapshot = make_snapshot(vec![]);
        let mut binding = make_binding("count", AnalyzedBindingKind::Const, ReactivityKind::Ref);
        binding.used_in_script = true;
        binding.used_in_style = true;
        snapshot.bindings = vec![binding];

        let bindings = extract_bindings(&snapshot);
        let (decl, _) = &bindings[0];

        // Positive: both usages tracked
        assert_eq!(decl.usages.len(), 2);
        assert_eq!(decl.usages[0].block, UsageBlock::Script);
        assert_eq!(decl.usages[1].block, UsageBlock::Style);
        assert_eq!(decl.usages[1].kind, UsageKind::StyleVBind);
    }

    #[test]
    fn extract_bindings_enriches_source_from_initializer() {
        use crate::input::{AnalyzedBindingKind, BindingInitializer, VueApiClassification};

        let mut snapshot = make_snapshot(vec![]);
        let mut binding = make_binding("count", AnalyzedBindingKind::Const, ReactivityKind::None);
        binding.initializer = Some(BindingInitializer::FunctionCall {
            callee: "ref".to_string(),
            callee_import_source: Some("vue".to_string()),
            vue_api: Some(VueApiClassification::Ref),
        });
        snapshot.bindings = vec![binding];

        let bindings = extract_bindings(&snapshot);

        // Positive: source enriched from initializer even though ReactivityKind is None
        assert_eq!(bindings[0].1.source, Some(ReactivitySource::Ref));
    }

    // ── Import graph extraction tests ──────────────────────────────────────

    #[test]
    fn extract_import_graph_empty() {
        let snapshot = make_snapshot(vec![]);
        let graph = extract_import_graph(&snapshot);
        assert!(graph.imports.is_empty());
        assert!(graph.import_sources.is_empty());
        assert!(!graph.has_unresolved());
    }

    #[test]
    fn extract_import_graph_named_imports() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        snapshot.imports = vec![AnalyzedImport {
            source: "./types".to_string(),
            is_type_only: false,
            bindings: vec![
                AnalyzedImportBinding {
                    name: "Foo".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: true,
                    vue_api: None,
                    span: Span::new(10, 13),
                },
                AnalyzedImportBinding {
                    name: "bar".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: Some("originalBar".to_string()),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(15, 18),
                },
            ],
            span: Span::new(0, 30),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
        }];

        let graph = extract_import_graph(&snapshot);

        // Positive: both symbols extracted
        assert_eq!(graph.imports.len(), 2);
        assert_eq!(graph.imports[0].local_name, "Foo");
        assert_eq!(graph.imports[0].exported_name, "Foo"); // no rename
        assert!(graph.imports[0].is_type_only);
        assert_eq!(graph.imports[0].kind, ImportKind::Named);

        // Positive: renamed import preserves original name
        assert_eq!(graph.imports[1].local_name, "bar");
        assert_eq!(graph.imports[1].exported_name, "originalBar");
        assert!(!graph.imports[1].is_type_only);

        // Positive: source tracked
        assert_eq!(graph.import_sources.len(), 1);
        assert!(graph.import_sources.contains(&"/src/types.ts".to_string()));

        // Negative: no unresolved
        assert!(!graph.has_unresolved());
    }

    #[test]
    fn extract_import_graph_default_import() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        snapshot.imports = vec![AnalyzedImport {
            source: "./App.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "App".to_string(),
                kind: ImportBindingKind::Default,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(7, 10),
            }],
            span: Span::new(0, 30),
            resolved_canonical_id: Some("/src/App.vue".to_string()),
        }];

        let graph = extract_import_graph(&snapshot);

        assert_eq!(graph.imports.len(), 1);
        assert_eq!(graph.imports[0].kind, ImportKind::Default);
        assert_eq!(graph.imports[0].exported_name, "default");
        assert_eq!(
            graph.imports[0].resolved_file_id.as_deref(),
            Some("/src/App.vue")
        );
    }

    #[test]
    fn extract_import_graph_unresolved_source() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        snapshot.imports = vec![AnalyzedImport {
            source: "external-pkg".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ext".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(10, 13),
            }],
            span: Span::new(0, 30),
            resolved_canonical_id: None,
        }];

        let graph = extract_import_graph(&snapshot);

        // Positive: symbol extracted
        assert_eq!(graph.imports.len(), 1);
        assert!(graph.imports[0].resolved_file_id.is_none());

        // Positive: unresolved detected
        assert!(graph.has_unresolved());

        // Negative: no sources in set since none resolved
        assert!(graph.import_sources.is_empty());
    }

    #[test]
    fn extract_import_graph_namespace_import() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        snapshot.imports = vec![AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Vue".to_string(),
                kind: ImportBindingKind::Namespace,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(7, 10),
            }],
            span: Span::new(0, 25),
            resolved_canonical_id: None,
        }];

        let graph = extract_import_graph(&snapshot);

        assert_eq!(graph.imports.len(), 1);
        assert_eq!(graph.imports[0].kind, ImportKind::Namespace);
        // Namespace: exported_name is the local name (no "default")
        assert_eq!(graph.imports[0].exported_name, "Vue");
    }

    #[test]
    fn extract_import_graph_type_only_propagation() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        // Declaration-level type-only: `import type { Foo } from "..."`
        snapshot.imports = vec![AnalyzedImport {
            source: "./types".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "Props".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false, // specifier not type-only, but declaration is
                vue_api: None,
                span: Span::new(14, 19),
            }],
            span: Span::new(0, 35),
            resolved_canonical_id: Some("/src/types.ts".into()),
        }];

        let graph = extract_import_graph(&snapshot);

        // Positive: declaration-level type-only propagates to symbol
        assert!(graph.imports[0].is_type_only);
    }

    #[test]
    fn extract_import_graph_specifier_level_type_only() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        // `import { type Foo, bar } from "..."`
        snapshot.imports = vec![AnalyzedImport {
            source: "./mixed".to_string(),
            is_type_only: false, // declaration NOT type-only
            bindings: vec![
                AnalyzedImportBinding {
                    name: "Foo".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: true, // specifier IS type-only
                    vue_api: None,
                    span: Span::new(14, 17),
                },
                AnalyzedImportBinding {
                    name: "bar".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(19, 22),
                },
            ],
            span: Span::new(0, 40),
            resolved_canonical_id: Some("/src/mixed.ts".into()),
        }];

        let graph = extract_import_graph(&snapshot);

        assert_eq!(graph.imports.len(), 2);
        // Positive: Foo is type-only
        assert!(graph.imports[0].is_type_only);
        // Negative: bar is NOT type-only
        assert!(!graph.imports[1].is_type_only);
    }

    #[test]
    fn extract_import_graph_multiple_sources() {
        use crate::input::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};

        let mut snapshot = make_snapshot(vec![]);
        snapshot.imports = vec![
            AnalyzedImport {
                source: "./a".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "A".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(9, 10),
                }],
                span: Span::new(0, 20),
                resolved_canonical_id: Some("/src/a.ts".into()),
            },
            AnalyzedImport {
                source: "./b".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "B".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(30, 31),
                }],
                span: Span::new(25, 45),
                resolved_canonical_id: Some("/src/b.ts".into()),
            },
        ];

        let graph = extract_import_graph(&snapshot);

        assert_eq!(graph.imports.len(), 2);
        assert_eq!(graph.import_sources.len(), 2);
        assert!(graph.import_sources.contains(&"/src/a.ts".to_string()));
        assert!(graph.import_sources.contains(&"/src/b.ts".to_string()));
    }

    // ── Prop constness tests ───────────────────────────────────────────────

    fn make_template_with_component_usage(
        usages: Vec<crate::input::TemplateComponentUsage>,
    ) -> TemplateAnalysisSnapshot {
        let mut template = TemplateAnalysisSnapshot::default();
        template.components = usages;
        template
    }

    fn make_component_usage(
        name: &str,
        props: Vec<crate::input::TemplatePropUsage>,
    ) -> crate::input::TemplateComponentUsage {
        crate::input::TemplateComponentUsage {
            name: name.to_string(),
            import_source: None,
            is_dynamic: false,
            props,
            has_spread: false,
            slots_used: Vec::new(),
            static_classes: Vec::new(),
            has_dynamic_class: false,
            dynamic_classes: Vec::new(),
            v_models: Vec::new(),
            span: Span::new(0, 50),
        }
    }

    fn make_prop_usage(
        name: &str,
        constness: PropValueConstness,
    ) -> crate::input::TemplatePropUsage {
        crate::input::TemplatePropUsage {
            name: name.to_string(),
            is_bound: constness != PropValueConstness::Const,
            expression: None,
            constness,
            referenced_bindings: Vec::new(),
            from_spread: false,
            span: Span::new(10, 20),
            name_span: Span::new(10, 15),
            is_shorthand: false,
        }
    }

    #[test]
    fn prop_constness_empty_template() {
        let template = TemplateAnalysisSnapshot::default();
        let facts = compute_prop_constness_for_child("Button", &template);
        assert!(facts.is_empty());
    }

    #[test]
    fn prop_constness_single_const_usage() {
        let template = make_template_with_component_usage(vec![make_component_usage(
            "Button",
            vec![make_prop_usage("color", PropValueConstness::Const)],
        )]);

        let facts = compute_prop_constness_for_child("Button", &template);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].prop_name, "color");
        assert_eq!(facts[0].constness, PropConstness::AlwaysConst);
        assert_eq!(facts[0].call_site_count, 1);
    }

    #[test]
    fn prop_constness_mixed_across_sites() {
        let template = make_template_with_component_usage(vec![
            make_component_usage(
                "Button",
                vec![make_prop_usage("color", PropValueConstness::Const)],
            ),
            make_component_usage(
                "Button",
                vec![make_prop_usage("color", PropValueConstness::Dynamic)],
            ),
        ]);

        let facts = compute_prop_constness_for_child("Button", &template);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].prop_name, "color");
        // Positive: mixed → SometimesDynamic
        assert_eq!(facts[0].constness, PropConstness::SometimesDynamic);
        assert_eq!(facts[0].call_site_count, 2);
    }

    #[test]
    fn prop_constness_different_component_not_counted() {
        let template = make_template_with_component_usage(vec![
            make_component_usage(
                "Button",
                vec![make_prop_usage("color", PropValueConstness::Const)],
            ),
            make_component_usage(
                "Input",
                vec![make_prop_usage("color", PropValueConstness::Dynamic)],
            ),
        ]);

        let facts = compute_prop_constness_for_child("Button", &template);
        assert_eq!(facts.len(), 1);
        // Positive: only Button usage counted, which is const
        assert_eq!(facts[0].constness, PropConstness::AlwaysConst);

        // Negative: Input's dynamic usage doesn't affect Button
        assert_eq!(facts[0].call_site_count, 1);
    }

    // ── Boundary extraction tests ──────────────────────────────────────────

    #[test]
    fn extract_boundary_edges_empty_template() {
        let template = TemplateAnalysisSnapshot::default();
        let graph = FileImportGraph::default();
        let edges = extract_boundary_edges("/src/App.vue", &template, &graph);
        assert!(edges.is_empty());
    }

    #[test]
    fn extract_boundary_edges_resolves_child_from_imports() {
        use crate::facts::symbol::{ImportKind, ImportedSymbol};

        let template = make_template_with_component_usage(vec![make_component_usage(
            "Button",
            vec![
                make_prop_usage("color", PropValueConstness::Const),
                make_prop_usage("size", PropValueConstness::Dynamic),
            ],
        )]);

        let graph = FileImportGraph {
            imports: vec![ImportedSymbol {
                local_name: "Button".into(),
                source_specifier: "./Button.vue".into(),
                resolved_file_id: Some("/src/Button.vue".into()),
                exported_name: "default".into(),
                kind: ImportKind::Default,
                is_type_only: false,
                span: Span::new(7, 13),
            }],
            import_sources: vec!["/src/Button.vue".into()],
        };

        let edges = extract_boundary_edges("/src/App.vue", &template, &graph);

        // Positive: one edge extracted
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].tag_name, "Button");
        assert_eq!(edges[0].parent_file_id, "/src/App.vue");
        assert_eq!(edges[0].child_file_id.as_deref(), Some("/src/Button.vue"));
        assert_eq!(edges[0].passed_props, vec!["color", "size"]);

        // Negative: no spread
        assert!(!edges[0].has_spread);
    }

    #[test]
    fn extract_boundary_edges_unresolved_component() {
        let template =
            make_template_with_component_usage(vec![make_component_usage("External", vec![])]);
        let graph = FileImportGraph::default(); // no imports

        let edges = extract_boundary_edges("/src/App.vue", &template, &graph);

        // Positive: edge created
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].tag_name, "External");

        // Positive: child is None (unresolved)
        assert!(edges[0].child_file_id.is_none());
    }

    #[test]
    fn extract_boundary_edges_v_model_creates_update_event() {
        use crate::input::TemplateComponentVModel;

        let mut usage = make_component_usage("Input", vec![]);
        usage.v_models = vec![TemplateComponentVModel {
            binding_name: "modelValue".into(),
            span: Span::new(20, 40),
        }];

        let template = make_template_with_component_usage(vec![usage]);
        let graph = FileImportGraph::default();

        let edges = extract_boundary_edges("/src/Form.vue", &template, &graph);

        // Positive: v-model generates update event
        assert_eq!(edges[0].passed_events, vec!["update:modelValue"]);
    }
}
