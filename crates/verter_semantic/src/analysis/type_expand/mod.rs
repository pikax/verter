//! Type expansion — now fully backed by the shared semantic dispatch layer.
//!
//! The standalone arena solver has been retired. All expansion goes through
//! `ProjectSemanticDispatch::execute`; the helpers here only convert solver
//! result metadata into the public expansion contract.

mod request;

pub use request::{
    ExpandedCallSignature, ExpandedComponentTypes, ExpandedField, ExpandedIndexSignature,
    ExpandedMacroObjectShape, ExpandedMacroProps, ExpandedNormalizedExpr, ExpandedObjectShape,
    ExpandedParameter, ExpandedProperty, ExpansionDiagnostic, ExpansionExactness,
    ExpansionExecutionStatus, ExpansionMetadata, ExpansionResult, ExpansionStopReason,
};

use crate::analysis::type_solver::result::{IncompleteReason, SolverDiagnostic, SolverResult};
use verter_type_expr::{ObjectMember, PrimitiveName, TypeExpr};

// ---------------------------------------------------------------------------
// Shared solver-result → expansion-result conversion
// ---------------------------------------------------------------------------

/// Convert a single `IncompleteReason` to an `ExpansionDiagnostic`.
fn incomplete_reason_to_expansion_diagnostic(reason: &IncompleteReason) -> ExpansionDiagnostic {
    let stop_reason = match reason {
        IncompleteReason::MissingSource { .. } => ExpansionStopReason::UnresolvedReference,
        IncompleteReason::UnsupportedSyntax { .. } => ExpansionStopReason::UnsupportedOperator,
        IncompleteReason::Cancelled => ExpansionStopReason::BudgetExceeded,
        IncompleteReason::RecursionPolicy { .. } => ExpansionStopReason::BudgetExceeded,
    };
    ExpansionDiagnostic {
        reason: stop_reason,
        context: reason.to_string(),
        property_name: None,
    }
}

/// Convert a single `SolverDiagnostic` to an `ExpansionDiagnostic`.
fn solver_diagnostic_to_expansion_diagnostic(diagnostic: &SolverDiagnostic) -> ExpansionDiagnostic {
    match diagnostic {
        SolverDiagnostic::ConditionalContextTruncated {
            available,
            captured,
        } => ExpansionDiagnostic {
            reason: ExpansionStopReason::ConditionalContextTruncated,
            context: format!(
                "conditional context truncated: {} available, {} captured",
                available, captured
            ),
            property_name: None,
        },
    }
}

/// Collect all expansion diagnostics from a `SolverResult` — both
/// `incomplete_reasons` and `diagnostics`.
pub fn solver_result_to_expansion_diagnostics<T>(
    result: &SolverResult<T>,
) -> Vec<ExpansionDiagnostic> {
    let mut out: Vec<ExpansionDiagnostic> = result
        .incomplete_reasons
        .iter()
        .map(incomplete_reason_to_expansion_diagnostic)
        .collect();
    out.extend(
        result
            .diagnostics
            .iter()
            .map(solver_diagnostic_to_expansion_diagnostic),
    );
    out
}

/// Convert a `SolverResult<TypeExpr>` to an `ExpansionResult<ExpandedNormalizedExpr>`,
/// preserving all metadata including solver diagnostics.
pub fn solver_result_to_normalized_expansion(
    result: SolverResult<TypeExpr>,
) -> ExpansionResult<ExpandedNormalizedExpr> {
    let diagnostics = solver_result_to_expansion_diagnostics(&result);
    ExpansionResult {
        value: ExpandedNormalizedExpr { expr: result.value },
        exactness: result.exactness,
        execution_status: result.execution_status,
        diagnostics,
    }
}

/// Convert a `SolverResult<TypeExpr>` to an `ExpansionResult<ExpandedObjectShape>`,
/// preserving all metadata including solver diagnostics.
pub fn solver_result_to_object_expansion(
    result: SolverResult<TypeExpr>,
) -> ExpansionResult<ExpandedObjectShape> {
    let diagnostics = solver_result_to_expansion_diagnostics(&result);
    let shape = type_expr_to_expanded_shape(&result.value);
    ExpansionResult {
        value: shape,
        exactness: result.exactness,
        execution_status: result.execution_status,
        diagnostics,
    }
}

