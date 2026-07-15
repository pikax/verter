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
            let mut member = serde_json::json!({
                "name": resolve_string(strings, m.name_id),
                "required": m.required,
                "readonly": m.readonly,
            });
            // Schema 4 add-only fields: the member's runtime DEFAULT source text
            // and its resolver-known declaration ORIGIN, surfaced only when
            // present (presence-aware default, optional origin).
            let object = member.as_object_mut().expect("member is a JSON object");
            if let Some(default_id) = m.default_value_id {
                object.insert(
                    "default".to_string(),
                    serde_json::Value::String(resolve_string(strings, default_id).to_string()),
                );
            }
            if let Some(origin) = m.origin.as_ref() {
                object.insert("origin".to_string(), project_member_origin(origin, strings));
            }
            member
        })
        .collect();

    serde_json::json!({
        "kind": kind,
        "support": support,
        "members": members,
        "diagnostics": diagnostics,
    })
}

/// Project a wire member origin (schema 4) into a stable JSON shape, resolving
/// string ids through the graph string table.
fn project_member_origin(
    origin: &wire::FrameworkSurfaceMemberOrigin,
    strings: &[String],
) -> serde_json::Value {
    let declaration = origin.declaration.as_ref().map(|d| {
        serde_json::json!({
            "resolvedName": resolve_string(strings, d.resolved_name_id),
            "canonicalSource": resolve_string(strings, d.canonical_source_id),
            "spanStart": d.span_start,
            "spanEnd": d.span_end,
        })
    });
    let chain: Vec<serde_json::Value> = origin
        .chain
        .iter()
        .map(|hop| {
            let kind = wire::FrameworkSurfaceOriginHopKind::try_from(hop.kind)
                .map(|k| k.as_str_name().to_string())
                .unwrap_or_else(|_| format!("UNKNOWN({})", hop.kind));
            // PRESENCE-AWARE: each hop string id is `optional` on the wire.
            // A field is projected ONLY when genuinely set — an absent field
            // is omitted entirely, NEVER resolved through the zero-based string
            // table (where id 0 is a real interned entry, not an absent
            // sentinel). The hop kind selects which fields a hop carries.
            let mut hop_json = serde_json::Map::new();
            hop_json.insert("kind".to_string(), serde_json::Value::String(kind));
            insert_optional_string(&mut hop_json, "from", hop.from_id, strings);
            insert_optional_string(&mut hop_json, "specifier", hop.specifier_id, strings);
            insert_optional_string(&mut hop_json, "importedName", hop.imported_name_id, strings);
            insert_optional_string(&mut hop_json, "to", hop.to_id, strings);
            insert_optional_string(&mut hop_json, "exportedName", hop.exported_name_id, strings);
            insert_optional_string(&mut hop_json, "originalName", hop.original_name_id, strings);
            insert_optional_string(&mut hop_json, "aliasName", hop.alias_name_id, strings);
            serde_json::Value::Object(hop_json)
        })
        .collect();
    serde_json::json!({
        "declaration": declaration,
        "chain": chain,
    })
}

/// Insert a PRESENCE-AWARE hop string field into `obj` under `key`, resolving
/// `id` through the graph string table ONLY when it is genuinely present
/// (`Some`). An absent (`None`) field is omitted — never resolved to the
/// zero-based table's entry 0.
fn insert_optional_string(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    id: Option<u32>,
    strings: &[String],
) {
    if let Some(id) = id {
        obj.insert(
            key.to_string(),
            serde_json::Value::String(resolve_string(strings, id).to_string()),
        );
    }
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
                    default_value_id: None,
                    origin: None,
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

    #[test]
    fn project_member_origin_omits_absent_hop_fields_over_a_nonempty_table_zero() {
        // DISCRIMINATING (the P0): the graph string table's entry 0 is a real
        // interned string — the encoder NEVER seeds it with `""`. A LOCAL hop
        // carries NO string fields; an IMPORT hop without a specifier carries
        // `from`+`importedName` but no specifier. The presence-aware projection
        // must OMIT the absent fields, never resolve them to entry 0
        // (`"__SENTINEL_ZERO__"`). The old id-0 decode fabricated entry 0 for
        // every absent field — RED before the presence-aware fix.
        let strings = vec![
            "__SENTINEL_ZERO__".to_string(),
            "/lib/props.ts".to_string(),
            "Size".to_string(),
        ];
        let origin = wire::FrameworkSurfaceMemberOrigin {
            declaration: Some(wire::FrameworkSurfaceMemberDeclaration {
                requested_name_id: 2,
                resolved_name_id: 2,
                canonical_source_id: 1,
                span_start: 0,
                span_end: 0,
                kind: wire::FrameworkSurfaceDeclarationKind::TypeAlias as i32,
            }),
            chain: vec![
                // LOCAL: no string fields at all.
                wire::FrameworkSurfaceOriginHop {
                    kind: wire::FrameworkSurfaceOriginHopKind::Local as i32,
                    from_id: None,
                    specifier_id: None,
                    imported_name_id: None,
                    to_id: None,
                    exported_name_id: None,
                    original_name_id: None,
                    alias_name_id: None,
                },
                // IMPORT: from + importedName present, NO specifier.
                wire::FrameworkSurfaceOriginHop {
                    kind: wire::FrameworkSurfaceOriginHopKind::Import as i32,
                    from_id: Some(1),
                    specifier_id: None,
                    imported_name_id: Some(2),
                    to_id: None,
                    exported_name_id: None,
                    original_name_id: None,
                    alias_name_id: None,
                },
            ],
        };

        let json = project_member_origin(&origin, &strings);
        let chain = json["chain"].as_array().expect("chain is an array");

        // LOCAL hop: only `kind` — every string field OMITTED, never entry 0.
        let local = &chain[0];
        assert_eq!(local["kind"], "FRAMEWORK_SURFACE_ORIGIN_HOP_KIND_LOCAL");
        for key in [
            "from",
            "specifier",
            "importedName",
            "to",
            "exportedName",
            "originalName",
            "aliasName",
        ] {
            assert!(
                local.get(key).is_none(),
                "LOCAL hop must omit `{key}`, not resolve it to string-table entry 0"
            );
        }

        // IMPORT hop: from + importedName resolved; specifier OMITTED.
        let import = &chain[1];
        assert_eq!(import["kind"], "FRAMEWORK_SURFACE_ORIGIN_HOP_KIND_IMPORT");
        assert_eq!(import["from"], "/lib/props.ts");
        assert_eq!(import["importedName"], "Size");
        assert!(
            import.get("specifier").is_none(),
            "an IMPORT hop with no recorded specifier must omit it, not fabricate entry 0"
        );
    }
}
