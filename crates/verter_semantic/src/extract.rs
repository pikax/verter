//! Extraction of semantic facts from analysis snapshots.
//!
//! Converts `verter_analysis` types into `verter_semantic` fact types.
//! This is the bridge between the raw analysis layer and the semantic DB.

use verter_analysis::types::{AnalyzedMacro, AnalyzedMacroKind};
use verter_analysis::ScriptAnalysisSnapshot;
use verter_span::Span;

use crate::facts::component::{
    ComponentSurface, DeclaredSurface, EventFact, ExposeFact, ModelFact, PropFact, SlotBindingFact,
    SlotFact,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::types::{
        AnalyzedEmitField, AnalyzedExposeField, AnalyzedPropField, AnalyzedSlotField,
        AnalyzedSlotFieldBinding, TypeResolutionSource,
    };

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
                },
                AnalyzedEmitField {
                    name: "close".to_string(),
                    span: Span::new(30, 40),
                    payload_type: None,
                    description: None,
                    tags: Vec::new(),
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
                }],
                return_type: None,
                description: None,
                tags: Vec::new(),
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
}
