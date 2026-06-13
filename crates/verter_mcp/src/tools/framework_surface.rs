//! Framework-surface tool projection.
//!
//! Thin adapter over the host's `resolve_framework_surface_with_audit`
//! entry — the single validation-first framework-surface executor. The
//! tool builds the wire `TypeInfoGraphRequest` envelope for a component
//! file, calls the host method, and projects the typed
//! `FrameworkSurfacePayload` (or the typed error arm) into a stable JSON
//! shape for the agent surface.
//!
//! This is NOT a second resolver: it constructs no semantics of its own,
//! it only encodes the request and projects the host's typed response.

use verter_protocol::typeinfo::graph::{
    self as wire, FrameworkSurfaceKind, FrameworkSurfaceKindEntry, FrameworkSurfaceKindSupport,
    FrameworkSurfacePayload, FrameworkTag, TypeInfoGraphRequest, TypeInfoGraphResponse,
    TypeInfoRequestError, TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use verter_protocol::verter::v1::{
    graph_closure_policy, type_info_graph_request, type_info_graph_response,
};

/// Build a well-formed framework-surface request envelope for a default
/// export component at `canonical`, selected by `adapter_id`.
///
/// The framework-surface operation rides the existing graph envelope; the
/// executor's requested set is always the full kind set (the request
/// carries no requested-kind field), so the envelope only names the
/// component selector.
pub fn build_request(canonical: &str, adapter_id: &str) -> TypeInfoGraphRequest {
    TypeInfoGraphRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(type_info_graph_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: adapter_id.to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Navigate as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(graph_closure_policy::Kind::OneLevel(
                        wire::ClosureOneLevel {},
                    )),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: true,
                include_projection: vec![],
                schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
            },
        )),
    }
}

/// Project a host `TypeInfoGraphResponse` into the tool JSON shape:
/// either the `framework_surface` arm (framework + per-kind surfaces) or
/// the typed `error` arm.
pub fn project_response(response: &TypeInfoGraphResponse) -> serde_json::Value {
    match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(payload)) => project_payload(payload),
        Some(type_info_graph_response::Kind::Error(error)) => project_error(error),
        // The `graph` arm is never produced by the framework-surface
        // operation; surface it explicitly rather than silently dropping.
        Some(type_info_graph_response::Kind::Graph(_)) => serde_json::json!({
            "error": "unexpected graph arm for a framework-surface request",
        }),
        None => serde_json::json!({ "error": "empty response" }),
    }
}

fn project_payload(payload: &FrameworkSurfacePayload) -> serde_json::Value {
    let framework = FrameworkTag::try_from(payload.framework)
        .map(|t| t.as_str_name().to_string())
        .unwrap_or_else(|_| format!("UNKNOWN({})", payload.framework));

    // The string table interns member names; resolve them by index.
    let strings: &[String] = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.as_slice())
        .unwrap_or(&[]);

    let surfaces: Vec<serde_json::Value> = payload
        .surfaces
        .iter()
        .map(|entry| project_kind_entry(entry, strings))
        .collect();

    serde_json::json!({
        "framework": framework,
        "surfaces": surfaces,
    })
}

fn project_kind_entry(entry: &FrameworkSurfaceKindEntry, strings: &[String]) -> serde_json::Value {
    let kind = FrameworkSurfaceKind::try_from(entry.kind)
        .map(|k| k.as_str_name().to_string())
        .unwrap_or_else(|_| format!("UNKNOWN({})", entry.kind));

    let (support, diagnostics) = match &entry.status {
        Some(status) => {
            let support = FrameworkSurfaceKindSupport::try_from(status.support)
                .map(|s| s.as_str_name().to_string())
                .unwrap_or_else(|_| format!("UNKNOWN({})", status.support));
            let diags: Vec<String> = status
                .diagnostics
                .iter()
                .map(|d| resolve_string(strings, d.message_name_id).to_string())
                .collect();
            (support, diags)
        }
        None => (
            "FRAMEWORK_SURFACE_KIND_SUPPORT_UNSPECIFIED".to_string(),
            vec![],
        ),
    };

    let members: Vec<serde_json::Value> = entry
        .members
        .iter()
        .map(|m| {
            serde_json::json!({
                "name": resolve_string(strings, m.name_id),
                "required": m.required,
                "readonly": m.readonly,
            })
        })
        .collect();

    serde_json::json!({
        "kind": kind,
        "support": support,
        "members": members,
        "diagnostics": diagnostics,
    })
}

fn project_error(error: &TypeInfoRequestError) -> serde_json::Value {
    serde_json::json!({
        "error": project_error_kind(error),
    })
}

