//! Discriminating coverage for the typeinfo graph request validation
//! surface (`crates/verter_session/src/typeinfo/request_validation.rs`).
//!
//! Each FAIL case constructs a request missing or carrying a malformed
//! field and asserts the matching closed `TypeInfoRequestError`
//! variant. Each PASS case constructs the documented well-formed shape
//! and asserts validation returns `Ok(_)`.
//!
//! Discriminator: the file imports
//! `verter_session::typeinfo::request_validation::*`; pre-substrate
//! the module does not exist (`error[E0432]: unresolved import`).
//! Post-substrate every test compiles and 9+ scenarios pass.

use verter_protocol::typeinfo::graph as wire;
use verter_protocol::verter::v1::{
    graph_closure_policy, structured_type_expression as wire_expr,
    type_info_graph_request as wire_request, type_info_request_error,
};

use verter_protocol::typeinfo::graph::TYPEINFO_GRAPH_SCHEMA_VERSION;
use verter_session::typeinfo::request_validation::{
    validate_contextual_type_request, validate_evaluate_type_expression_graph_request,
    validate_expand_graph_around_request, validate_flow_narrowing_request,
    validate_framework_surface_request, validate_project_path_graph_request,
    validate_resolve_symbol_graph_request, validate_schema_version_for_operation,
    validate_type_info_graph_request, FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
    MAX_EXPANSION_DEPTH_BUDGET, MAX_EXPANSION_NODE_BUDGET, MAX_STRUCTURED_EXPRESSION_DEPTH,
    MIN_TYPEINFO_GRAPH_SCHEMA_VERSION, SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
};

fn prim_string_expr() -> wire::StructuredTypeExpression {
    wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::Primitive(wire::ExprPrimitive {
            kind: wire::PrimitiveKind::String as i32,
        })),
    }
}

fn default_context() -> wire::ProjectionReductionContext {
    wire::ProjectionReductionContext {
        mode: wire::ProjectionMode::Expanded as i32,
        demand: wire::ReductionDemand::Published as i32,
    }
}

fn one_level_closure() -> wire::ClosurePolicy {
    wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::OneLevel(
            wire::ClosureOneLevel {},
        )),
    }
}

fn default_display_policy() -> wire::DisplayPolicy {
    wire::DisplayPolicy {
        qualification: wire::DisplayQualification::Qualified as i32,
        branding: wire::DisplayBranding::On as i32,
        budgets: Some(wire::DisplayBudgets {
            max_string_length: 4096,
            max_depth: 16,
        }),
    }
}

fn valid_resolve_symbol_request() -> wire::ResolveSymbolGraphRequest {
    wire::ResolveSymbolGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    }
}

fn err_variant_label(err: &wire::TypeInfoRequestError) -> &'static str {
    let kind = err
        .kind
        .as_ref()
        .expect("validation error must carry a kind variant");
    match kind {
        type_info_request_error::Kind::MissingProjectionContext(_) => "MissingProjectionContext",
        type_info_request_error::Kind::MissingDisplayPolicy(_) => "MissingDisplayPolicy",
        type_info_request_error::Kind::InvalidMode(_) => "InvalidMode",
        type_info_request_error::Kind::MissingClosurePolicy(_) => "MissingClosurePolicy",
        type_info_request_error::Kind::UnknownSchemaVersion(_) => "UnknownSchemaVersion",
        type_info_request_error::Kind::MalformedPayload(_) => "MalformedPayload",
        type_info_request_error::Kind::OmittedRoots(_) => "OmittedRoots",
        type_info_request_error::Kind::UnstableState(_) => "UnstableState",
        type_info_request_error::Kind::MalformedStructuredExpression(_) => {
            "MalformedStructuredExpression"
        }
        type_info_request_error::Kind::MissingProjectPath(_) => "MissingProjectPath",
        type_info_request_error::Kind::ExpansionBudgetOutOfRange(_) => "ExpansionBudgetOutOfRange",
    }
}

// ───────── ResolveSymbol ─────────

#[test]
fn resolve_symbol_request_missing_projection_context_fails() {
    let mut request = valid_resolve_symbol_request();
    request.context = None;
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingProjectionContext");
}

#[test]
fn resolve_symbol_request_missing_display_policy_fails() {
    let mut request = valid_resolve_symbol_request();
    request.display_policy = None;
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingDisplayPolicy");
}

#[test]
fn resolve_symbol_request_missing_closure_policy_fails() {
    let mut request = valid_resolve_symbol_request();
    request.closure = None;
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingClosurePolicy");
}

#[test]
fn resolve_symbol_request_missing_project_path_fails() {
    let mut request = valid_resolve_symbol_request();
    request.canonical_id = String::new();
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingProjectPath");
}

#[test]
fn resolve_symbol_request_invalid_mode_fails() {
    let mut request = valid_resolve_symbol_request();
    request.context.as_mut().unwrap().mode = 9999;
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "InvalidMode");
}

