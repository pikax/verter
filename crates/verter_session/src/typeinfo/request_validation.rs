//! Request validation for the typeinfo graph entry-points.
//!
//! Every typeinfo entry-point dispatches a `TypeInfoGraphRequest`
//! through the audit-emission boundary. The semantic execution layer
//! must NOT see a malformed request — validation runs FIRST, returns
//! a typed [`TypeInfoRequestError`] if the request is structurally
//! incomplete, and only valid requests reach
//! `SemanticGraphStore::execute`.
//!
//! The validation surface is shape-only:
//!
//! - required-field presence (schema version, mode, demand, display
//!   policy, closure policy, project path);
//! - bounded numeric ranges (expansion budget within `0..=MAX_*`);
//! - structural well-formedness of the carried
//!   `StructuredTypeExpression` (no detectable infinite cycle, no
//!   missing required sub-payloads);
//! - operation-kind special cases (`relate` requires both endpoints,
//!   `listSymbols` ignores the expression payload).
//!
//! No semantic execution. No resolver invocation. No file IO.
//! Producers downstream of this module can assume the request is
//! structurally well-formed.
//!
//! Caller integration: every `_with_audit` entry-point on the typeinfo
//! session calls `validate_*_request(&request)?` BEFORE running the
//! semantic query. A failed validation emits a
//! `RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayload::from_validation_error(op))`
//! envelope so observability captures the rejection without
//! re-encoding the wire error twice.

#![deny(missing_docs)]

use std::collections::HashSet;

