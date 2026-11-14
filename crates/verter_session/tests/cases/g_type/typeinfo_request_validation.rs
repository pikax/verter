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
    validate_type_info_graph_request, EXPAND_AROUND_MIN_SCHEMA_VERSION,
    FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION, FRAMEWORK_SURFACE_REQUESTED_KINDS_MIN_SCHEMA_VERSION,
    LIST_SYMBOLS_MIN_SCHEMA_VERSION, MAX_EXPANSION_DEPTH_BUDGET, MAX_EXPANSION_NODE_BUDGET,
    MAX_STRUCTURED_EXPRESSION_DEPTH, MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
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
        type_info_request_error::Kind::UnknownFrameworkProfile(_) => "UnknownFrameworkProfile",
        type_info_request_error::Kind::StaleBasis(_) => "StaleBasis",
        type_info_request_error::Kind::UnknownNodeRef(_) => "UnknownNodeRef",
        type_info_request_error::Kind::UnsupportedOperation(_) => "UnsupportedOperation",
        type_info_request_error::Kind::UnknownSource(_) => "UnknownSource",
    }
}

/// A well-shaped node reference: 32-byte query, basis and slot digests and
/// an empty (declaration-root) role.
fn well_shaped_node_ref() -> wire::NodeRef {
    wire::NodeRef {
        query_identity_digest: vec![1; 32],
        input_basis_digest: vec![2; 32],
        decl_slot_digest: vec![3; 32],
        role_digest: Vec::new(),
    }
}