#[test]
fn resolve_symbol_request_well_formed_passes() {
    validate_resolve_symbol_graph_request(&valid_resolve_symbol_request())
        .expect("a well-formed resolve-symbol request must validate");
}

// ───────── EvaluateTypeExpressionGraph ─────────

fn valid_evaluate_type_expression_request() -> wire::EvaluateTypeExpressionGraphRequest {
    wire::EvaluateTypeExpressionGraphRequest {
        scope_canonical: "/a.ts".to_string(),
        expression: Some(prim_string_expr()),
        extra_imports: vec![],
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
    }
}

#[test]
fn evaluate_type_expression_request_missing_expression_fails() {
    let mut request = valid_evaluate_type_expression_request();
    request.expression = None;
    let err = validate_evaluate_type_expression_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedStructuredExpression");
}

#[test]
fn evaluate_type_expression_request_well_formed_passes() {
    validate_evaluate_type_expression_graph_request(&valid_evaluate_type_expression_request())
        .expect("a well-formed evaluate-expression request must validate");
}

#[test]
fn evaluate_type_expression_request_with_cycle_via_depth_fails() {
    // Build a synthetic tree deeper than MAX_STRUCTURED_EXPRESSION_DEPTH
    // by nesting Union over and over. The validator's depth gate trips
    // before any semantic execution can run.
    fn nested(depth: u32) -> wire::StructuredTypeExpression {
        if depth == 0 {
            prim_string_expr()
        } else {
            wire::StructuredTypeExpression {
                kind: Some(wire_expr::Kind::Union(wire::ExprUnion {
                    members: vec![nested(depth - 1)],
                })),
            }
        }
    }

    let mut request = valid_evaluate_type_expression_request();
    request.expression = Some(nested(MAX_STRUCTURED_EXPRESSION_DEPTH + 4));
    let err = validate_evaluate_type_expression_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedStructuredExpression");
}

// ───────── ProjectPathGraph ─────────

fn valid_project_path_request() -> wire::ProjectPathGraphRequest {
    wire::ProjectPathGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        path: vec![wire::TypePathSegment {
            kind: Some(
                verter_protocol::verter::v1::graph_type_path_segment::Kind::Property(
                    wire::wire_path_segment_property(7),
                ),
            ),
        }],
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    }
}

#[test]
fn project_path_request_empty_path_fails() {
    let mut request = valid_project_path_request();
    request.path.clear();
    let err = validate_project_path_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingProjectPath");
}

#[test]
fn project_path_request_well_formed_passes() {
    validate_project_path_graph_request(&valid_project_path_request())
        .expect("a well-formed project-path request must validate");
}

// ───────── Expansion budget ─────────

#[test]
fn expanded_closure_with_out_of_range_node_budget_fails() {
    let mut request = valid_resolve_symbol_request();
    request.closure = Some(wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::Expanded(
            wire::ClosureExpanded {
                node_budget: MAX_EXPANSION_NODE_BUDGET + 1,
                depth_budget: 32,
            },
        )),
    });
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "ExpansionBudgetOutOfRange");
}

#[test]
fn expanded_closure_with_out_of_range_depth_fails() {
    let mut request = valid_resolve_symbol_request();
    request.closure = Some(wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::Expanded(
            wire::ClosureExpanded {
                node_budget: 100,
                depth_budget: MAX_EXPANSION_DEPTH_BUDGET + 1,
            },
        )),
    });
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "ExpansionBudgetOutOfRange");
}

#[test]
fn expanded_closure_with_zero_budgets_fails() {
    let mut request = valid_resolve_symbol_request();
    request.closure = Some(wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::Expanded(
            wire::ClosureExpanded {
                node_budget: 0,
                depth_budget: 0,
            },
        )),
    });
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "ExpansionBudgetOutOfRange");
}

// ───────── Schema version handshake (closed-set policy) ─────────

/// Below-MIN versions must reject. Pre-fix passes (the validator
/// already had the MIN gate); post-fix continues to pass because the
/// closed-set contract still excludes `0`. Retained for regression
/// pinning of the legacy lower bound.
#[test]
fn schema_version_below_min_fails() {
    let request = wire::TypeInfoGraphRequest {
        schema_version: 0,
        operation: wire::Operation::ResolveSymbol as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "UnknownSchemaVersion");
}

/// Sanity: the current server schema version must validate so
/// well-formed contemporary clients reach dispatch.
#[test]
fn schema_version_at_current_passes() {
    let request = wire::TypeInfoGraphRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::ResolveSymbol as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    validate_type_info_graph_request(&request).expect("the current schema version must validate");
}