use verter_protocol::typeinfo::graph::{
    self as wire, ClosurePolicy, ContextualTypeRequest, DisplayPolicy,
    EvaluateTypeExpressionGraphRequest, ExpandGraphAroundRequest, FlowNarrowingRequest,
    FrameworkSurfaceRequest, Operation as WireOperation, ProjectPathGraphRequest, ProjectionMode,
    ProjectionReductionContext, ReductionDemand, ResolveSymbolGraphRequest,
    StructuredTypeExpression, TypeInfoGraphRequest, TypeInfoRequestError,
    TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use verter_protocol::verter::v1::{
    graph_closure_policy, structured_type_expression as wire_expr,
    type_info_graph_request as wire_request, type_info_request_error,
};

/// Upper bound on the expansion-policy `node_budget` an
/// `EvaluateTypeExpressionGraphRequest` / `ResolveSymbolGraphRequest`
/// closure can request. Producers exceeding this cap receive
/// [`TypeInfoRequestError::ExpansionBudgetOutOfRange`] before any
/// semantic execution.
pub const MAX_EXPANSION_NODE_BUDGET: u32 = 1 << 14; // 16384

/// Upper bound on the expansion-policy `depth_budget`. Same contract
/// as [`MAX_EXPANSION_NODE_BUDGET`].
pub const MAX_EXPANSION_DEPTH_BUDGET: u32 = 256;

/// Minimum schema version this validator accepts. Producers sending
/// older payloads receive
/// [`TypeInfoRequestError::UnknownSchemaVersion`].
///
/// Bumped to 2 when the env-LESS `GraphDeclSlotRef` roots carrier was
/// retired: a v1 payload's roots rode the env-less slot (tag 2, now
/// reserved), which lacks `whole_hash`, so a v1 envelope decoded by a
/// v2 server would silently drop its roots. Unknown-field wire-compat
/// is NOT semantic compat — the closed-set validator rejects v1 rather
/// than admit a payload whose roots cannot be recovered.
pub const MIN_TYPEINFO_GRAPH_SCHEMA_VERSION: u32 = 2;

/// Closed set of schema versions this validator accepts. Any wire
/// version outside this set (below MIN or above the current server
/// version) is rejected with
/// [`TypeInfoRequestError::UnknownSchemaVersion`].
///
/// The set is single-sourced from
/// [`TYPEINFO_GRAPH_SCHEMA_VERSION`]: bumping the server schema
/// adds the new version here and removes obsolete ones in the same
/// commit. A future-version request (e.g. a newer client talking to
/// an older server) MUST fail validation rather than reach
/// dispatch, because the dispatcher's payload arms are only
/// type-checked against the supported set; an unrecognised version
/// silently passing through would let a malformed payload reach
/// semantic execution.
pub const SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS: &[u32] = &[TYPEINFO_GRAPH_SCHEMA_VERSION];

/// Maximum recursion depth allowed when walking a
/// [`StructuredTypeExpression`] tree during shape validation. Trees
/// deeper than this cap fail with
/// [`TypeInfoRequestError::MalformedStructuredExpression`].
pub const MAX_STRUCTURED_EXPRESSION_DEPTH: u32 = 256;

/// Validated wrapper around a typeinfo graph request. Producers
/// consume `ValidatedTypeInfoGraphRequest` so the static type
/// system enforces that semantic execution runs only against a
/// validated payload.
#[derive(Debug, Clone)]
pub struct ValidatedTypeInfoGraphRequest {
    request: TypeInfoGraphRequest,
}

impl ValidatedTypeInfoGraphRequest {
    /// Borrow the validated request.
    #[must_use]
    pub fn inner(&self) -> &TypeInfoGraphRequest {
        &self.request
    }

    /// Consume the wrapper, returning the original request.
    #[must_use]
    pub fn into_inner(self) -> TypeInfoGraphRequest {
        self.request
    }
}

/// Validate a [`TypeInfoGraphRequest`] envelope. Returns a typed
/// `ValidatedTypeInfoGraphRequest` on success or a closed
/// `TypeInfoRequestError` on rejection.
///
/// # Errors
///
/// Returns a typed `TypeInfoRequestError` when the request is missing
/// a required field, carries an out-of-range expansion budget, has
/// a malformed `StructuredTypeExpression`, omits a required path /
/// span / endpoint, or carries an unsupported schema version.
pub fn validate_type_info_graph_request(
    request: &TypeInfoGraphRequest,
) -> Result<ValidatedTypeInfoGraphRequest, TypeInfoRequestError> {
    validate_schema_version(request.schema_version)?;
    let operation = decode_operation(request.operation).ok_or_else(invalid_mode_error)?;
    let payload = request
        .payload
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing payload"))?;

    // Operation discriminator MUST match the payload's variant tag.
    match (operation, payload) {
        (WireOperation::ResolveSymbol, wire_request::Payload::ResolveSymbol(r)) => {
            validate_resolve_symbol_graph_request(r)?;
        }
        (WireOperation::EvaluateExpression, wire_request::Payload::EvaluateTypeExpression(r)) => {
            validate_evaluate_type_expression_graph_request(r)?;
        }
        (WireOperation::ProjectPath, wire_request::Payload::ProjectPath(r)) => {
            validate_project_path_graph_request(r)?;
        }
        (WireOperation::FlowNarrowingAt, wire_request::Payload::FlowNarrowing(r)) => {
            validate_flow_narrowing_request(r)?;
        }
        (WireOperation::ContextualTypeAt, wire_request::Payload::ContextualType(r)) => {
            validate_contextual_type_request(r)?;
        }
        (WireOperation::ExpandAround, wire_request::Payload::ExpandAround(r)) => {
            validate_expand_graph_around_request(r)?;
        }
        (WireOperation::FrameworkSurfaces, wire_request::Payload::FrameworkSurface(r)) => {
            validate_framework_surface_request(r)?;
        }
        (WireOperation::Relate, _) | (_, _) => {
            // Relate has a dedicated request shape outside the graph
            // envelope; receiving a `Relate` operation through this
            // path is a payload mismatch. Other (op, payload)
            // discriminator mismatches are equally malformed.
            return Err(malformed_payload_error_with_detail(
                "operation discriminator does not match payload arm",
            )());
        }
    }

    Ok(ValidatedTypeInfoGraphRequest {
        request: request.clone(),
    })
}

/// Validate a `ResolveSymbolGraphRequest`. See module docs for
/// contract; mirrors the same checks the envelope dispatcher would
/// perform.
///
/// # Errors
///
/// Returns the same closed [`TypeInfoRequestError`] set as
/// [`validate_type_info_graph_request`].
pub fn validate_resolve_symbol_graph_request(
    request: &ResolveSymbolGraphRequest,
) -> Result<(), TypeInfoRequestError> {
    if request.canonical_id.is_empty() {
        return Err(missing_project_path_error());
    }
    if request.name.is_empty() {
        return Err(malformed_payload_error_with_detail("missing symbol name")());
    }
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_closure_policy(request.closure.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    Ok(())
}

/// Validate an `EvaluateTypeExpressionGraphRequest`.
///
/// # Errors
///
/// As [`validate_resolve_symbol_graph_request`], plus structural
/// checks against the carried `StructuredTypeExpression`.
pub fn validate_evaluate_type_expression_graph_request(
    request: &EvaluateTypeExpressionGraphRequest,
) -> Result<(), TypeInfoRequestError> {
    if request.scope_canonical.is_empty() {
        return Err(missing_project_path_error());
    }
    let expression = request.expression.as_ref().ok_or_else(
        malformed_structured_expression_error_with_detail("missing expression"),
    )?;
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_closure_policy(request.closure.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    validate_structured_expression(expression, 0)?;
    Ok(())
}

/// Validate a `ProjectPathGraphRequest`.
///
/// # Errors
///
/// Includes the path-empty check on top of the resolve-symbol checks.
pub fn validate_project_path_graph_request(
    request: &ProjectPathGraphRequest,
) -> Result<(), TypeInfoRequestError> {
    if request.canonical_id.is_empty() {
        return Err(missing_project_path_error());
    }
    if request.name.is_empty() {
        return Err(malformed_payload_error_with_detail("missing symbol name")());
    }
    if request.path.is_empty() {
        return Err(missing_project_path_error());
    }
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_closure_policy(request.closure.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    Ok(())
}

/// Validate a `FlowNarrowingRequest`. Closure is implicit
/// (projection-required `flow_narrowing`), so the closure field is
/// not present in the wire request.
///
/// # Errors
///
/// As [`validate_resolve_symbol_graph_request`].
pub fn validate_flow_narrowing_request(
    request: &FlowNarrowingRequest,
) -> Result<(), TypeInfoRequestError> {
    if request.canonical_id.is_empty() {
        return Err(missing_project_path_error());
    }
    let _span = request
        .span
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing span"))?;
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    Ok(())
}

/// Validate a `ContextualTypeRequest`. Same shape as
/// [`FlowNarrowingRequest`].
///
/// # Errors
///
/// As [`validate_flow_narrowing_request`].
pub fn validate_contextual_type_request(
    request: &ContextualTypeRequest,
) -> Result<(), TypeInfoRequestError> {
    if request.canonical_id.is_empty() {
        return Err(missing_project_path_error());
    }
    let _span = request
        .span
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing span"))?;
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    Ok(())
}

/// Validate an `ExpandGraphAroundRequest`.
///
/// # Errors
///
/// As [`validate_resolve_symbol_graph_request`].
pub fn validate_expand_graph_around_request(
    request: &ExpandGraphAroundRequest,
) -> Result<(), TypeInfoRequestError> {
    let _parent = request
        .parent_graph
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing parent graph"))?;
    let _target = request
        .target
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing target node"))?;
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_closure_policy(request.closure.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    Ok(())
}

/// Validate a `FrameworkSurfaceRequest`.
///
/// # Errors
///
/// As [`validate_resolve_symbol_graph_request`], plus selector-
/// presence and schema-version cross-check.
pub fn validate_framework_surface_request(
    request: &FrameworkSurfaceRequest,
) -> Result<(), TypeInfoRequestError> {
    let selector = request
        .selector
        .as_ref()
        .ok_or_else(malformed_payload_error_with_detail("missing selector"))?;
    if selector.canonical_id.is_empty() {
        return Err(missing_project_path_error());
    }
    if selector.framework_adapter_id.is_empty() {
        return Err(malformed_payload_error_with_detail(
            "framework_adapter_id is required",
        )());
    }
    validate_projection_reduction_context(request.context.as_ref())?;
    validate_closure_policy(request.closure.as_ref())?;
    validate_display_policy(request.display_policy.as_ref())?;
    validate_schema_version(request.schema_version)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared validators
// ---------------------------------------------------------------------------

fn validate_schema_version(version: u32) -> Result<(), TypeInfoRequestError> {
    if !SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.contains(&version) {
        return Err(unknown_schema_version_error(version));
    }
    Ok(())
}

fn validate_projection_reduction_context(
    ctx: Option<&ProjectionReductionContext>,
) -> Result<(), TypeInfoRequestError> {
    let ctx = ctx.ok_or_else(missing_projection_context_error)?;
    decode_projection_mode(ctx.mode).ok_or_else(invalid_mode_error)?;
    decode_reduction_demand(ctx.demand)
        .ok_or_else(|| malformed_payload_error_with_detail("invalid reduction demand")())?;
    Ok(())
}

fn validate_closure_policy(policy: Option<&ClosurePolicy>) -> Result<(), TypeInfoRequestError> {
    let policy = policy.ok_or_else(missing_closure_policy_error)?;
    let kind = policy
        .kind
        .as_ref()
        .ok_or_else(missing_closure_policy_error)?;
    if let graph_closure_policy::Kind::Expanded(expanded) = kind {
        if expanded.node_budget > MAX_EXPANSION_NODE_BUDGET
            || expanded.depth_budget > MAX_EXPANSION_DEPTH_BUDGET
            || (expanded.node_budget == 0 && expanded.depth_budget == 0)
        {
            return Err(expansion_budget_out_of_range_error(
                expanded.node_budget,
                expanded.depth_budget,
            ));
        }
    }
    Ok(())
}

fn validate_display_policy(policy: Option<&DisplayPolicy>) -> Result<(), TypeInfoRequestError> {
    let _policy = policy.ok_or_else(missing_display_policy_error)?;
    Ok(())
}

fn validate_structured_expression(
    expression: &StructuredTypeExpression,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if depth > MAX_STRUCTURED_EXPRESSION_DEPTH {
        return Err(malformed_structured_expression_error_with_detail(
            "structured expression exceeds maximum depth",
        )());
    }
    let kind =
        expression
            .kind
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "expression kind is missing",
            ))?;

    // EXHAUSTIVE dispatch over every `StructuredTypeExpression` arm.
    // The match is on the outer enum (not on payload fields) so the
    // compiler's exhaustiveness check fires when a new variant is
    // added to the proto schema. Each field-bearing arm calls a
    // dedicated `validate_*` helper that checks its required nested
    // sub-payloads; leaf arms (literal / primitive / typeof-expr /
    // this-type / unique-symbol) pass trivially.
    match kind {
        wire_expr::Kind::Reference(r) => validate_reference_expr(r, depth)?,
        wire_expr::Kind::Union(u) => validate_union_expr(u, depth)?,
        wire_expr::Kind::Intersection(i) => validate_intersection_expr(i, depth)?,
        wire_expr::Kind::IndexedAccess(boxed) => validate_indexed_access_expr(boxed, depth)?,
        wire_expr::Kind::Keyof(boxed) => validate_keyof_expr(boxed, depth)?,
        wire_expr::Kind::TypeofExpr(t) => validate_typeof_expr(t)?,
        wire_expr::Kind::Tuple(t) => validate_tuple_expr(t, depth)?,
        wire_expr::Kind::Array(boxed) => validate_array_expr(boxed, depth)?,
        wire_expr::Kind::ObjectLiteral(obj) => validate_object_literal_expr(obj, depth)?,
        wire_expr::Kind::Mapped(boxed) => validate_mapped_expr(boxed, depth)?,
        wire_expr::Kind::Conditional(boxed) => validate_conditional_expr(boxed, depth)?,
        wire_expr::Kind::Literal(_) => {}
        wire_expr::Kind::Primitive(_) => {}
        wire_expr::Kind::TemplateLiteral(t) => validate_template_literal_expr(t, depth)?,
        wire_expr::Kind::InferExpr(boxed) => validate_infer_expr(boxed, depth)?,
        wire_expr::Kind::FunctionExpr(boxed) => validate_function_expr(boxed, depth)?,
        wire_expr::Kind::ClassExpr(c) => validate_class_expr(c, depth)?,
        wire_expr::Kind::ThisType(_) => {}
        wire_expr::Kind::SatisfiesExpr(boxed) => validate_satisfies_expr(boxed, depth)?,
        wire_expr::Kind::UniqueSymbol(u) => validate_unique_symbol_expr(u)?,
        wire_expr::Kind::NoInfer(boxed) => validate_no_infer_expr(boxed, depth)?,
        wire_expr::Kind::LocalTypeRef(r) => validate_local_type_ref_expr(r)?,
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-variant structured-expression validators. Each helper checks
// the variant's required nested fields and recurses into its
// structured sub-trees. Leaf variants (Literal / Primitive /
// ThisType) have no helper because they carry no nested expression.
// ---------------------------------------------------------------------------

fn validate_reference_expr(
    expr: &wire::ExprReference,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if expr.name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "reference expression missing name",
        )());
    }
    for arg in &expr.type_arguments {
        validate_structured_expression(arg, depth + 1)?;
    }
    Ok(())
}

fn validate_union_expr(expr: &wire::ExprUnion, depth: u32) -> Result<(), TypeInfoRequestError> {
    for member in &expr.members {
        validate_structured_expression(member, depth + 1)?;
    }
    Ok(())
}

fn validate_intersection_expr(
    expr: &wire::ExprIntersection,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    for member in &expr.members {
        validate_structured_expression(member, depth + 1)?;
    }
    Ok(())
}

fn validate_indexed_access_expr(
    expr: &wire::ExprIndexedAccess,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let object =
        expr.object
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "indexed access missing object",
            ))?;
    let index =
        expr.index
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "indexed access missing index",
            ))?;
    validate_structured_expression(object, depth + 1)?;
    validate_structured_expression(index, depth + 1)?;
    Ok(())
}

