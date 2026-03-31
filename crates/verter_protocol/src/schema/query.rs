//! Query result envelope DTO for protocol boundaries.

use serde::{Deserialize, Serialize};

/// Completeness classification for protocol responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletenessDto {
    Complete,
    Partial,
    Unavailable,
}

/// Revision marker DTO for protocol responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionMarkerDto {
    pub workspace_revision: u64,
    pub parser_revision: u64,
    pub compiler_revision: u64,
    pub provider_revision: u64,
}

/// Generic query result envelope for all protocol responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultDto<T> {
    pub value: T,
    pub revision: RevisionMarkerDto,
    pub completeness: CompletenessDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_inputs: Vec<String>,
    #[serde(default)]
    pub stale_ref: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_result_dto_camel_case() {
        let dto = QueryResultDto {
            value: 42,
            revision: RevisionMarkerDto {
                workspace_revision: 1,
                parser_revision: 2,
                compiler_revision: 0,
                provider_revision: 0,
            },
            completeness: CompletenessDto::Complete,
            missing_inputs: vec![],
            stale_ref: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"workspaceRevision\""));
        assert!(json.contains("\"parserRevision\""));
        assert!(json.contains("\"staleRef\""));
        assert!(!json.contains("\"missingInputs\"")); // skip_serializing_if empty
    }

    #[test]
    fn partial_result_includes_missing() {
        let dto = QueryResultDto {
            value: Option::<i32>::None,
            revision: RevisionMarkerDto {
                workspace_revision: 1,
                parser_revision: 0,
                compiler_revision: 0,
                provider_revision: 0,
            },
            completeness: CompletenessDto::Partial,
            missing_inputs: vec!["provider:tsgo".into()],
            stale_ref: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"missingInputs\""));
        assert!(json.contains("provider:tsgo"));
    }
}