/// Discriminator for the closed-set contract: a wire version
/// **above** the current server version MUST be rejected. The
/// previous validator used `version < MIN`, which accepts every
/// future version through to dispatch — that is the bug this fix
/// closes. Without this test the regression to an open-ended
/// fallback would land silently.
#[test]
fn schema_version_above_current_fails() {
    // A version far in the future. The closed-set contract means
    // dispatchers do not get to see unsupported versions.
    let request = wire::TypeInfoGraphRequest {
        schema_version: 999,
        operation: wire::Operation::ResolveSymbol as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "UnknownSchemaVersion");
}

/// The closed-set constant must contain the documented current
/// version exactly once. Adding a new supported version requires
/// extending the constant; the dispatcher's payload arms get type-
/// checked against this set so an unrecognised version cannot reach
/// semantic execution.
#[test]
fn supported_schema_versions_constant_contains_current() {
    assert!(
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.contains(&TYPEINFO_GRAPH_SCHEMA_VERSION),
        "supported-set must contain the current server schema version",
    );
    assert!(
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.contains(&MIN_TYPEINFO_GRAPH_SCHEMA_VERSION),
        "supported-set must contain the documented minimum version",
    );
}

// ───────── Operation discriminator + payload mismatch ─────────

#[test]
fn graph_request_with_mismatched_operation_payload_fails() {
    let request = wire::TypeInfoGraphRequest {
        schema_version: MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
        // Operation says "ProjectPath" but payload carries ResolveSymbol.
        operation: wire::Operation::ProjectPath as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
}

#[test]
fn graph_request_with_relate_operation_through_graph_path_fails() {
    // `relate` has its own dedicated request shape; receiving it
    // through the graph envelope is a payload mismatch.
    let request = wire::TypeInfoGraphRequest {
        schema_version: MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::Relate as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
}

// ───────── Flow / Contextual / Expand / Framework surface ─────────

#[test]
fn flow_narrowing_missing_span_fails() {
    let request = wire::FlowNarrowingRequest {
        canonical_id: "/a.ts".to_string(),
        span: None,
        context: Some(default_context()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    };
    let err = validate_flow_narrowing_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
}

#[test]
fn flow_narrowing_well_formed_passes() {
    let request = wire::FlowNarrowingRequest {
        canonical_id: "/a.ts".to_string(),
        span: Some(wire::SpanRef {
            canonical_id: "/a.ts".to_string(),
            start: 1,
            end: 4,
        }),
        context: Some(default_context()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    };
    validate_flow_narrowing_request(&request).expect("flow narrowing request must validate");
}

#[test]
fn contextual_type_well_formed_passes() {
    let request = wire::ContextualTypeRequest {
        canonical_id: "/a.ts".to_string(),
        span: Some(wire::SpanRef {
            canonical_id: "/a.ts".to_string(),
            start: 1,
            end: 4,
        }),
        context: Some(default_context()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    };
    validate_contextual_type_request(&request).expect("contextual type request must validate");
}

#[test]
fn expand_around_well_formed_passes() {
    let request = wire::ExpandGraphAroundRequest {
        parent_graph: Some(wire::Handle {
            opaque: vec![1, 2, 3],
        }),
        target: Some(wire::TypeNodeRef {
            node_id: 3,
            identity: None,
            is_canonical: false,
        }),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    };
    validate_expand_graph_around_request(&request).expect("expand-around request must validate");
}

fn framework_surface_request_with(
    adapter_id: &str,
    schema_version: u32,
) -> wire::FrameworkSurfaceRequest {
    wire::FrameworkSurfaceRequest {
        selector: Some(wire::ComponentSelector {
            canonical_id: "/Foo.vue".to_string(),
            export_name: String::new(),
            has_export_name: false,
            framework_adapter_id: adapter_id.to_string(),
        }),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        schema_version,
    }
}

#[test]
fn framework_surface_request_missing_adapter_id_fails() {
    let request = framework_surface_request_with("", FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION);
    let err = validate_framework_surface_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
}

#[test]
fn framework_surface_request_well_formed_passes() {
    let request = framework_surface_request_with("vue", FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION);
    validate_framework_surface_request(&request)
        .expect("a well-formed framework surface request must validate");
}

// ───────── Per-operation schema-version minimum (legacy-ops-only schema 2) ─────────
//
// Schema 2 is LEGACY-OPERATIONS-ONLY: every pre-existing operation
// accepts `[2, 3]`; the framework-surface operation requires 3. The
// gate runs through `validate_schema_version_for_operation` — global
// membership first (UnknownSchemaVersion outside `[2, 3]`), then the
// per-operation minimum (MalformedPayload below the op minimum — NOT
// UnknownSchemaVersion, because v2 stays globally supported; NOT a
// new error oneof arm, because v2 clients could not decode one).

/// Builds a valid envelope for one legacy graph-payload operation at
/// the given schema version.
fn legacy_envelope(operation: wire::Operation, schema_version: u32) -> wire::TypeInfoGraphRequest {
    let payload = match operation {
        wire::Operation::ResolveSymbol => {
            wire_request::Payload::ResolveSymbol(valid_resolve_symbol_request())
        }
        wire::Operation::EvaluateExpression => {
            wire_request::Payload::EvaluateTypeExpression(valid_evaluate_type_expression_request())
        }
        wire::Operation::ProjectPath => {
            wire_request::Payload::ProjectPath(valid_project_path_request())
        }
        wire::Operation::FlowNarrowingAt => {
            wire_request::Payload::FlowNarrowing(wire::FlowNarrowingRequest {
                canonical_id: "/a.ts".to_string(),
                span: Some(wire::SpanRef {
                    canonical_id: "/a.ts".to_string(),
                    start: 1,
                    end: 4,
                }),
                context: Some(default_context()),
                display_policy: Some(default_display_policy()),
                include_provenance: false,
                include_diagnostics: false,
            })
        }
        wire::Operation::ContextualTypeAt => {
            wire_request::Payload::ContextualType(wire::ContextualTypeRequest {
                canonical_id: "/a.ts".to_string(),
                span: Some(wire::SpanRef {
                    canonical_id: "/a.ts".to_string(),
                    start: 1,
                    end: 4,
                }),
                context: Some(default_context()),
                display_policy: Some(default_display_policy()),
                include_provenance: false,
                include_diagnostics: false,
            })
        }
        wire::Operation::ExpandAround => {
            wire_request::Payload::ExpandAround(wire::ExpandGraphAroundRequest {
                parent_graph: Some(wire::Handle {
                    opaque: vec![1, 2, 3],
                }),
                target: Some(wire::TypeNodeRef {
                    node_id: 3,
                    identity: None,
                    is_canonical: false,
                }),
                context: Some(default_context()),
                closure: Some(one_level_closure()),
                display_policy: Some(default_display_policy()),
                include_provenance: false,
                include_diagnostics: false,
            })
        }
        wire::Operation::Relate | wire::Operation::FrameworkSurfaces => {
            panic!("legacy_envelope covers the six legacy graph-payload operations only")
        }
    };
    wire::TypeInfoGraphRequest {
        schema_version,
        operation: operation as i32,
        payload: Some(payload),
    }
}

/// The six legacy graph-payload operations, asserted OP-BY-OP (not
/// sampled). `Relate` rides a dedicated request shape outside this
/// envelope; its op-minimum is pinned through the direct
/// `validate_schema_version_for_operation` walk below.
const LEGACY_GRAPH_PAYLOAD_OPERATIONS: &[wire::Operation] = &[
    wire::Operation::ResolveSymbol,
    wire::Operation::EvaluateExpression,
    wire::Operation::ProjectPath,
    wire::Operation::FlowNarrowingAt,
    wire::Operation::ContextualTypeAt,
    wire::Operation::ExpandAround,
];

#[test]
fn every_legacy_operation_accepts_schema_two_and_three_op_by_op() {
    for &operation in LEGACY_GRAPH_PAYLOAD_OPERATIONS {
        for version in [
            MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            TYPEINFO_GRAPH_SCHEMA_VERSION,
        ] {
            validate_type_info_graph_request(&legacy_envelope(operation, version)).unwrap_or_else(
                |err| {
                    panic!(
                        "legacy operation {operation:?} must accept schema {version}, got {err:?}"
                    )
                },
            );
        }
    }
}

#[test]
fn schema_versions_one_and_five_are_rejected_with_typed_errors() {
    // 1 is below the floor; 5 is the first version ABOVE the current supported
    // set ([2, 3, 4]). Both are outside the closed set and rejected.
    for version in [1u32, 5u32] {
        let err = validate_type_info_graph_request(&legacy_envelope(
            wire::Operation::ResolveSymbol,
            version,
        ))
        .unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "UnknownSchemaVersion",
            "schema {version} is outside the closed supported set and must be \
             rejected with the typed UnknownSchemaVersion error",
        );
    }
}

fn framework_surface_envelope(
    envelope_version: u32,
    payload_version: u32,
) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: envelope_version,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            framework_surface_request_with("vue", payload_version),
        )),
    }
}