fn expand_around_request_with(node_ref: Option<wire::NodeRef>) -> wire::ExpandGraphAroundRequest {
    wire::ExpandGraphAroundRequest {
        node_ref,
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
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

#[test]
fn resolve_symbol_request_rejects_unavailable_provenance() {
    let mut request = valid_resolve_symbol_request();
    request.include_provenance = true;
    let err = validate_resolve_symbol_graph_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
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

/// A served operation whose payload arm belongs to another operation is a
/// client bug: `MalformedPayload`, never the unserved-operation answer.
/// The current schema clears every served operation's floor, so only the
/// payload check can refuse.
#[test]
fn a_served_operation_with_the_wrong_payload_stays_malformed() {
    let cases = [
        (
            wire::Operation::ExpandAround,
            wire_request::Payload::ResolveSymbol(valid_resolve_symbol_request()),
        ),
        (
            wire::Operation::ResolveSymbol,
            wire_request::Payload::FrameworkSurface(framework_surface_request_with(
                well_shaped_profile(),
                TYPEINFO_GRAPH_SCHEMA_VERSION,
            )),
        ),
    ];
    for (operation, payload) in cases {
        let request = wire::TypeInfoGraphRequest {
            schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
            operation: operation as i32,
            payload: Some(payload),
        };
        let err = validate_type_info_graph_request(&request).unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "{operation:?} with another operation's payload is malformed"
        );
    }
}

/// The operations the host does not serve. Each is refused whole as
/// `UnsupportedOperation`, whatever payload it carries.
const UNSERVED_OPERATIONS: &[wire::Operation] = &[
    wire::Operation::EvaluateExpression,
    wire::Operation::ProjectPath,
    wire::Operation::Relate,
    wire::Operation::FlowNarrowingAt,
    wire::Operation::ContextualTypeAt,
];

fn assert_unsupported(
    request: &wire::TypeInfoGraphRequest,
    requested: wire::Operation,
    case: &str,
) {
    let err = validate_type_info_graph_request(request).unwrap_err();
    let Some(type_info_request_error::Kind::UnsupportedOperation(unsupported)) = &err.kind else {
        panic!(
            "{requested:?} at schema {} with {case} must be UnsupportedOperation, got {}",
            request.schema_version,
            err_variant_label(&err)
        );
    };
    // Resolve-symbol is the proto default (0): an unset echo would name it.
    assert_eq!(
        unsupported.operation, requested as i32,
        "the refusal echoes the requested operation"
    );
}

#[test]
fn every_unserved_operation_is_unsupported_never_malformed() {
    for &operation in UNSERVED_OPERATIONS {
        for version in [
            MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            TYPEINFO_GRAPH_SCHEMA_VERSION,
        ] {
            // Its own (coherent) payload arm; relate has none, so it carries
            // the resolve-symbol arm.
            assert_unsupported(
                &legacy_envelope(operation, version),
                operation,
                "its coherent payload",
            );
            // Another operation's payload arm.
            let mismatched = if operation == wire::Operation::ProjectPath {
                wire_request::Payload::ResolveSymbol(valid_resolve_symbol_request())
            } else {
                wire_request::Payload::ProjectPath(valid_project_path_request())
            };
            assert_unsupported(
                &wire::TypeInfoGraphRequest {
                    schema_version: version,
                    operation: operation as i32,
                    payload: Some(mismatched),
                },
                operation,
                "a mismatched payload",
            );
            // No payload at all.
            assert_unsupported(
                &wire::TypeInfoGraphRequest {
                    schema_version: version,
                    operation: operation as i32,
                    payload: None,
                },
                operation,
                "no payload",
            );
        }
    }
}

/// The global schema check and the operation decode answer before the
/// served-operation check: an unserved operation at an unsupported version
/// is `UnknownSchemaVersion`, and an operation number outside the enum is
/// `InvalidMode`.
#[test]
fn refusal_order_is_schema_then_operation_then_unsupported() {
    for version in [1u32, TYPEINFO_GRAPH_SCHEMA_VERSION + 1] {
        let request = legacy_envelope(wire::Operation::Relate, version);
        let err = validate_type_info_graph_request(&request).unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "UnknownSchemaVersion",
            "an unserved operation at schema {version} is refused for its version first"
        );
    }
    let unknown_operation = wire::TypeInfoGraphRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: 99,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&unknown_operation).unwrap_err();
    assert_eq!(err_variant_label(&err), "InvalidMode");
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

/// A request span ref that is inverted, or names no file, is a typed
/// `MalformedPayload` before execution — never normalised or swapped — for
/// both span-carrying operations.
#[test]
fn span_carrying_requests_refuse_inverted_and_file_less_span_refs() {
    let span = |canonical_id: &str, start, end| {
        Some(wire::SpanRef {
            canonical_id: canonical_id.to_string(),
            start,
            end,
        })
    };
    for (label, span) in [
        ("inverted", span("/a.ts", 9, 4)),
        ("file-less", span("", 1, 4)),
    ] {
        let flow = wire::FlowNarrowingRequest {
            canonical_id: "/a.ts".to_string(),
            span: span.clone(),
            context: Some(default_context()),
            display_policy: Some(default_display_policy()),
            include_provenance: false,
            include_diagnostics: false,
        };
        let err = validate_flow_narrowing_request(&flow).unwrap_err();
        assert_eq!(err_variant_label(&err), "MalformedPayload", "flow {label}");
        let contextual = wire::ContextualTypeRequest {
            canonical_id: "/a.ts".to_string(),
            span,
            context: Some(default_context()),
            display_policy: Some(default_display_policy()),
            include_provenance: false,
            include_diagnostics: false,
        };
        let err = validate_contextual_type_request(&contextual).unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "contextual {label}"
        );
    }
    // An empty range is a valid zero-width position, not an inverted one.
    let empty = wire::FlowNarrowingRequest {
        canonical_id: "/a.ts".to_string(),
        span: span("/a.ts", 3, 3),
        context: Some(default_context()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    };
    validate_flow_narrowing_request(&empty).expect("a zero-width span is exact");
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
    validate_expand_graph_around_request(&expand_around_request_with(Some(well_shaped_node_ref())))
        .expect("a declaration-root reference must validate");
    let nested = wire::NodeRef {
        role_digest: vec![4; 32],
        ..well_shaped_node_ref()
    };
    validate_expand_graph_around_request(&expand_around_request_with(Some(nested)))
        .expect("a nested-role reference must validate");
}

/// The reference's shape is validated before the host's index is consulted:
/// a missing reference, a query, basis or slot digest of any length but 32,
/// a role digest neither empty nor 32 bytes, and the unsupported provenance
/// flag are each a MalformedPayload.
#[test]
fn expand_around_node_ref_shape_is_validated() {
    let mut malformed = vec![("missing", expand_around_request_with(None))];
    for (field, node_ref) in [
        (
            "query",
            wire::NodeRef {
                query_identity_digest: vec![1; 31],
                ..well_shaped_node_ref()
            },
        ),
        (
            "basis",
            wire::NodeRef {
                input_basis_digest: Vec::new(),
                ..well_shaped_node_ref()
            },
        ),
        (
            "slot",
            wire::NodeRef {
                decl_slot_digest: vec![3; 33],
                ..well_shaped_node_ref()
            },
        ),
        (
            "role",
            wire::NodeRef {
                role_digest: vec![4; 5],
                ..well_shaped_node_ref()
            },
        ),
    ] {
        malformed.push((field, expand_around_request_with(Some(node_ref))));
    }
    malformed.push((
        "provenance",
        wire::ExpandGraphAroundRequest {
            include_provenance: true,
            ..expand_around_request_with(Some(well_shaped_node_ref()))
        },
    ));
    for (case, request) in malformed {
        let err = validate_expand_graph_around_request(&request).unwrap_err();
        assert_eq!(err_variant_label(&err), "MalformedPayload", "{case}");
    }
}

/// Expand-around's floor is schema 12: every globally supported version
/// below it is a MalformedPayload for this operation, 12 validates.
#[test]
fn expand_around_operation_requires_schema_twelve() {
    assert_eq!(EXPAND_AROUND_MIN_SCHEMA_VERSION, 12);
    validate_type_info_graph_request(&legacy_envelope(
        wire::Operation::ExpandAround,
        EXPAND_AROUND_MIN_SCHEMA_VERSION,
    ))
    .expect("a v12 expand-around request must validate");
    for version in [MIN_TYPEINFO_GRAPH_SCHEMA_VERSION, 3, 10, 11] {
        let err = validate_type_info_graph_request(&legacy_envelope(
            wire::Operation::ExpandAround,
            version,
        ))
        .unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "a v{version} expand-around request is a MalformedPayload"
        );
    }
}