fn validate_keyof_expr(expr: &wire::ExprKeyOf, depth: u32) -> Result<(), TypeInfoRequestError> {
    let operand =
        expr.operand
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "keyof missing operand",
            ))?;
    validate_structured_expression(operand, depth + 1)?;
    Ok(())
}

fn validate_typeof_expr(expr: &wire::ExprTypeOf) -> Result<(), TypeInfoRequestError> {
    if expr.value_root_canonical.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "typeof expression missing value_root_canonical",
        )());
    }
    Ok(())
}

fn validate_tuple_expr(expr: &wire::ExprTuple, depth: u32) -> Result<(), TypeInfoRequestError> {
    for element in &expr.elements {
        let v = element.value.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail("tuple element missing value"),
        )?;
        validate_structured_expression(v, depth + 1)?;
    }
    Ok(())
}

fn validate_array_expr(expr: &wire::ExprArray, depth: u32) -> Result<(), TypeInfoRequestError> {
    let element =
        expr.element
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "array missing element",
            ))?;
    validate_structured_expression(element, depth + 1)?;
    Ok(())
}

fn validate_object_literal_expr(
    expr: &wire::ExprObject,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let mut seen: HashSet<&str> = HashSet::new();
    for member in &expr.members {
        if !seen.insert(member.name.as_str()) {
            return Err(malformed_structured_expression_error_with_detail(
                "duplicate object member name",
            )());
        }
        let v =
            member
                .value
                .as_ref()
                .ok_or_else(malformed_structured_expression_error_with_detail(
                    "object member missing value",
                ))?;
        validate_structured_expression(v, depth + 1)?;
    }
    for index_signature in &expr.index_signatures {
        validate_object_index_signature(index_signature, depth)?;
    }
    for call_signature in &expr.call_signatures {
        validate_object_call_signature(call_signature, depth)?;
    }
    for construct_signature in &expr.construct_signatures {
        validate_object_call_signature(construct_signature, depth)?;
    }
    Ok(())
}