#[test]
fn framework_surface_operation_accepts_schema_three() {
    validate_type_info_graph_request(&framework_surface_envelope(
        FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
        FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
    ))
    .expect("a v3 framework-surface request must validate");
}

/// DISCRIMINATING: the framework-surface operation rejects schema 2
/// with `MalformedPayload` — and the rejection happens BEFORE any
/// adapter lookup or semantic dispatch. Both validators exercised
/// here are pure shape-only functions (no host, no registry, no
/// resolver access exists in this module — pinned by the
/// `typeinfo_request_validation_is_a_separate_module` guard), and
/// `validate_type_info_graph_request` is the gate every
/// `_with_audit` entry-point runs before semantic execution; a typed
/// error from the pure validator therefore structurally precedes any
/// adapter lookup.
#[test]
fn framework_surface_operation_rejects_schema_two_with_malformed_payload() {
    // Envelope path: a v2 framework-surface envelope is rejected.
    let err = validate_type_info_graph_request(&framework_surface_envelope(
        MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
        MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    ))
    .unwrap_err();
    assert_eq!(
        err_variant_label(&err),
        "MalformedPayload",
        "a v2 framework-surface request must be rejected with MalformedPayload — \
         NOT UnknownSchemaVersion (v2 stays globally supported) and NOT a new \
         error oneof arm (v2 clients could not decode one)",
    );

    // Payload path: `validate_framework_surface_request` applies the
    // same op-minimum gate on the payload's own schema_version.
    let payload_err = validate_framework_surface_request(&framework_surface_request_with(
        "vue",
        MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    ))
    .unwrap_err();
    assert_eq!(err_variant_label(&payload_err), "MalformedPayload");
}