/// List-symbols' floor is schema 12: every globally supported version
/// below it is a MalformedPayload for this operation, 12 validates, and an
/// empty file path is a MissingProjectPath.
#[test]
fn list_symbols_below_its_schema_floor_is_malformed() {
    assert_eq!(LIST_SYMBOLS_MIN_SCHEMA_VERSION, 12);
    validate_type_info_graph_request(&legacy_envelope(
        wire::Operation::ListSymbols,
        LIST_SYMBOLS_MIN_SCHEMA_VERSION,
    ))
    .expect("a v12 list-symbols request must validate");
    for version in [MIN_TYPEINFO_GRAPH_SCHEMA_VERSION, 11] {
        let request = legacy_envelope(wire::Operation::ListSymbols, version);
        let err = validate_type_info_graph_request(&request).unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "a v{version} list-symbols request is a MalformedPayload"
        );
    }
    let mut empty = legacy_envelope(
        wire::Operation::ListSymbols,
        LIST_SYMBOLS_MIN_SCHEMA_VERSION,
    );
    empty.payload = Some(wire_request::Payload::ListSymbols(
        wire::ListSymbolsRequest {
            canonical_id: String::new(),
        },
    ));
    let err = validate_type_info_graph_request(&empty).unwrap_err();
    assert_eq!(err_variant_label(&err), "MissingProjectPath");
}

/// A well-shaped profile reference: a 32-byte digest. Validation checks the
/// shape only; admission is the registry's question.
fn well_shaped_profile() -> Option<wire::FrameworkProfileRef> {
    Some(wire::FrameworkProfileRef {
        digest: vec![0x11; wire::FRAMEWORK_PROFILE_DIGEST_LEN],
    })
}

fn framework_surface_request_with(
    framework_profile: Option<wire::FrameworkProfileRef>,
    schema_version: u32,
) -> wire::FrameworkSurfaceRequest {
    wire::FrameworkSurfaceRequest {
        selector: Some(wire::ComponentSelector {
            canonical_id: "/Foo.vue".to_string(),
            export_name: String::new(),
            has_export_name: false,
            framework_profile,
        }),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        schema_version,
        requested_kinds: vec![wire::FrameworkSurfaceKind::Props as i32],
    }
}

