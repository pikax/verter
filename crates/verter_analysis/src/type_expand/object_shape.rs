//! ObjectShape expansion: extracts a materialized object surface from a `TypeExpr`.

use crate::type_eval::{evaluate, EvalEnv};
use crate::type_expr::{ObjectExpr, ObjectMember, TypeExpr};
use rustc_hash::{FxHashMap, FxHashSet};

use super::normalized::{normalize_expr_with_diagnostics, record_partial_markers};
use super::request::{
    ExpandedIndexSignature, ExpandedObjectResult, ExpandedObjectShape, ExpandedProperty,
    ExpansionBudget, ExpansionCompleteness, ExpansionDiagnostic, ExpansionResult,
    ExpansionStopReason,
};

/// Expand a `TypeExpr` into an `ExpandedObjectShape`.
///
/// This is the primary entry point for consumers that need a list of
/// typed members (e.g., `defineProps<T>()`, fallthrough surface).
///
/// The expander handles structural types directly (Object, Intersection,
/// Union) and delegates to `evaluate()` for reference resolution, generic
/// instantiation, and utility type application.
pub fn expand_object_shape(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    budget: &ExpansionBudget,
) -> ExpandedObjectResult {
    env.apply_expansion_budget(budget);

    let mut diagnostics = Vec::new();
    let shape = extract_shape(expr, env, &mut diagnostics);
    if env.budget_exhausted() {
        diagnostics.push(ExpansionDiagnostic {
            reason: ExpansionStopReason::BudgetExceeded,
            context: "symbolic work limit reached during object-shape expansion".to_string(),
            property_name: None,
        });
    }

    // Scan property types for unexpanded forms (Mapped, Conditional, unresolved Ref, etc.)
    for prop in &shape.properties {
        record_partial_markers(&prop.ty, &mut diagnostics, Some(prop.name.as_str()));
    }

    let completeness = if diagnostics.is_empty() {
        ExpansionCompleteness::Exact
    } else {
        ExpansionCompleteness::Partial
    };

    ExpansionResult {
        value: shape,
        completeness,
        diagnostics,
    }
}

/// Extract an `ExpandedObjectShape` from a `TypeExpr`, handling structural
/// forms directly and delegating sub-expression resolution to `evaluate()`.
fn extract_shape(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
) -> ExpandedObjectShape {
    match expr {
        // Direct object — extract properties
        TypeExpr::Object(obj) => object_expr_to_shape(obj, env, diagnostics, true),

        TypeExpr::Parenthesized(inner) => extract_shape(inner, env, diagnostics),

        // Intersection — merge shapes from each branch with correct optionality
        TypeExpr::Intersection(types) => {
            let shapes: Vec<ExpandedObjectShape> = types
                .iter()
                .map(|t| extract_shape(t, env, diagnostics))
                .collect();
            merge_intersection_shapes(shapes)
        }

        // Union — Vue props merge semantics
        TypeExpr::Union(types) => {
            let shapes: Vec<ExpandedObjectShape> = types
                .iter()
                .map(|t| extract_shape(t, env, diagnostics))
                .collect();
            merge_union_shapes_vue(shapes)
        }

        // Type reference — evaluate to resolve, then extract shape from result
        TypeExpr::Ref { .. } => {
            let evaluated = evaluate(expr, env);
            if env.budget_exhausted() {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::BudgetExceeded,
                    context: "symbolic work limit reached during evaluation".to_string(),
                    property_name: None,
                });
            }
            // If evaluate returned the same Ref (unresolved), emit diagnostic
            // Skip if budget already exhausted (that's the real cause)
            if let TypeExpr::Ref { name, .. } = &evaluated {
                if !env.budget_exhausted() {
                    diagnostics.push(ExpansionDiagnostic {
                        reason: ExpansionStopReason::UnresolvedReference,
                        context: format!("unresolved type reference '{name}'"),
                        property_name: None,
                    });
                }
                return ExpandedObjectShape::empty();
            }
            if let TypeExpr::Object(obj) = &evaluated {
                return object_expr_to_shape(obj, env, diagnostics, false);
            }
            // Recurse on the evaluated result (may be Object, Intersection, etc.)
            extract_shape(&evaluated, env, diagnostics)
        }

        // Mapped type that wasn't expanded by evaluate (infinite key space or depth limit)
        TypeExpr::Mapped { source, value, .. } => {
            // First try evaluating the whole mapped type
            let evaluated = evaluate(expr, env);
            // If it resolved to an Object, extract that
            if let TypeExpr::Object(obj) = &evaluated {
                return object_expr_to_shape(obj, env, diagnostics, false);
            }
            // Still a Mapped — check why
            if is_infinite_source(source) {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::InfiniteKeySpace,
                    context: "mapped type has infinite key space".to_string(),
                    property_name: None,
                });
                ExpandedObjectShape {
                    properties: Vec::new(),
                    index_signatures: vec![ExpandedIndexSignature {
                        key_type: *source.clone(),
                        value_type: *value.clone(),
                        readonly: false,
                    }],
                    call_signatures: Vec::new(),
                }
            } else {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::MappedDepthExceeded,
                    context: "mapped type preserved symbolically".to_string(),
                    property_name: None,
                });
                ExpandedObjectShape::empty()
            }
        }

        // Conditional — resolve if possible, skip if indeterminate
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_eval = evaluate(check, env);
            let extends_eval = evaluate(extends, env);
            if crate::type_eval::is_assignable_to(&check_eval, &extends_eval) {
                let evaluated_branch = evaluate(true_type, env);
                if let TypeExpr::Object(obj) = &evaluated_branch {
                    return object_expr_to_shape(obj, env, diagnostics, false);
                }
                return extract_shape(&evaluated_branch, env, diagnostics);
            } else if crate::type_eval::is_definitely_not_assignable(&check_eval, &extends_eval) {
                let evaluated_branch = evaluate(false_type, env);
                if let TypeExpr::Object(obj) = &evaluated_branch {
                    return object_expr_to_shape(obj, env, diagnostics, false);
                }
                return extract_shape(&evaluated_branch, env, diagnostics);
            }
            // Indeterminate — skip, emit diagnostic
            diagnostics.push(ExpansionDiagnostic {
                reason: ExpansionStopReason::IndeterminateConditional,
                context: "conditional type could not be resolved".to_string(),
                property_name: None,
            });
            ExpandedObjectShape::empty()
        }

        // KeyOf, IndexedAccess, TypeOf — try evaluating
        TypeExpr::KeyOf(_) | TypeExpr::IndexedAccess { .. } | TypeExpr::TypeOf(_) => {
            let evaluated = evaluate(expr, env);
            if matches!(
                &evaluated,
                TypeExpr::KeyOf(_) | TypeExpr::IndexedAccess { .. } | TypeExpr::TypeOf(_)
            ) {
                // Still unresolved
                ExpandedObjectShape::empty()
            } else if let TypeExpr::Object(obj) = &evaluated {
                object_expr_to_shape(obj, env, diagnostics, false)
            } else {
                extract_shape(&evaluated, env, diagnostics)
            }
        }

        // All other forms produce no object shape
        _ => ExpandedObjectShape::empty(),
    }
}