#[test]
fn framework_surface_envelope_payload_version_mismatch_is_malformed_payload() {
    let err = validate_type_info_graph_request(&framework_surface_envelope(
        FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
        MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    ))
    .unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
    let detail = match err.kind.as_ref().expect("error kind") {
        type_info_request_error::Kind::MalformedPayload(p) => p.detail.as_str(),
        other => panic!("expected MalformedPayload, got {other:?}"),
    };
    assert!(
        detail.contains("mismatch"),
        "the envelope/payload version-mismatch rejection must carry a \
         mismatch detail, got: {detail}",
    );
}

#[test]
fn supported_schema_version_set_is_exactly_two_three_and_four() {
    assert_eq!(
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
        &[
            MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            3,
            TYPEINFO_GRAPH_SCHEMA_VERSION
        ],
        "the supported set holds every version some operation still accepts: \
         schema 2 (legacy-operations-only), schema 3 (the framework-surface \
         floor), and schema 4 (current — adds the member default/origin fields)",
    );
    assert_eq!(MIN_TYPEINFO_GRAPH_SCHEMA_VERSION, 2);
    assert_eq!(TYPEINFO_GRAPH_SCHEMA_VERSION, 4);
    assert_eq!(FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION, 3);
}

/// The supported-set advertisement surface: the `UnknownSchemaVersion`
/// error payload's `server_supported_versions`, populated from
/// `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` via
/// `wire_error_unknown_schema_version` — the single advertisement
/// source. It reports `[2, 3, 4]`.
#[test]
fn unknown_schema_version_error_advertises_two_three_and_four() {
    // Version 5 is the first version OUTSIDE the supported set — it triggers
    // the UnknownSchemaVersion rejection (4 is now supported).
    let err = validate_type_info_graph_request(&legacy_envelope(wire::Operation::ResolveSymbol, 5))
        .unwrap_err();
    let payload = match err.kind.as_ref().expect("error kind") {
        type_info_request_error::Kind::UnknownSchemaVersion(p) => p,
        other => panic!("expected UnknownSchemaVersion, got {other:?}"),
    };
    assert_eq!(
        payload.server_supported_versions,
        vec![2, 3, 4],
        "the advertisement surface must report exactly [2, 3, 4]",
    );
    assert_eq!(
        payload.server_supported_versions,
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.to_vec(),
        "the advertisement is fed by the supported-set constant — the single source",
    );

    // The constructor itself is the single advertisement source.
    let wire_payload = wire::wire_error_unknown_schema_version(
        5,
        TYPEINFO_GRAPH_SCHEMA_VERSION,
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
    );
    assert_eq!(wire_payload.server_supported_versions, vec![2, 3, 4]);
}

/// Walks EVERY operation discriminant through
/// `validate_schema_version_for_operation`. The mirror match below is
/// wildcard-free, so a future `Operation` variant fails to compile
/// here (and in the production gate) until its op-minimum row is
/// decided.
#[test]
fn op_minimum_gate_walks_every_operation_discriminant() {
    /// Expected per-operation minimum — a wildcard-free mirror of the
    /// production gate's exhaustive match.
    fn expected_minimum(operation: wire::Operation) -> u32 {
        match operation {
            wire::Operation::ResolveSymbol
            | wire::Operation::EvaluateExpression
            | wire::Operation::ProjectPath
            | wire::Operation::Relate
            | wire::Operation::ExpandAround
            | wire::Operation::FlowNarrowingAt
            | wire::Operation::ContextualTypeAt => MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            wire::Operation::FrameworkSurfaces => FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
        }
    }

    const EVERY_OPERATION: &[wire::Operation] = &[
        wire::Operation::ResolveSymbol,
        wire::Operation::EvaluateExpression,
        wire::Operation::ProjectPath,
        wire::Operation::Relate,
        wire::Operation::FrameworkSurfaces,
        wire::Operation::ExpandAround,
        wire::Operation::FlowNarrowingAt,
        wire::Operation::ContextualTypeAt,
    ];
    assert_eq!(
        EVERY_OPERATION.len(),
        8,
        "the walk must cover every operation discriminant",
    );

    for &operation in EVERY_OPERATION {
        let minimum = expected_minimum(operation);

        // At or above the minimum (within the supported set): Ok.
        for version in SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS
            .iter()
            .copied()
            .filter(|v| *v >= minimum)
        {
            validate_schema_version_for_operation(operation, version).unwrap_or_else(|err| {
                panic!("{operation:?} must accept schema {version}, got {err:?}")
            });
        }

        // Below the minimum but globally supported: MalformedPayload.
        for version in SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS
            .iter()
            .copied()
            .filter(|v| *v < minimum)
        {
            let err = validate_schema_version_for_operation(operation, version).unwrap_err();
            assert_eq!(
                err_variant_label(&err),
                "MalformedPayload",
                "{operation:?} below its op-minimum must reject with MalformedPayload",
            );
        }

        // Outside the global set: UnknownSchemaVersion regardless of
        // the operation (global membership runs first). 1 is below the
        // floor; 5 is the first version above the current set ([2, 3, 4]).
        for version in [1u32, 5u32] {
            let err = validate_schema_version_for_operation(operation, version).unwrap_err();
            assert_eq!(
                err_variant_label(&err),
                "UnknownSchemaVersion",
                "{operation:?} outside the global set must reject with UnknownSchemaVersion",
            );
        }
    }
}

