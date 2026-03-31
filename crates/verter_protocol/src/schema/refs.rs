//! Stable external reference DTOs for protocol boundaries.
//!
//! These mirror verter_semantic::refs but are the canonical serialized form
//! for all external consumers. Internal interned IDs never cross this boundary.

use serde::{Deserialize, Serialize};

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
    pub span_start: u32,
    pub span_end: u32,
}

/// Component reference DTO.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRefDto {
    pub file_id: String,
    pub component_key: String,
    pub export_name: String,
    pub span_start: u32,
    pub span_end: u32,
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
            span_start: 10,
            span_end: 15,
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
            span_start: 0,
            span_end: 100,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: ComponentRefDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
        assert!(json.contains("\"componentKey\""));
        assert!(json.contains("\"exportName\""));
    }
}