fn validate_object_index_signature(
    sig: &wire::IndexSignatureExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let value =
        sig.value
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "object index signature missing value",
            ))?;
    validate_structured_expression(value, depth + 1)?;
    Ok(())
}

fn validate_object_call_signature(
    sig: &wire::ExprFunction,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    // Call / construct signatures share the function-expression shape;
    // delegate to the same well-formedness check so nested params and
    // return type get exercised.
    validate_function_shape(sig, depth)
}

fn validate_mapped_expr(expr: &wire::ExprMapped, depth: u32) -> Result<(), TypeInfoRequestError> {
    let type_param =
        expr.type_param
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "mapped missing type_param",
            ))?;
    validate_mapped_type_param(type_param, depth)?;
    let value =
        expr.value_type
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "mapped missing value type",
            ))?;
    validate_structured_expression(value, depth + 1)?;
    if expr.has_name_remap {
        let remap = expr.name_remap.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "mapped has_name_remap=true but name_remap missing",
            ),
        )?;
        validate_structured_expression(remap, depth + 1)?;
    }
    Ok(())
}

fn validate_mapped_type_param(
    type_param: &wire::MappedTypeParamExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if type_param.binder_id.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "mapped type_param missing binder_id",
        )());
    }
    if type_param.name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "mapped type_param missing name",
        )());
    }
    if let Some(constraint) = type_param.constraint.as_ref() {
        validate_structured_expression(constraint, depth + 1)?;
    }
    Ok(())
}