/// The rewritten `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` rustdoc
/// states the legacy-operations-only contract; the retired
/// single-version policy text ("single-sourced from", "removes
/// obsolete ones") may not survive — code and policy text may not
/// diverge. Also pins the production op-minimum match as
/// wildcard-free (the compile-time half of the exhaustiveness rule).
#[test]
fn supported_versions_rustdoc_states_legacy_operations_only_contract() {
    let crate_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path = crate_root.join("src/typeinfo/request_validation.rs");
    let source = std::fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", source_path.display()));

    let const_idx = source
        .find("pub const SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS")
        .expect("the supported-set constant must exist");
    let doc_window = &source[const_idx.saturating_sub(2000)..const_idx];
    assert!(
        doc_window.contains("legacy-operations-only"),
        "the supported-set rustdoc must state the legacy-operations-only contract",
    );
    assert!(
        !doc_window.contains("removes obsolete ones in the same"),
        "the retired single-version policy text must not survive the rewrite",
    );

    // The production op-minimum gate matches exhaustively with NO
    // wildcard arm: a future operation cannot compile without an
    // explicit op-minimum decision.
    let fn_idx = source
        .find("pub fn validate_schema_version_for_operation")
        .expect("the op-minimum gate must exist");
    let fn_window = &source[fn_idx
        ..source[fn_idx..]
            .find("\npub ")
            .map_or(source.len(), |o| fn_idx + o)];
    assert!(
        !fn_window.contains("_ =>"),
        "validate_schema_version_for_operation must match exhaustively with no wildcard arm",
    );
}

// ───────── Exhaustive structured-expression coverage ─────────
//
// The validator dispatches on the outer `StructuredTypeExpression`
// `kind` enum exhaustively. Each field-bearing variant has a per-
// variant validator that walks its required nested fields. The tests
// below construct one malformed instance per field-bearing variant
// and assert it routes through `MalformedStructuredExpression`.
//
// These tests act as the discriminator for the exhaustive-validation
// fix: pre-fix, several variants (FunctionExpr, ClassExpr, InferExpr,
// Mapped's type_param, object call/index signatures, UniqueSymbol's
// decl_canonical, TypeofExpr's value_root_canonical) had no nested
// validator — malformed payloads slipped through. Post-fix every
// variant's nested-field check trips on the corresponding test.

fn evaluate_with_expression(
    expr: wire::StructuredTypeExpression,
) -> wire::EvaluateTypeExpressionGraphRequest {
    wire::EvaluateTypeExpressionGraphRequest {
        scope_canonical: "/a.ts".to_string(),
        expression: Some(expr),
        extra_imports: vec![],
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
    }
}

fn malformed_expression_label() -> &'static str {
    "MalformedStructuredExpression"
}

fn assert_malformed_expr(expr: wire::StructuredTypeExpression) {
    let request = evaluate_with_expression(expr);
    let err = validate_evaluate_type_expression_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), malformed_expression_label());
}