/// Convert a `ObjectExpr` (from TypeExpr::Object) to `ExpandedObjectShape`.
fn object_expr_to_shape(
    obj: &ObjectExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
) -> ExpandedObjectShape {
    let mut properties = Vec::new();
    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();

    for member in &obj.properties {
        match member {
            ObjectMember::Property(prop) => {
                properties.push(ExpandedProperty {
                    name: prop.name.clone(),
                    ty: normalize_member_type(&prop.ty, env, diagnostics, normalize_members),
                    optional: prop.optional,
                    readonly: prop.readonly,
                });
            }
            ObjectMember::IndexSignature(sig) => {
                index_signatures.push(ExpandedIndexSignature {
                    key_type: normalize_member_type(
                        &sig.key_type,
                        env,
                        diagnostics,
                        normalize_members,
                    ),
                    value_type: normalize_member_type(
                        &sig.value_type,
                        env,
                        diagnostics,
                        normalize_members,
                    ),
                    readonly: sig.readonly,
                });
            }
            ObjectMember::CallSignature(sig) | ObjectMember::ConstructSignature(sig) => {
                call_signatures.push(function_expr_to_call_sig(
                    sig,
                    env,
                    diagnostics,
                    normalize_members,
                ));
            }
            ObjectMember::Method(method) => {
                properties.push(ExpandedProperty {
                    name: method.name.clone(),
                    ty: TypeExpr::Function(function_expr_to_type(
                        &method.function,
                        env,
                        diagnostics,
                        normalize_members,
                    )),
                    optional: method.optional,
                    readonly: false,
                });
            }
        }
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

fn normalize_member_type(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
) -> TypeExpr {
    if normalize_members {
        normalize_expr_with_diagnostics(expr, env, diagnostics)
    } else {
        expr.clone()
    }
}

fn function_expr_to_type(
    func: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
) -> crate::type_expr::FunctionExpr {
    if normalize_members {
        normalize_function_expr(func, env, diagnostics)
    } else {
        func.clone()
    }
}

/// Merge intersection of object shapes.
///
/// Optionality: both must be optional for the result to be optional.
/// Readonly: either is readonly makes the result readonly.
fn merge_intersection_shapes(shapes: Vec<ExpandedObjectShape>) -> ExpandedObjectShape {
    let mut merged_props: Vec<ExpandedProperty> = Vec::new();
    let mut merged_index = Vec::new();
    let mut merged_call = Vec::new();

    for shape in shapes {
        for prop in shape.properties {
            if let Some(existing) = merged_props.iter_mut().find(|p| p.name == prop.name) {
                // Intersection: optional only if BOTH are optional
                existing.optional = existing.optional && prop.optional;
                // Intersection: readonly if EITHER is readonly
                existing.readonly = existing.readonly || prop.readonly;
                // Intersection: intersect property types when they differ
                if existing.ty != prop.ty {
                    existing.ty = TypeExpr::Intersection(vec![existing.ty.clone(), prop.ty]);
                }
            } else {
                merged_props.push(prop);
            }
        }
        merged_index.extend(shape.index_signatures);
        merged_call.extend(shape.call_signatures);
    }

    ExpandedObjectShape {
        properties: merged_props,
        index_signatures: merged_index,
        call_signatures: merged_call,
    }
}

/// Merge union of object shapes using Vue props merge semantics.
///
/// - Keys present in ALL variants are required (unless optional in any)
/// - Keys present in only some variants are optional
/// - Property types are unioned across variants
/// - Property order: first appearance across variants
fn merge_union_shapes_vue(shapes: Vec<ExpandedObjectShape>) -> ExpandedObjectShape {
    if shapes.is_empty() {
        return ExpandedObjectShape::empty();
    }
    if shapes.len() == 1 {
        return shapes.into_iter().next().unwrap();
    }

    let variant_count = shapes.len();

    struct PropState {
        present_in: usize,
        optional_in_any: bool,
        types: Vec<TypeExpr>,
        readonly: bool,
    }

    let mut order: Vec<String> = Vec::new();
    let mut states: FxHashMap<String, PropState> = FxHashMap::default();

    for shape in &shapes {
        let mut seen_in_variant: FxHashSet<String> = FxHashSet::default();
        for prop in &shape.properties {
            let state = states.entry(prop.name.clone()).or_insert_with(|| {
                order.push(prop.name.clone());
                PropState {
                    present_in: 0,
                    optional_in_any: false,
                    types: Vec::new(),
                    readonly: true, // AND semantics: start true, any non-readonly makes false
                }
            });

            if seen_in_variant.insert(prop.name.clone()) {
                state.present_in += 1;
            }
            state.optional_in_any |= prop.optional;
            // Union: readonly only if ALL variants mark it readonly
            state.readonly &= prop.readonly;
            if !state.types.iter().any(|t| t == &prop.ty) {
                state.types.push(prop.ty.clone());
            }
        }
    }

    let properties = order
        .into_iter()
        .filter_map(|name| {
            let state = states.remove(&name)?;
            let optional = state.present_in < variant_count || state.optional_in_any;
            let ty = TypeExpr::union(state.types);
            Some(ExpandedProperty {
                name,
                ty,
                optional,
                readonly: state.readonly,
            })
        })
        .collect();

    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();
    for shape in shapes {
        index_signatures.extend(shape.index_signatures);
        call_signatures.extend(shape.call_signatures);
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

/// Convert a `FunctionExpr` to an `ExpandedCallSignature`.
fn function_expr_to_call_sig(
    sig: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
) -> super::request::ExpandedCallSignature {
    use crate::type_expr::PrimitiveName;
    let normalized = function_expr_to_type(sig, env, diagnostics, normalize_members);
    super::request::ExpandedCallSignature {
        parameters: normalized
            .parameters
            .iter()
            .map(|p| super::request::ExpandedParameter {
                name: p.name.clone().unwrap_or_default(),
                ty: p.ty.clone(),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: normalized
            .return_type
            .as_deref()
            .cloned()
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
        type_parameters: normalized.type_parameters.clone(),
    }
}

fn normalize_function_expr(
    sig: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
) -> crate::type_expr::FunctionExpr {
    crate::type_expr::FunctionExpr {
        parameters: sig
            .parameters
            .iter()
            .map(|param| crate::type_expr::FunctionParam {
                name: param.name.clone(),
                ty: normalize_expr_with_diagnostics(&param.ty, env, diagnostics),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: sig
            .return_type
            .as_ref()
            .map(|ret| Box::new(normalize_expr_with_diagnostics(ret, env, diagnostics))),
        type_parameters: sig.type_parameters.clone(),
    }
}

/// Check if a mapped type source represents an infinite key space.
fn is_infinite_source(source: &TypeExpr) -> bool {
    matches!(
        source,
        TypeExpr::Primitive(crate::type_expr::PrimitiveName::String)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Number)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Symbol)
    )
}
