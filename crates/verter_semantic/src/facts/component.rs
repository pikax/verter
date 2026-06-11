//! Component surface facts — declared and accepted props, events, slots,
//! models, and exposed members.
//!
//! These facts represent the component's public API surface as understood
//! by the semantic engine. They are derived from script analysis macros
//! (defineProps, defineEmits, defineSlots, defineModel, defineExpose) and
//! cross-file type resolution.

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// The declared surface of a component — what it explicitly defines.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeclaredSurface {
    pub props: Vec<PropFact>,
    pub events: Vec<EventFact>,
    pub slots: Vec<SlotFact>,
    pub models: Vec<ModelFact>,
    pub expose: Vec<ExposeFact>,
}

/// A single declared prop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropFact {
    pub name: String,
    pub is_optional: bool,
    pub type_text: Option<String>,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub span: Span,
}

/// A single declared event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFact {
    pub name: String,
    pub payload_type: Option<String>,
    pub description: Option<String>,
    pub span: Span,
}

/// A single declared slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotFact {
    pub name: String,
    pub is_required: bool,
    pub bindings: Vec<SlotBindingFact>,
    pub description: Option<String>,
    pub span: Span,
}

/// A binding property on a slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotBindingFact {
    pub name: String,
    pub type_text: Option<String>,
}

/// A declared v-model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFact {
    pub name: String,
    pub type_text: Option<String>,
    pub span: Span,
}

/// An exposed member from defineExpose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposeFact {
    pub name: String,
    pub span: Span,
}

/// How complete the accepted surface computation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceCompleteness {
    /// All inheritance branches were fully resolved.
    Exact,
    /// Some branches could not be resolved (partial/unresolved children).
    LowerBound,
}

/// The full component surface including inherited (fallthrough) surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentSurface {
    /// What the component explicitly declares.
    pub declared: DeclaredSurface,
    /// Computed accepted props (declared + inherited via fallthrough).
    pub accepted_props: Vec<PropFact>,
    /// Computed accepted events (declared + inherited via fallthrough).
    pub accepted_events: Vec<EventFact>,
    /// How complete the accepted surface is.
    pub completeness: Option<SurfaceCompleteness>,
    /// Whether `inheritAttrs: false` is set.
    pub inherit_attrs_disabled: bool,
}

/// Cross-file prop constness classification.
///
/// Indicates whether a prop is always passed as a compile-time constant
/// across all call sites of a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropConstness {
    /// All call sites pass a constant value for this prop.
    AlwaysConst,
    /// At least one call site passes a dynamic/reactive value.
    SometimesDynamic,
    /// Not enough data to determine (no call sites found or unanalyzed).
    Unknown,
}

/// Per-prop constness fact for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropConstnessFact {
    pub prop_name: String,
    pub constness: PropConstness,
    /// Number of call sites analyzed.
    pub call_site_count: usize,
}

/// Structural classification of the template root for fallthrough analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RootReachability {
    /// Single native HTML element root.
    SingleNative,
    /// Single component root.
    SingleComponent,
    /// Conditional branches (v-if/v-else).
    Conditional,
    /// Fragment (multiple roots).
    Fragment,
    /// Root is v-for.
    VFor,
    /// Dynamic component (:is).
    DynamicComponent,
    /// No template content.
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_surface_default_is_empty() {
        let s = DeclaredSurface::default();
        assert!(s.props.is_empty());
        assert!(s.events.is_empty());
        assert!(s.slots.is_empty());
        assert!(s.models.is_empty());
        assert!(s.expose.is_empty());
    }

    #[test]
    fn component_surface_carries_completeness() {
        let mut surface = ComponentSurface::default();
        assert!(surface.completeness.is_none());

        surface.completeness = Some(SurfaceCompleteness::Exact);
        assert_eq!(surface.completeness, Some(SurfaceCompleteness::Exact));
    }

    #[test]
    fn prop_fact_serializes() {
        let prop = PropFact {
            name: "color".into(),
            is_optional: true,
            type_text: Some("'primary' | 'secondary'".into()),
            default_value: Some("'primary'".into()),
            description: None,
            span: Span::new(10, 50),
        };
        let json = serde_json::to_string(&prop).unwrap();
        assert!(json.contains("color"));
        assert!(json.contains("primary"));
        let back: PropFact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "color");
        assert!(back.is_optional);
    }

    #[test]
    fn root_reachability_variants() {
        let variants = [
            RootReachability::SingleNative,
            RootReachability::SingleComponent,
            RootReachability::Conditional,
            RootReachability::Fragment,
            RootReachability::VFor,
            RootReachability::DynamicComponent,
            RootReachability::Empty,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn component_surface_serializes() {
        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "msg".into(),
            is_optional: false,
            type_text: Some("string".into()),
            default_value: None,
            description: Some("The message".into()),
            span: Span::new(10, 20),
        });
        surface.declared.events.push(EventFact {
            name: "click".into(),
            payload_type: Some("[e: MouseEvent]".into()),
            description: None,
            span: Span::new(30, 40),
        });
        surface.inherit_attrs_disabled = true;

        let json = serde_json::to_string(&surface).unwrap();
        let back: ComponentSurface = serde_json::from_str(&json).unwrap();
        assert_eq!(back.declared.props[0].name, "msg");
        assert_eq!(back.declared.events[0].name, "click");
        assert!(back.inherit_attrs_disabled);
    }

    #[test]
    fn prop_constness_variants() {
        assert_ne!(PropConstness::AlwaysConst, PropConstness::SometimesDynamic);
        assert_ne!(PropConstness::SometimesDynamic, PropConstness::Unknown);
        assert_ne!(PropConstness::AlwaysConst, PropConstness::Unknown);
    }

    #[test]
    fn slot_fact_with_bindings() {
        let slot = SlotFact {
            name: "header".into(),
            is_required: true,
            bindings: vec![
                SlotBindingFact {
                    name: "title".into(),
                    type_text: Some("string".into()),
                },
                SlotBindingFact {
                    name: "subtitle".into(),
                    type_text: None,
                },
            ],
            description: Some("Header slot".into()),
            span: Span::new(50, 60),
        };
        assert_eq!(slot.bindings.len(), 2);
        assert!(slot.is_required);
        assert!(slot.bindings[1].type_text.is_none());
    }

    #[test]
    fn model_fact_serializes() {
        let model = ModelFact {
            name: "modelValue".into(),
            type_text: Some("string".into()),
            span: Span::new(0, 20),
        };
        let json = serde_json::to_string(&model).unwrap();
        let back: ModelFact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "modelValue");
    }

    #[test]
    fn surface_completeness_variants() {
        assert_ne!(SurfaceCompleteness::Exact, SurfaceCompleteness::LowerBound);
    }
}