/// The facet demand is validated before execution: an empty, unknown or
/// duplicated demand is a typed `MalformedPayload` on every accepted
/// framework-surface schema. Discriminating: an executor that silently
/// widened an empty demand to every kind, or dropped a duplicate, would pass
/// these requests.
#[test]
fn framework_surface_requested_kinds_demand_is_validated_per_schema() {
    let floor = FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION;
    assert!(floor >= FRAMEWORK_SURFACE_REQUESTED_KINDS_MIN_SCHEMA_VERSION);
    let with_kinds = |schema_version: u32, kinds: Vec<i32>| {
        let mut request = framework_surface_request_with(well_shaped_profile(), schema_version);
        request.requested_kinds = kinds;
        request
    };
    let props = wire::FrameworkSurfaceKind::Props as i32;
    let emits = wire::FrameworkSurfaceKind::Emits as i32;

    validate_framework_surface_request(&with_kinds(floor, vec![emits, props]))
        .expect("a request naming known distinct kinds validates in any order");

    let rejected = [
        (floor, vec![], "empty demand"),
        (floor, vec![props, props], "duplicate kind"),
        (floor, vec![99], "unknown kind"),
    ];
    for (schema_version, kinds, why) in rejected {
        let err =
            validate_framework_surface_request(&with_kinds(schema_version, kinds)).expect_err(why);
        assert_eq!(err_variant_label(&err), "MalformedPayload", "{why}");
    }
}

/// The typed selector's profile reference is shape-checked before any
/// registry access: an absent reference or a digest that is not exactly 32
/// bytes is a `MalformedPayload`; a 32-byte digest passes validation (the
/// executor then asks the registry whether it is admitted).
#[test]
fn framework_surface_request_profile_reference_shape_is_validated() {
    let floor = FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION;
    let err = validate_framework_surface_request(&framework_surface_request_with(None, floor))
        .expect_err("a selector without framework_profile is rejected");
    assert_eq!(err_variant_label(&err), "MalformedPayload");
    for len in [0, 31, 33] {
        let request = framework_surface_request_with(
            Some(wire::FrameworkProfileRef {
                digest: vec![0x11; len],
            }),
            floor,
        );
        let err = validate_framework_surface_request(&request)
            .expect_err("a digest that is not 32 bytes is rejected");
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "a {len}-byte digest is a MalformedPayload"
        );
    }
}

#[test]
fn framework_surface_request_well_formed_passes() {
    let request =
        framework_surface_request_with(well_shaped_profile(), FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION);
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

/// Builds an envelope whose payload arm matches `operation` at the given
/// schema version. Relate has no graph payload arm, so it carries the
/// resolve-symbol arm.
fn legacy_envelope(operation: wire::Operation, schema_version: u32) -> wire::TypeInfoGraphRequest {
    let payload = match operation {
        wire::Operation::ResolveSymbol | wire::Operation::Relate => {
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
        wire::Operation::ExpandAround => wire_request::Payload::ExpandAround(
            expand_around_request_with(Some(well_shaped_node_ref())),
        ),
        wire::Operation::ListSymbols => {
            wire_request::Payload::ListSymbols(wire::ListSymbolsRequest {
                canonical_id: "/a.ts".to_string(),
            })
        }
        wire::Operation::FrameworkSurfaces => {
            panic!("framework-surface envelopes are built by framework_surface_envelope")
        }
    };
    wire::TypeInfoGraphRequest {
        schema_version,
        operation: operation as i32,
        payload: Some(payload),
    }
}

/// Resolve-symbol is the one served operation whose floor is the global
/// minimum: it validates from schema 2 through the current version. The
/// unserved operations never validate (`UNSERVED_OPERATIONS`), and their
/// floors stay pinned through the direct
/// `validate_schema_version_for_operation` walk below. Expand-around's
/// floor is 12 (`expand_around_operation_requires_schema_twelve`).
#[test]
fn resolve_symbol_accepts_the_minimum_and_current_schema() {
    for version in [
        MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
        TYPEINFO_GRAPH_SCHEMA_VERSION,
    ] {
        validate_type_info_graph_request(&legacy_envelope(wire::Operation::ResolveSymbol, version))
            .unwrap_or_else(|err| {
                panic!("resolve-symbol must accept schema {version}, got {err:?}")
            });
    }
}

#[test]
fn schema_versions_one_and_thirteen_are_rejected_with_typed_errors() {
    // 1 is below the floor; 13 is the first version ABOVE the current supported
    // set ([2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]). Both are outside the closed set and rejected.
    for version in [1u32, 13u32] {
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
            framework_surface_request_with(well_shaped_profile(), payload_version),
        )),
    }
}