fn validate_conditional_expr(
    expr: &wire::ExprConditional,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let check =
        expr.check
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "conditional missing check",
            ))?;
    let extends_type = expr.extends_type.as_ref().ok_or_else(
        malformed_structured_expression_error_with_detail("conditional missing extends"),
    )?;
    let true_branch =
        expr.true_branch
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "conditional missing true branch",
            ))?;
    let false_branch = expr.false_branch.as_ref().ok_or_else(
        malformed_structured_expression_error_with_detail("conditional missing false branch"),
    )?;
    validate_structured_expression(check, depth + 1)?;
    validate_structured_expression(extends_type, depth + 1)?;
    validate_structured_expression(true_branch, depth + 1)?;
    validate_structured_expression(false_branch, depth + 1)?;
    Ok(())
}

fn validate_template_literal_expr(
    expr: &wire::ExprTemplateLiteral,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    for sub_expr in &expr.expressions {
        validate_structured_expression(sub_expr, depth + 1)?;
    }
    Ok(())
}

fn validate_infer_expr(expr: &wire::ExprInfer, depth: u32) -> Result<(), TypeInfoRequestError> {
    if expr.name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "infer expression missing name",
        )());
    }
    if expr.has_constraint {
        let constraint = expr.constraint.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "infer has_constraint=true but constraint missing",
            ),
        )?;
        validate_structured_expression(constraint, depth + 1)?;
    }
    Ok(())
}