#[test]
fn function_expr_missing_return_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::FunctionExpr(Box::new(
            wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![],
                return_expr: None,
                signature_kind: wire::SignatureKind::Call as i32,
            },
        ))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn function_expr_parameter_missing_type_ref_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::FunctionExpr(Box::new(
            wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![wire::FunctionParameterExpr {
                    name: "p".to_string(),
                    type_ref: None,
                    optional: false,
                    rest: false,
                    inference_policy: wire::InferencePolicy::Normal as i32,
                }],
                return_expr: Some(Box::new(wire::FunctionReturnExpr {
                    kind: Some(
                        verter_protocol::verter::v1::function_return_expr::Kind::Type(Box::new(
                            prim_string_expr(),
                        )),
                    ),
                })),
                signature_kind: wire::SignatureKind::Call as i32,
            },
        ))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn function_expr_type_parameter_missing_name_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::FunctionExpr(Box::new(
            wire::ExprFunction {
                type_parameters: vec![wire::TypeParameterExpr {
                    name: String::new(),
                    constraint: None,
                    has_constraint: false,
                    default_type: None,
                    has_default: false,
                    variance: wire::Variance::Independent as i32,
                    is_const: false,
                }],
                this_param: None,
                has_this_param: false,
                parameters: vec![],
                return_expr: Some(Box::new(wire::FunctionReturnExpr {
                    kind: Some(
                        verter_protocol::verter::v1::function_return_expr::Kind::Type(Box::new(
                            prim_string_expr(),
                        )),
                    ),
                })),
                signature_kind: wire::SignatureKind::Call as i32,
            },
        ))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn class_expr_has_name_but_empty_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::ClassExpr(wire::ExprClass {
            class_name: String::new(),
            has_class_name: true,
            type_parameters: vec![],
            instance_members: vec![],
            static_members: vec![],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn class_expr_member_missing_value_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::ClassExpr(wire::ExprClass {
            class_name: "C".to_string(),
            has_class_name: true,
            type_parameters: vec![],
            instance_members: vec![wire::ObjectMemberExpr {
                name: "m".to_string(),
                name_kind: wire::MemberNameKind::Identifier as i32,
                value: None,
                optional_member: false,
                readonly: false,
            }],
            static_members: vec![],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn infer_expr_missing_name_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::InferExpr(Box::new(wire::ExprInfer {
            name: String::new(),
            constraint: None,
            has_constraint: false,
        }))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn infer_expr_has_constraint_but_missing_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::InferExpr(Box::new(wire::ExprInfer {
            name: "U".to_string(),
            constraint: None,
            has_constraint: true,
        }))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn mapped_expr_missing_type_param_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::Mapped(Box::new(wire::ExprMapped {
            type_param: None,
            name_remap: None,
            has_name_remap: false,
            value_type: Some(Box::new(prim_string_expr())),
            readonly_modifier: wire::MappedModifier::None as i32,
            optional_modifier: wire::MappedModifier::None as i32,
        }))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn mapped_expr_type_param_missing_binder_id_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::Mapped(Box::new(wire::ExprMapped {
            type_param: Some(Box::new(wire::MappedTypeParamExpr {
                binder_id: String::new(),
                name: "K".to_string(),
                constraint: None,
            })),
            name_remap: None,
            has_name_remap: false,
            value_type: Some(Box::new(prim_string_expr())),
            readonly_modifier: wire::MappedModifier::None as i32,
            optional_modifier: wire::MappedModifier::None as i32,
        }))),
    };
    assert_malformed_expr(expr);
}

#[test]
fn object_literal_index_signature_missing_value_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::ObjectLiteral(wire::ExprObject {
            members: vec![],
            index_signatures: vec![wire::IndexSignatureExpr {
                key_kind: wire::IndexKeyKind::String as i32,
                value: None,
                readonly: false,
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn object_literal_call_signature_missing_return_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::ObjectLiteral(wire::ExprObject {
            members: vec![],
            index_signatures: vec![],
            call_signatures: vec![wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![],
                return_expr: None,
                signature_kind: wire::SignatureKind::Call as i32,
            }],
            construct_signatures: vec![],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn object_literal_construct_signature_missing_return_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::ObjectLiteral(wire::ExprObject {
            members: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![],
                return_expr: None,
                signature_kind: wire::SignatureKind::Construct as i32,
            }],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn typeof_expr_missing_value_root_canonical_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::TypeofExpr(wire::ExprTypeOf {
            value_root_canonical: String::new(),
            path: vec!["x".to_string()],
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn unique_symbol_missing_decl_canonical_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::UniqueSymbol(wire::ExprUniqueSymbol {
            decl_canonical: String::new(),
            name: "id".to_string(),
        })),
    };
    assert_malformed_expr(expr);
}

#[test]
fn unique_symbol_missing_name_fails() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::UniqueSymbol(wire::ExprUniqueSymbol {
            decl_canonical: "/a.ts".to_string(),
            name: String::new(),
        })),
    };
    assert_malformed_expr(expr);
}

/// Sanity: a well-formed FunctionExpr with parameters and a typed
/// return must validate. Negative discriminator for the new
/// function-shape check.
#[test]
fn function_expr_well_formed_passes() {
    let expr = wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::FunctionExpr(Box::new(
            wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![wire::FunctionParameterExpr {
                    name: "p".to_string(),
                    type_ref: Some(Box::new(prim_string_expr())),
                    optional: false,
                    rest: false,
                    inference_policy: wire::InferencePolicy::Normal as i32,
                }],
                return_expr: Some(Box::new(wire::FunctionReturnExpr {
                    kind: Some(
                        verter_protocol::verter::v1::function_return_expr::Kind::Type(Box::new(
                            prim_string_expr(),
                        )),
                    ),
                })),
                signature_kind: wire::SignatureKind::Call as i32,
            },
        ))),
    };
    let request = evaluate_with_expression(expr);
    validate_evaluate_type_expression_graph_request(&request)
        .expect("well-formed function expression must validate");
}