/// Project the typed `TypeInfoRequestError` oneof into a stable
/// `{ case, detail? }` JSON shape — NOT Rust debug formatting. The `case`
/// is the wire variant tag; `detail` carries the variant's payload string
/// where it has one.
fn project_error_kind(error: &TypeInfoRequestError) -> serde_json::Value {
    use verter_protocol::verter::v1::type_info_request_error::Kind;
    let Some(kind) = &error.kind else {
        return serde_json::json!({ "case": "unspecified" });
    };
    match kind {
        Kind::MalformedPayload(p) => {
            serde_json::json!({ "case": "malformedPayload", "detail": p.detail })
        }
        Kind::MalformedStructuredExpression(p) => {
            serde_json::json!({ "case": "malformedStructuredExpression", "detail": p.detail })
        }
        Kind::UnknownSchemaVersion(p) => serde_json::json!({
            "case": "unknownSchemaVersion",
            "wireVersion": p.wire_version,
            "serverVersion": p.server_version,
        }),
        Kind::InvalidMode(p) => {
            serde_json::json!({ "case": "invalidMode", "received": p.received })
        }
        Kind::MissingProjectionContext(_) => {
            serde_json::json!({ "case": "missingProjectionContext" })
        }
        Kind::MissingDisplayPolicy(_) => serde_json::json!({ "case": "missingDisplayPolicy" }),
        Kind::MissingClosurePolicy(_) => serde_json::json!({ "case": "missingClosurePolicy" }),
        Kind::MissingProjectPath(_) => serde_json::json!({ "case": "missingProjectPath" }),
        Kind::OmittedRoots(_) => serde_json::json!({ "case": "omittedRoots" }),
        Kind::UnstableState(p) => {
            serde_json::json!({ "case": "unstableState", "attempts": p.attempts })
        }
        Kind::ExpansionBudgetOutOfRange(_) => {
            serde_json::json!({ "case": "expansionBudgetOutOfRange" })
        }
    }
}

/// Resolve an interned string-table index to its string, or a placeholder
/// when out of range (a structurally-malformed payload would do this; the
/// projection never panics on a bad index).
fn resolve_string(strings: &[String], id: u32) -> &str {
    strings.get(id as usize).map(String::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_carries_the_selector_and_full_schema() {
        let req = build_request("/App.vue", "vue");
        assert_eq!(req.schema_version, TYPEINFO_GRAPH_SCHEMA_VERSION);
        assert_eq!(req.operation, wire::Operation::FrameworkSurfaces as i32);
        let Some(type_info_graph_request::Payload::FrameworkSurface(fs)) = &req.payload else {
            panic!("expected a framework-surface payload arm");
        };
        let selector = fs.selector.as_ref().expect("selector present");
        assert_eq!(selector.canonical_id, "/App.vue");
        assert_eq!(selector.framework_adapter_id, "vue");
        assert!(!selector.has_export_name);
        assert_eq!(fs.schema_version, TYPEINFO_GRAPH_SCHEMA_VERSION);
    }

    #[test]
    fn project_response_surfaces_the_error_arm() {
        let response = TypeInfoGraphResponse {
            kind: Some(type_info_graph_response::Kind::Error(
                TypeInfoRequestError {
                    kind: Some(
                        verter_protocol::verter::v1::type_info_request_error::Kind::MalformedPayload(
                            verter_protocol::verter::v1::TypeInfoRequestErrorMalformedPayload {
                                detail: "boom".to_string(),
                            },
                        ),
                    ),
                },
            )),
        };
        let json = project_response(&response);
        // The error arm projects the TYPED oneof variant + detail, NOT a
        // Rust debug string.
        assert_eq!(json["error"]["case"], "malformedPayload");
        assert_eq!(json["error"]["detail"], "boom");
    }

    #[test]
    fn project_payload_resolves_member_names_through_the_string_table() {
        // A payload with one PROPS surface carrying a member whose name
        // interns at index 1 must project that name, not the raw index.
        let payload = FrameworkSurfacePayload {
            schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
            selector: None,
            framework: FrameworkTag::Vue as i32,
            graph: Some(wire::SemanticTypeGraph {
                strings: Some(wire::StringTable {
                    entries: vec!["".to_string(), "count".to_string()],
                }),
                ..Default::default()
            }),
            surfaces: vec![FrameworkSurfaceKindEntry {
                kind: FrameworkSurfaceKind::Props as i32,
                members: vec![wire::FrameworkSurfaceMember {
                    name_id: 1,
                    type_node_id: 0,
                    required: true,
                    readonly: false,
                }],
                status: Some(wire::FrameworkSurfaceKindStatus {
                    support: FrameworkSurfaceKindSupport::Supported as i32,
                    exactness: 0,
                    diagnostics: vec![],
                }),
            }],
        };
        let json = project_payload(&payload);
        assert_eq!(json["framework"], "FRAMEWORK_TAG_VUE");
        let surface = &json["surfaces"][0];
        assert_eq!(surface["kind"], "FRAMEWORK_SURFACE_KIND_PROPS");
        assert_eq!(
            surface["support"],
            "FRAMEWORK_SURFACE_KIND_SUPPORT_SUPPORTED"
        );
        assert_eq!(surface["members"][0]["name"], "count");
        assert_eq!(surface["members"][0]["required"], true);
    }
}
