//! Runtime schema facts for semantic-assisted codegen.
//!
//! A [`ComponentRuntimeSchema`] is a target-neutral description of a component's
//! runtime shape — props with types, models, events, slots — suitable for
//! generating runtime validators (Zod, io-ts, etc.) or documentation.
//!
//! The schema is derived from the component surface and type information.
//! Compiler/codegen modules consume it as an input (e.g., `CompileSemanticHints`)
//! without depending on `verter_semantic` directly.

use serde::{Deserialize, Serialize};

use crate::facts::component::ComponentSurface;

/// Target-neutral runtime schema for a component.
///
/// Describes the component's runtime contract in a form suitable for
/// code generation (validator emission, documentation, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentRuntimeSchema {
    /// Props with type information for validator generation.
    pub props: Vec<RuntimePropSchema>,
    /// Models (v-model bindings).
    pub models: Vec<RuntimeModelSchema>,
    /// Events with payload types.
    pub events: Vec<RuntimeEventSchema>,
    /// Slots with binding types.
    pub slots: Vec<RuntimeSlotSchema>,
}

/// Runtime prop schema entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePropSchema {
    pub name: String,
    pub required: bool,
    pub type_text: Option<String>,
    pub default_value: Option<String>,
}

/// Runtime model schema entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeModelSchema {
    pub name: String,
    pub type_text: Option<String>,
}

/// Runtime event schema entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEventSchema {
    pub name: String,
    pub payload_type: Option<String>,
}

/// Runtime slot schema entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSlotSchema {
    pub name: String,
    pub required: bool,
}

/// Extract a runtime schema from a component surface.
pub fn extract_runtime_schema(surface: &ComponentSurface) -> ComponentRuntimeSchema {
    ComponentRuntimeSchema {
        props: surface
            .declared
            .props
            .iter()
            .map(|p| RuntimePropSchema {
                name: p.name.clone(),
                required: !p.is_optional && p.default_value.is_none(),
                type_text: p.type_text.clone(),
                default_value: p.default_value.clone(),
            })
            .collect(),
        models: surface
            .declared
            .models
            .iter()
            .map(|m| RuntimeModelSchema {
                name: m.name.clone(),
                type_text: m.type_text.clone(),
            })
            .collect(),
        events: surface
            .declared
            .events
            .iter()
            .map(|e| RuntimeEventSchema {
                name: e.name.clone(),
                payload_type: e.payload_type.clone(),
            })
            .collect(),
        slots: surface
            .declared
            .slots
            .iter()
            .map(|s| RuntimeSlotSchema {
                name: s.name.clone(),
                required: s.is_required,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::component::*;
    use verter_span::Span;

    #[test]
    fn empty_surface_produces_empty_schema() {
        let surface = ComponentSurface::default();
        let schema = extract_runtime_schema(&surface);
        assert!(schema.props.is_empty());
        assert!(schema.models.is_empty());
        assert!(schema.events.is_empty());
        assert!(schema.slots.is_empty());
    }

    #[test]
    fn props_extracted_with_required_flag() {
        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "color".into(),
            is_optional: false,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(0, 10),
        });
        surface.declared.props.push(PropFact {
            name: "size".into(),
            is_optional: true,
            type_text: Some("'sm' | 'lg'".into()),
            default_value: Some("'sm'".into()),
            description: None,
            span: Span::new(10, 20),
        });

        let schema = extract_runtime_schema(&surface);

        assert_eq!(schema.props.len(), 2);
        assert!(schema.props[0].required);
        assert_eq!(schema.props[0].type_text.as_deref(), Some("string"));
        assert!(!schema.props[1].required);
        assert_eq!(schema.props[1].default_value.as_deref(), Some("'sm'"));
    }

    #[test]
    fn models_and_events_extracted() {
        let mut surface = ComponentSurface::default();
        surface.declared.models.push(ModelFact {
            name: "modelValue".into(),
            type_text: Some("string".into()),
            span: Span::new(0, 10),
        });
        surface.declared.events.push(EventFact {
            name: "submit".into(),
            payload_type: Some("[data: FormData]".into()),
            description: None,
            span: Span::new(20, 30),
        });

        let schema = extract_runtime_schema(&surface);

        assert_eq!(schema.models.len(), 1);
        assert_eq!(schema.models[0].name, "modelValue");
        assert_eq!(schema.events.len(), 1);
        assert_eq!(
            schema.events[0].payload_type.as_deref(),
            Some("[data: FormData]")
        );
    }

    #[test]
    fn schema_serializes() {
        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "title".into(),
            is_optional: false,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(0, 5),
        });
        let schema = extract_runtime_schema(&surface);
        let json = serde_json::to_string(&schema).unwrap();
        let back: ComponentRuntimeSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(back.props[0].name, "title");
        assert!(back.props[0].required);
    }
}