// ───────── FunctionReturnExpr: deep nested validation ─────────
//
// `FunctionReturnExpr` carries a closed `oneof` (`Type` / `Predicate`
// / `Assertion`). Pre-fix the validator stopped at the outer
// discriminator and never reached `TypePredicateExpr.parameter`
// (subject) nor `AssertionEffectExpr.kind`. Malformed payloads with
// the right outer arm but absent nested required fields slipped past
// shape validation and could reach semantic execution. The tests
// below construct one malformed payload per nested gap and assert
// each routes through `MalformedStructuredExpression`.

fn function_expr_with_return(
    return_kind: verter_protocol::verter::v1::function_return_expr::Kind,
) -> wire::StructuredTypeExpression {
    wire::StructuredTypeExpression {
        kind: Some(wire_expr::Kind::FunctionExpr(Box::new(
            wire::ExprFunction {
                type_parameters: vec![],
                this_param: None,
                has_this_param: false,
                parameters: vec![wire::FunctionParameterExpr {
                    name: "p".to_string(),
                    type_ref: Some(Box::new(prim_string_expr())),
                    optional: false,
                    rest: false,
                    inference_policy: wire::InferencePolicy::Normal as i32,
                }],
                return_expr: Some(Box::new(wire::FunctionReturnExpr {
                    kind: Some(return_kind),
                })),
                signature_kind: wire::SignatureKind::Call as i32,
            },
        ))),
    }
}

/// `value is T` predicate with `predicate_type` set but no `parameter`
/// (subject). Pre-fix the validator only checked `predicate_type`; the
/// missing subject slipped past. Post-fix
/// `validate_type_predicate_expr` rejects with
/// `MalformedStructuredExpression`.
#[test]
fn function_return_predicate_without_parameter_fails() {
    let expr = function_expr_with_return(
        verter_protocol::verter::v1::function_return_expr::Kind::Predicate(Box::new(
            wire::TypePredicateExpr {
                parameter: None,
                predicate_type: Some(Box::new(prim_string_expr())),
                asserts: false,
            },
        )),
    );
    assert_malformed_expr(expr);
}

/// `value is T` predicate with a `PredicateSubject` that has its
/// `kind` oneof unset (closed-enum discriminator). Pre-fix the
/// validator did not look at the subject at all; post-fix
/// `validate_predicate_subject` rejects with
/// `MalformedStructuredExpression`.
#[test]
fn function_return_predicate_with_invalid_subject_kind_fails() {
    let expr = function_expr_with_return(
        verter_protocol::verter::v1::function_return_expr::Kind::Predicate(Box::new(
            wire::TypePredicateExpr {
                parameter: Some(verter_protocol::verter::v1::PredicateSubject { kind: None }),
                predicate_type: Some(Box::new(prim_string_expr())),
                asserts: false,
            },
        )),
    );
    assert_malformed_expr(expr);
}

/// `asserts ...` effect with the closed-enum `kind` oneof unset.
/// Pre-fix the assertion arm was ignored entirely; post-fix
/// `validate_assertion_effect_expr` rejects with
/// `MalformedStructuredExpression`.
#[test]
fn function_return_assertion_without_kind_fails() {
    let expr = function_expr_with_return(
        verter_protocol::verter::v1::function_return_expr::Kind::Assertion(Box::new(
            wire::AssertionEffectExpr { kind: None },
        )),
    );
    assert_malformed_expr(expr);
}

/// `asserts x is T` identifier-arm effect with `has_predicate=true`
/// but no `predicate` payload. The boolean flag is the single source
/// of truth — a true flag with absent payload is malformed. Pre-fix
/// the assertion arm was ignored entirely; post-fix
/// `validate_assertion_effect_identifier` rejects with
/// `MalformedStructuredExpression`.
#[test]
fn function_return_assertion_has_predicate_true_without_predicate_fails() {
    use verter_protocol::verter::v1::assertion_effect_expr;
    let expr = function_expr_with_return(
        verter_protocol::verter::v1::function_return_expr::Kind::Assertion(Box::new(
            wire::AssertionEffectExpr {
                kind: Some(assertion_effect_expr::Kind::Identifier(Box::new(
                    wire::AssertionEffectIdentifier {
                        name: 0,
                        predicate: None,
                        has_predicate: true,
                    },
                ))),
            },
        )),
    );
    assert_malformed_expr(expr);
}
