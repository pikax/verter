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
    validate_resolve_symbol_graph_request, validate_type_info_graph_request,
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

#[test]
fn framework_surface_request_missing_adapter_id_fails() {
    let request = wire::FrameworkSurfaceRequest {
        selector: Some(wire::ComponentSelector {
            canonical_id: "/Foo.vue".to_string(),
            export_name: String::new(),
            has_export_name: false,
            framework_adapter_id: String::new(),
        }),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        schema_version: MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    };
    let err = validate_framework_surface_request(&request).unwrap_err();
    assert_eq!(err_variant_label(&err), "MalformedPayload");
}

#[test]
fn framework_surface_request_well_formed_passes() {
    let request = wire::FrameworkSurfaceRequest {
        selector: Some(wire::ComponentSelector {
            canonical_id: "/Foo.vue".to_string(),
            export_name: String::new(),
            has_export_name: false,
            framework_adapter_id: "vue".to_string(),
        }),
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        schema_version: MIN_TYPEINFO_GRAPH_SCHEMA_VERSION,
    };
    validate_framework_surface_request(&request)
        .expect("a well-formed framework surface request must validate");
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
