//! Component metadata schema DTOs for protocol boundaries.
//!
//! These are the canonical transport-facing component metadata types.
//! They replace the `FfiComponentMeta` family in verter_ffi for new consumers.

use serde::{Deserialize, Serialize};

/// Component surface DTO for protocol responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSurfaceDto {
    pub props: Vec<PropDto>,
    pub events: Vec<EventDto>,
    pub slots: Vec<SlotDto>,
    pub models: Vec<ModelDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose: Vec<ExposeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness: Option<String>,
    #[serde(default)]
    pub inherit_attrs_disabled: bool,
}

/// Prop DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropDto {
    pub name: String,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub span_start: u32,
    pub span_end: u32,
}

/// Event DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub span_start: u32,
    pub span_end: u32,
}

/// Slot DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotDto {
    pub name: String,
    #[serde(default)]
    pub is_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SlotBindingDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub span_start: u32,
    pub span_end: u32,
}

/// Slot binding DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotBindingDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
}

/// v-model DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    pub span_start: u32,
    pub span_end: u32,
}

/// Expose DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposeDto {
    pub name: String,
    pub span_start: u32,
    pub span_end: u32,
}

/// Boundary issue DTO for diagnostic output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryIssueDto {
    pub kind: String,
    pub component_name: String,
    pub member_name: String,
    pub span_start: u32,
    pub span_end: u32,
}

/// Reactivity fact DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactivityDto {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<ProvenanceStepDto>,
}

/// Provenance trace step DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceStepDto {
    pub kind: String,
    pub description: String,
    pub span_start: u32,
    pub span_end: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_surface_dto_camel_case() {
        let dto = ComponentSurfaceDto {
            props: vec![PropDto {
                name: "color".into(),
                is_optional: true,
                type_text: Some("string".into()),
                default_value: Some("'blue'".into()),
                description: None,
                span_start: 10,
                span_end: 50,
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"isOptional\""));
        assert!(json.contains("\"typeText\""));
        assert!(json.contains("\"defaultValue\""));
        assert!(json.contains("\"spanStart\""));
        // Negative: no snake_case
        assert!(!json.contains("\"is_optional\""));
        assert!(!json.contains("\"type_text\""));
    }

    #[test]
    fn empty_surface_minimal_json() {
        let dto = ComponentSurfaceDto::default();
        let json = serde_json::to_string(&dto).unwrap();
        // Positive: minimal — empty arrays present, optional fields skipped
        assert!(json.contains("\"props\":[]"));
        assert!(!json.contains("\"expose\"")); // skip_serializing_if empty
        assert!(!json.contains("\"completeness\""));
    }

    #[test]
    fn boundary_issue_dto_round_trips() {
        let dto = BoundaryIssueDto {
            kind: "unknownProp".into(),
            component_name: "Button".into(),
            member_name: "unknown".into(),
            span_start: 100,
            span_end: 107,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: BoundaryIssueDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "unknownProp");
        assert_eq!(back.member_name, "unknown");
    }

    #[test]
    fn reactivity_dto_skips_empty_trace() {
        let dto = ReactivityDto {
            status: "reactive".into(),
            source: Some("ref".into()),
            trace: vec![],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("\"trace\""));
    }
}