fn validate_function_expr(
    expr: &wire::ExprFunction,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    validate_function_shape(expr, depth)
}

fn validate_function_shape(
    expr: &wire::ExprFunction,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    // Function-shaped messages (top-level FunctionExpr + object
    // call/construct signatures) share this contract: parameters must
    // each carry a well-formed type, the return slot must be present,
    // and every declared type parameter must be well-formed.
    for tp in &expr.type_parameters {
        validate_type_parameter(tp, depth)?;
    }
    if expr.has_this_param {
        let this_param = expr.this_param.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "function has_this_param=true but this_param missing",
            ),
        )?;
        validate_function_parameter(this_param, depth)?;
    }
    for param in &expr.parameters {
        validate_function_parameter(param, depth)?;
    }
    let return_expr =
        expr.return_expr
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "function missing return_expr",
            ))?;
    validate_function_return_expr(return_expr, depth)?;
    Ok(())
}

fn validate_function_parameter(
    param: &wire::FunctionParameterExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let type_ref =
        param
            .type_ref
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "function parameter missing type_ref",
            ))?;
    validate_structured_expression(type_ref, depth + 1)?;
    Ok(())
}

fn validate_function_return_expr(
    return_expr: &wire::FunctionReturnExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    use verter_protocol::verter::v1::function_return_expr;

    let kind =
        return_expr
            .kind
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "function return missing kind",
            ))?;
    match kind {
        function_return_expr::Kind::Type(ty) => {
            validate_structured_expression(ty, depth + 1)?;
        }
        function_return_expr::Kind::Predicate(predicate) => {
            validate_type_predicate_expr(predicate, depth)?;
        }
        function_return_expr::Kind::Assertion(assertion) => {
            validate_assertion_effect_expr(assertion, depth)?;
        }
    }
    Ok(())
}

/// Deep-validate a [`TypePredicateExpr`] inside a `FunctionReturnExpr`.
///
/// The predicate must carry a `parameter` (subject) — without it the
/// `value is T`/`asserts value is T` form has no anchor. The subject
/// itself is a closed `oneof` and must dispatch to a known variant.
/// The carried `predicate_type` continues through
/// [`validate_structured_expression`] like any other nested type.
fn validate_type_predicate_expr(
    predicate: &wire::TypePredicateExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let parameter = predicate.parameter.as_ref().ok_or_else(
        malformed_structured_expression_error_with_detail("type predicate missing parameter"),
    )?;
    validate_predicate_subject(parameter)?;
    let predicate_type = predicate.predicate_type.as_ref().ok_or_else(
        malformed_structured_expression_error_with_detail("type predicate missing predicate_type"),
    )?;
    validate_structured_expression(predicate_type, depth + 1)?;
    Ok(())
}

/// Closed-enum dispatch on a structured-expression [`PredicateSubject`]:
/// the `kind` oneof must be `Some(Identifier)` or `Some(ThisSubject)`.
/// Both arms are shape-closed at the proto layer; the identifier name
/// is a `name_id` table reference rather than a string so there is no
/// further leaf check beyond presence of the discriminator. Note this
/// is the structured-expression `PredicateSubject`
/// (`verter::v1::PredicateSubject`) and is intentionally distinct from
/// the graph-typed `GraphPredicateSubject` re-exported as
/// `wire::PredicateSubject`.
fn validate_predicate_subject(
    subject: &verter_protocol::verter::v1::PredicateSubject,
) -> Result<(), TypeInfoRequestError> {
    use verter_protocol::verter::v1::predicate_subject;

    let kind =
        subject
            .kind
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "type predicate subject missing kind",
            ))?;
    match kind {
        predicate_subject::Kind::Identifier(_) | predicate_subject::Kind::ThisSubject(_) => Ok(()),
    }
}