// PROVISIONAL — OWNED BY block U8. The schema-3 acceptance pinned here is
// provisional: block U8 bumps `SemanticTypeGraph.schema_version` and retags
// `FrameworkSurfacePayload.graph` to `TypeInfoGraphPayload`, at which point U8
// re-versions this acceptance. Until then it pins the current wire surface; the
// specific version number is not the permanent contract.
#[test]
fn framework_surface_operation_accepts_schema_eleven_only() {
    validate_type_info_graph_request(&framework_surface_envelope(
        FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
        FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
    ))
    .expect("a v11 framework-surface request must validate");
    // Every globally supported version below the floor — including 10, the
    // facet-demand schema — is a MalformedPayload for this operation, even
    // with a well-shaped profile reference.
    for version in [3, 9, 10] {
        let err = validate_type_info_graph_request(&framework_surface_envelope(version, version))
            .unwrap_err();
        assert_eq!(
            err_variant_label(&err),
            "MalformedPayload",
            "a v{version} framework-surface request is a MalformedPayload"
        );
    }
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
        well_shaped_profile(),
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
fn supported_schema_version_set_is_exactly_two_through_twelve() {
    assert_eq!(
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
        &[
            MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            TYPEINFO_GRAPH_SCHEMA_VERSION
        ],
        "the supported set holds every version some operation still accepts: \
         schema 2 (legacy-operations-only), schema 3 (the framework-surface \
         floor), schema 4 (member default/origin fields), schema 5 \
         (payload-side relation_proofs table), schema 6 (typed property keys), \
         schema 7 (the canonical object-spread-program node), schema 8 (the \
         compiler-intrinsic application node), schema 9 (the stated UTF-8 byte \
         unit of every span), schema 10 (the framework-surface facet demand \
         `requested_kinds`), schema 11 (the typed \
         `ComponentSelector.framework_profile`) and schema 12 (current — \
         host-issued `NodeRef` positions)",
    );
    assert_eq!(MIN_TYPEINFO_GRAPH_SCHEMA_VERSION, 2);
    assert_eq!(TYPEINFO_GRAPH_SCHEMA_VERSION, 12);
    assert_eq!(EXPAND_AROUND_MIN_SCHEMA_VERSION, 12);
    assert_eq!(FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION, 11);
    assert_eq!(FRAMEWORK_SURFACE_REQUESTED_KINDS_MIN_SCHEMA_VERSION, 10);
}

/// The supported-set advertisement surface: the `UnknownSchemaVersion`
/// error payload's `server_supported_versions`, populated from
/// `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` via
/// `wire_error_unknown_schema_version` — the single advertisement
/// source. It reports `[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]`.
#[test]
fn unknown_schema_version_error_advertises_two_through_twelve() {
    // Version 13 is the first version OUTSIDE the supported set — it triggers
    // the UnknownSchemaVersion rejection (12 is now supported).
    let err =
        validate_type_info_graph_request(&legacy_envelope(wire::Operation::ResolveSymbol, 13))
            .unwrap_err();
    let payload = match err.kind.as_ref().expect("error kind") {
        type_info_request_error::Kind::UnknownSchemaVersion(p) => p,
        other => panic!("expected UnknownSchemaVersion, got {other:?}"),
    };
    assert_eq!(
        payload.server_supported_versions,
        vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "the advertisement surface must report exactly [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]",
    );
    assert_eq!(
        payload.server_supported_versions,
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.to_vec(),
        "the advertisement is fed by the supported-set constant — the single source",
    );

    // The constructor itself is the single advertisement source.
    let wire_payload = wire::wire_error_unknown_schema_version(
        13,
        TYPEINFO_GRAPH_SCHEMA_VERSION,
        SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
    );
    assert_eq!(
        wire_payload.server_supported_versions,
        vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    );
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
            | wire::Operation::FlowNarrowingAt
            | wire::Operation::ContextualTypeAt => MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
            wire::Operation::ExpandAround => EXPAND_AROUND_MIN_SCHEMA_VERSION,
            wire::Operation::FrameworkSurfaces => FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION,
            wire::Operation::ListSymbols => LIST_SYMBOLS_MIN_SCHEMA_VERSION,
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
        wire::Operation::ListSymbols,
    ];
    assert_eq!(
        EVERY_OPERATION.len(),
        9,
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
        // floor; 13 is the first version above the current set ([2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).
        for version in [1u32, 13u32] {
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
                value: None,
                optional_member: false,
                readonly: false,
                property_key: Some(wire::PropertyKeyExpr {
                    key: Some(
                        verter_protocol::verter::v1::property_key_expr::Key::StringValue(
                            "m".to_string(),
                        ),
                    ),
                }),
                member_kind: wire::ObjectMemberKind::Property as i32,
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