/// Extract an `ExpandedObjectShape` from a `TypeExpr`.
///
/// Handles `Object`, `Union` (merge with optional), and `Intersection` (merge).
/// Non-object types return an empty shape.
pub fn type_expr_to_object_shape(expr: &TypeExpr) -> ExpandedObjectShape {
    type_expr_to_expanded_shape(expr)
}

/// Recursively extract an `ExpandedObjectShape` from a `TypeExpr`,
/// handling `Object`, `Union`, and `Intersection` types.
pub fn type_expr_to_expanded_shape(expr: &TypeExpr) -> ExpandedObjectShape {
    match expr {
        TypeExpr::Object(obj) => object_to_shape(obj),
        // Union: collect members from each object variant.
        // Props in all variants stay required; others become optional.
        TypeExpr::Union(variants) => {
            let mut all_props: rustc_hash::FxHashMap<String, ExpandedProperty> =
                rustc_hash::FxHashMap::default();
            let mut all_index_sigs = Vec::new();
            let mut all_call_sigs = Vec::new();
            let mut prop_variant_count: rustc_hash::FxHashMap<String, usize> =
                rustc_hash::FxHashMap::default();
            let mut total_object_variants = 0usize;

            for variant in variants.iter() {
                let shape = type_expr_to_expanded_shape(variant);
                if shape.properties.is_empty()
                    && shape.index_signatures.is_empty()
                    && shape.call_signatures.is_empty()
                {
                    continue;
                }
                total_object_variants += 1;
                for prop in shape.properties {
                    *prop_variant_count.entry(prop.name.clone()).or_insert(0) += 1;
                    all_props.entry(prop.name.clone()).or_insert(prop);
                }
                all_index_sigs.extend(shape.index_signatures);
                all_call_sigs.extend(shape.call_signatures);
            }
            let properties: Vec<ExpandedProperty> = all_props
                .into_values()
                .map(|mut p| {
                    let count = prop_variant_count.get(&p.name).copied().unwrap_or_default();
                    if count < total_object_variants {
                        p.optional = true;
                    }
                    p
                })
                .collect();

            ExpandedObjectShape {
                properties,
                index_signatures: all_index_sigs,
                call_signatures: all_call_sigs,
            }
        }
        // Intersection: merge properties from all branches
        TypeExpr::Intersection(parts) => {
            let mut properties = Vec::new();
            let mut index_signatures = Vec::new();
            let mut call_signatures = Vec::new();
            let mut property_positions: rustc_hash::FxHashMap<String, usize> =
                rustc_hash::FxHashMap::default();

            for part in parts.iter() {
                let shape = type_expr_to_expanded_shape(part);
                for prop in shape.properties {
                    if let Some(index) = property_positions.get(&prop.name).copied() {
                        properties[index] = prop;
                    } else {
                        property_positions.insert(prop.name.clone(), properties.len());
                        properties.push(prop);
                    }
                }
                index_signatures.extend(shape.index_signatures);
                call_signatures.extend(shape.call_signatures);
            }

            ExpandedObjectShape {
                properties,
                index_signatures,
                call_signatures,
            }
        }
        _ => ExpandedObjectShape::empty(),
    }
}

fn object_to_shape(obj: &verter_type_expr::ObjectExpr) -> ExpandedObjectShape {
    let mut properties = Vec::new();
    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();

    for member in &obj.properties {
        match member {
            ObjectMember::Property(p) => properties.push(ExpandedProperty {
                name: p.name.clone(),
                ty: p.ty.clone(),
                optional: p.optional,
                readonly: p.readonly,
                declared_in_macro_type_arg: false,
            }),
            ObjectMember::IndexSignature(idx) => index_signatures.push(ExpandedIndexSignature {
                key_type: idx.key_type.clone(),
                value_type: idx.value_type.clone(),
                readonly: idx.readonly,
            }),
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                call_signatures.push(ExpandedCallSignature {
                    parameters: func
                        .parameters
                        .iter()
                        .map(|p| ExpandedParameter {
                            name: p.name.clone().unwrap_or_default(),
                            ty: p.ty.clone(),
                            optional: p.optional,
                            rest: p.rest,
                        })
                        .collect(),
                    return_type: func
                        .return_type
                        .as_ref()
                        .map(|r| r.as_ref().clone())
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                    type_parameters: func.type_parameters.clone(),
                })
            }
            ObjectMember::Method(method) => properties.push(ExpandedProperty {
                name: method.name.clone(),
                ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                optional: method.optional,
                readonly: false,
                declared_in_macro_type_arg: false,
            }),
        }
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}