/// Deep-validate an [`AssertionEffectExpr`]. The `kind` oneof MUST be
/// set (closed discriminator). The identifier and this-assert arms
/// carry a `has_predicate` flag plus an optional `predicate` payload:
/// if the flag claims a predicate, the payload must be present and
/// well-formed. The condition arm is unit-shaped.
fn validate_assertion_effect_expr(
    assertion: &wire::AssertionEffectExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    use verter_protocol::verter::v1::assertion_effect_expr;

    let kind =
        assertion
            .kind
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "assertion effect missing kind",
            ))?;
    match kind {
        assertion_effect_expr::Kind::Identifier(identifier) => {
            validate_assertion_effect_identifier(identifier, depth)?;
        }
        assertion_effect_expr::Kind::ThisAssert(this_assert) => {
            validate_assertion_effect_this(this_assert, depth)?;
        }
        assertion_effect_expr::Kind::Condition(_) => {
            // Unit-shaped condition variant; the discriminator alone
            // closes the shape.
        }
    }
    Ok(())
}

/// Validate the identifier arm of an [`AssertionEffectExpr`]: if
/// `has_predicate` claims a predicate is present, the `predicate`
/// payload must be `Some(_)` AND its body must validate as a
/// well-formed structured expression. If the flag is false, the
/// payload must be absent (the boolean is the single source of truth).
fn validate_assertion_effect_identifier(
    identifier: &wire::AssertionEffectIdentifier,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if identifier.has_predicate {
        let predicate = identifier.predicate.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "assertion effect identifier has_predicate=true but predicate missing",
            ),
        )?;
        validate_structured_expression(predicate, depth + 1)?;
    } else if identifier.predicate.is_some() {
        return Err(malformed_structured_expression_error_with_detail(
            "assertion effect identifier has_predicate=false but predicate present",
        )());
    }
    Ok(())
}

/// Validate the `this`-assert arm of an [`AssertionEffectExpr`]: same
/// `has_predicate` ↔ `predicate.is_some()` invariant as the identifier
/// arm.
fn validate_assertion_effect_this(
    this_assert: &wire::AssertionEffectThis,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if this_assert.has_predicate {
        let predicate = this_assert.predicate.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "assertion effect this has_predicate=true but predicate missing",
            ),
        )?;
        validate_structured_expression(predicate, depth + 1)?;
    } else if this_assert.predicate.is_some() {
        return Err(malformed_structured_expression_error_with_detail(
            "assertion effect this has_predicate=false but predicate present",
        )());
    }
    Ok(())
}

fn validate_type_parameter(
    tp: &wire::TypeParameterExpr,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    if tp.name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "type parameter missing name",
        )());
    }
    if tp.has_constraint {
        let constraint = tp.constraint.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "type parameter has_constraint=true but constraint missing",
            ),
        )?;
        validate_structured_expression(constraint, depth + 1)?;
    }
    if tp.has_default {
        let default_type = tp.default_type.as_ref().ok_or_else(
            malformed_structured_expression_error_with_detail(
                "type parameter has_default=true but default_type missing",
            ),
        )?;
        validate_structured_expression(default_type, depth + 1)?;
    }
    Ok(())
}

fn validate_class_expr(expr: &wire::ExprClass, depth: u32) -> Result<(), TypeInfoRequestError> {
    if expr.has_class_name && expr.class_name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "class expression has_class_name=true but class_name missing",
        )());
    }
    for tp in &expr.type_parameters {
        validate_type_parameter(tp, depth)?;
    }
    for member in expr
        .instance_members
        .iter()
        .chain(expr.static_members.iter())
    {
        let v =
            member
                .value
                .as_ref()
                .ok_or_else(malformed_structured_expression_error_with_detail(
                    "class member missing value",
                ))?;
        validate_structured_expression(v, depth + 1)?;
    }
    Ok(())
}

