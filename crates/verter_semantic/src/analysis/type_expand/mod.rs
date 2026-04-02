//! Type expansion — now fully backed by the native type solver.
//!
//! The old lightweight evaluator functions have been removed. All expansion
//! goes through `type_solver::solve::solve_type()`.

mod request;

pub use request::{
    ExpandedCallSignature, ExpandedComponentTypes, ExpandedField, ExpandedIndexSignature,
    ExpandedMacroObjectShape, ExpandedMacroProps, ExpandedNormalizedExpr, ExpandedObjectShape,
    ExpandedParameter, ExpandedProperty, ExpansionCompleteness, ExpansionDiagnostic,
    ExpansionMetadata, ExpansionResult, ExpansionStopReason, SolverExactCompat,
};

use crate::analysis::type_expr::{ObjectMember, PrimitiveName, TypeExpr};
use crate::analysis::type_solver::host::TypeSolverHost;
use crate::analysis::type_solver::result::SolverExactness;
use crate::analysis::type_solver::solve::{solve_type, SolveLimits};

/// Expand a `TypeExpr` into an `ExpandedObjectShape` via the native solver.
pub fn expand_object_shape(
    expr: &TypeExpr,
    solver_host: &dyn TypeSolverHost,
) -> ExpansionResult<ExpandedObjectShape> {
    let result = solve_type(expr, solver_host, SolveLimits::default());
    type_expr_to_object_result(&result.value, result.exactness)
}

/// Expand a `TypeExpr` into a normalized expression via the native solver.
pub fn expand_normalized_expr(
    expr: &TypeExpr,
    solver_host: &dyn TypeSolverHost,
) -> ExpansionResult<ExpandedNormalizedExpr> {
    let result = solve_type(expr, solver_host, SolveLimits::default());
    ExpansionResult {
        value: ExpandedNormalizedExpr { expr: result.value },
        completeness: SolverExactCompat::from(result.exactness),
        diagnostics: Vec::new(),
    }
}

/// Extract an `ExpandedObjectShape` from a `TypeExpr`.
///
/// Handles `Object`, `Union` (merge with optional), and `Intersection` (merge).
/// Non-object types return an empty shape.
pub fn type_expr_to_object_shape(expr: &TypeExpr) -> ExpandedObjectShape {
    type_expr_to_expanded_shape(expr)
}

fn type_expr_to_object_result(
    expr: &TypeExpr,
    exactness: SolverExactness,
) -> ExpansionResult<ExpandedObjectShape> {
    ExpansionResult {
        value: type_expr_to_expanded_shape(expr),
        completeness: SolverExactCompat::from(exactness),
        diagnostics: Vec::new(),
    }
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
            let mut seen_names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();

            for part in parts.iter() {
                let shape = type_expr_to_expanded_shape(part);
                for prop in shape.properties {
                    if seen_names.insert(prop.name.clone()) {
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

fn object_to_shape(obj: &crate::analysis::type_expr::ObjectExpr) -> ExpandedObjectShape {
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
            }),
        }
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}
