//! Stable external reference DTOs for protocol boundaries.
//!
//! These mirror verter_semantic::refs but are the canonical serialized form
//! for all external consumers. Internal interned IDs never cross this boundary.

use serde::{Deserialize, Serialize};

use crate::types::TaggedSpan;

/// File reference DTO.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRefDto {
    pub file_id: String,
}

/// Binding reference DTO.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRefDto {
    pub file_id: String,
    pub binding_key: String,
    pub name: String,
    /// Range in the owning file, tagged with its encoding (`utf8.v1`).
    pub span: TaggedSpan,
}

/// Component reference DTO.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRefDto {
    pub file_id: String,
    pub component_key: String,
    pub export_name: String,
    /// Range in the owning file, tagged with its encoding (`utf8.v1`).
    pub span: TaggedSpan,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_ref_dto_camel_case() {
        let dto = FileRefDto {
            file_id: "app.vue".into(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"fileId\""));
        assert!(!json.contains("\"file_id\""));
    }

    #[test]
    fn binding_ref_dto_round_trips() {
        let dto = BindingRefDto {
            file_id: "app.vue".into(),
            binding_key: "setup_0".into(),
            name: "count".into(),
            span: TaggedSpan::utf8(verter_span::Span::new(10, 15)).unwrap(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: BindingRefDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    #[test]
    fn component_ref_dto_round_trip() {
        let dto = ComponentRefDto {
            file_id: "button.vue".into(),
            component_key: "default".into(),
            export_name: "default".into(),
            span: TaggedSpan::utf8(verter_span::Span::new(0, 100)).unwrap(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: ComponentRefDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
        assert!(json.contains("\"componentKey\""));
        assert!(json.contains("\"exportName\""));
    }

    #[test]
    fn file_ref_dto_round_trip() {
        let dto = FileRefDto {
            file_id: "/src/App.vue".into(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: FileRefDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    #[test]
    fn binding_ref_dto_camel_case() {
        let dto = BindingRefDto {
            file_id: "f.vue".into(),
            binding_key: "k".into(),
            name: "x".into(),
            span: TaggedSpan::utf8(verter_span::Span::new(0, 1)).unwrap(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"bindingKey\""));
        // A tagged range, never a bare unit-less pair.
        assert!(json.contains("\"span\":{\"start\":0,\"end\":1,\"encoding\":\"utf8.v1\"}"));
        assert!(!json.contains("\"spanStart\""));
        // Negative: no snake_case
        assert!(!json.contains("\"binding_key\""));
        assert!(!json.contains("\"span_start\""));
    }
}