fn validate_satisfies_expr(
    expr: &wire::ExprSatisfies,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let value =
        expr.value
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "satisfies missing value",
            ))?;
    let constraint =
        expr.constraint
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "satisfies missing constraint",
            ))?;
    validate_structured_expression(value, depth + 1)?;
    validate_structured_expression(constraint, depth + 1)?;
    Ok(())
}

fn validate_unique_symbol_expr(expr: &wire::ExprUniqueSymbol) -> Result<(), TypeInfoRequestError> {
    if expr.decl_canonical.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "unique symbol missing decl_canonical",
        )());
    }
    if expr.name.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "unique symbol missing name",
        )());
    }
    Ok(())
}

fn validate_no_infer_expr(
    expr: &wire::ExprNoInfer,
    depth: u32,
) -> Result<(), TypeInfoRequestError> {
    let inner =
        expr.inner
            .as_ref()
            .ok_or_else(malformed_structured_expression_error_with_detail(
                "noInfer missing inner",
            ))?;
    validate_structured_expression(inner, depth + 1)?;
    Ok(())
}

fn validate_local_type_ref_expr(expr: &wire::ExprLocalTypeRef) -> Result<(), TypeInfoRequestError> {
    if expr.binder_id.is_empty() {
        return Err(malformed_structured_expression_error_with_detail(
            "local type ref missing binder id",
        )());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire enum decoders
// ---------------------------------------------------------------------------

fn decode_operation(value: i32) -> Option<WireOperation> {
    match value {
        0 => Some(WireOperation::ResolveSymbol),
        1 => Some(WireOperation::EvaluateExpression),
        2 => Some(WireOperation::ProjectPath),
        3 => Some(WireOperation::Relate),
        4 => Some(WireOperation::FrameworkSurfaces),
        5 => Some(WireOperation::ExpandAround),
        6 => Some(WireOperation::FlowNarrowingAt),
        7 => Some(WireOperation::ContextualTypeAt),
        _ => None,
    }
}

fn decode_projection_mode(value: i32) -> Option<ProjectionMode> {
    match value {
        0 => Some(ProjectionMode::Identity),
        1 => Some(ProjectionMode::Navigate),
        2 => Some(ProjectionMode::Shallow),
        3 => Some(ProjectionMode::Expanded),
        4 => Some(ProjectionMode::Skeleton),
        _ => None,
    }
}

fn decode_reduction_demand(value: i32) -> Option<ReductionDemand> {
    match value {
        0 => Some(ReductionDemand::Published),
        1 => Some(ReductionDemand::StructuralTransit),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Error constructors
// ---------------------------------------------------------------------------

fn missing_projection_context_error() -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MissingProjectionContext(
            wire::wire_error_missing_projection_context(),
        )),
    }
}

fn missing_display_policy_error() -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MissingDisplayPolicy(
            wire::wire_error_missing_display_policy(),
        )),
    }
}

fn invalid_mode_error() -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::InvalidMode(
            wire::wire_error_invalid_mode(""),
        )),
    }
}

fn missing_closure_policy_error() -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MissingClosurePolicy(
            wire::wire_error_missing_closure_policy(),
        )),
    }
}

fn unknown_schema_version_error(wire_version: u32) -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::UnknownSchemaVersion(
            wire::wire_error_unknown_schema_version(
                wire_version,
                TYPEINFO_GRAPH_SCHEMA_VERSION,
                SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
            ),
        )),
    }
}

fn missing_project_path_error() -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MissingProjectPath(
            wire::wire_error_missing_project_path(),
        )),
    }
}

fn expansion_budget_out_of_range_error(
    node_budget: u32,
    depth_budget: u32,
) -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::ExpansionBudgetOutOfRange(
            wire::wire_error_expansion_budget_out_of_range(
                node_budget,
                depth_budget,
                MAX_EXPANSION_NODE_BUDGET,
                MAX_EXPANSION_DEPTH_BUDGET,
            ),
        )),
    }
}

fn malformed_payload_error_with_detail(detail: &'static str) -> impl Fn() -> TypeInfoRequestError {
    move || TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MalformedPayload(
            wire::wire_error_malformed_payload(detail),
        )),
    }
}

fn malformed_structured_expression_error_with_detail(
    detail: &'static str,
) -> impl Fn() -> TypeInfoRequestError {
    move || TypeInfoRequestError {
        kind: Some(
            type_info_request_error::Kind::MalformedStructuredExpression(
                wire::wire_error_malformed_structured_expression(detail),
            ),
        ),
    }
}
