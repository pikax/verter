//! NormalizedExpr expansion: resolves references and utility types while
//! preserving complex forms symbolically when exact resolution is not possible.

use crate::type_eval::{
    evaluate_with_lookup, is_assignable_to, is_definitely_not_assignable,
    try_evaluate_conditional_with_infer, EvalEnv, EvalLookup, NoopEvalLookup,
};
use crate::type_expr::{ObjectMember, TypeExpr};

use super::request::{
    ExpandedExprResult, ExpandedNormalizedExpr, ExpansionBudget, ExpansionCompleteness,
    ExpansionDiagnostic, ExpansionResult, ExpansionStopReason,
};

/// Expand a `TypeExpr` into an `ExpandedNormalizedExpr`.
///
/// Resolves references and applies utility types where possible.
/// Indeterminate conditionals are preserved symbolically with
/// unevaluated branches. The result is marked `Partial` when
/// any symbolic form could not be fully resolved.
pub fn expand_normalized_expr(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    budget: &ExpansionBudget,
) -> ExpandedExprResult {
    let mut lookup = NoopEvalLookup;
    expand_normalized_expr_with_lookup(expr, env, budget, &mut lookup)
}

pub fn expand_normalized_expr_with_lookup(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    budget: &ExpansionBudget,
    lookup: &mut dyn EvalLookup,
) -> ExpandedExprResult {
    env.apply_expansion_budget(budget);

    let mut diagnostics = Vec::new();
    let result = normalize_expr_with_diagnostics_with_lookup(expr, env, &mut diagnostics, lookup);
    if env.budget_exhausted() {
        push_diagnostic(
            &mut diagnostics,
            ExpansionStopReason::BudgetExceeded,
            "symbolic work limit reached during normalization".to_string(),
            None,
        );
    }
    record_partial_markers(&result, &mut diagnostics, None);

    let completeness = if diagnostics.is_empty() {
        ExpansionCompleteness::Exact
    } else {
        ExpansionCompleteness::Partial
    };

    ExpansionResult {
        value: ExpandedNormalizedExpr { expr: result },
        completeness,
        diagnostics,
    }
}

/// Normalize a `TypeExpr`, handling conditionals specially to avoid
/// evaluating branches of indeterminate conditionals.
#[allow(dead_code)]
pub(crate) fn normalize_expr_with_diagnostics(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
) -> TypeExpr {
    let mut lookup = NoopEvalLookup;
    normalize_expr_with_diagnostics_with_lookup(expr, env, diagnostics, &mut lookup)
}

pub(crate) fn normalize_expr_with_diagnostics_with_lookup(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    match expr {
        // Conditional — check assignability first, only evaluate the resolved branch
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_eval = evaluate_with_lookup(check, env, lookup);
            let extends_eval = evaluate_with_lookup(extends, env, lookup);

            if is_assignable_to(&check_eval, &extends_eval) {
                // Resolved — evaluate only the true branch
                normalize_expr_with_diagnostics_with_lookup(true_type, env, diagnostics, lookup)
            } else if let Some(result) = try_evaluate_conditional_with_infer(
                &check_eval,
                &extends_eval,
                true_type,
                env,
                lookup,
            ) {
                result
            } else if is_definitely_not_assignable(&check_eval, &extends_eval) {
                // Resolved — evaluate only the false branch
                normalize_expr_with_diagnostics_with_lookup(false_type, env, diagnostics, lookup)
            } else {
                // Indeterminate — preserve with UNEVALUATED branches
                push_diagnostic(
                    diagnostics,
                    ExpansionStopReason::IndeterminateConditional,
                    "conditional type could not be resolved".to_string(),
                    None,
                );
                TypeExpr::Conditional {
                    check: std::sync::Arc::new(check_eval),
                    extends: std::sync::Arc::new(extends_eval),
                    true_type: true_type.clone(),
                    false_type: false_type.clone(),
                }
            }
        }

        // All other expressions — delegate to evaluate()
        _ => evaluate_with_lookup(expr, env, lookup),
    }
}

pub(crate) fn record_partial_markers(
    expr: &TypeExpr,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    property_name: Option<&str>,
) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::UnresolvedReference,
                format!("unresolved type reference '{name}'"),
                property_name,
            );
            for arg in type_arguments.iter() {
                record_partial_markers(arg, diagnostics, property_name);
            }
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            let reason = if is_infinite_source(source) {
                ExpansionStopReason::InfiniteKeySpace
            } else {
                ExpansionStopReason::MappedDepthExceeded
            };
            let context = if reason == ExpansionStopReason::InfiniteKeySpace {
                "mapped type has infinite key space".to_string()
            } else {
                "mapped type was preserved symbolically".to_string()
            };
            push_diagnostic(diagnostics, reason, context, property_name);
            record_partial_markers(source, diagnostics, property_name);
            record_partial_markers(value, diagnostics, property_name);
            if let Some(name_type) = name_type {
                record_partial_markers(name_type, diagnostics, property_name);
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::IndeterminateConditional,
                "conditional type could not be resolved".to_string(),
                property_name,
            );
            record_partial_markers(check, diagnostics, property_name);
            record_partial_markers(extends, diagnostics, property_name);
            record_partial_markers(true_type, diagnostics, property_name);
            record_partial_markers(false_type, diagnostics, property_name);
        }
        TypeExpr::KeyOf(inner) => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::UnsupportedOperator,
                "keyof expression was preserved symbolically".to_string(),
                property_name,
            );
            record_partial_markers(inner, diagnostics, property_name);
        }
        TypeExpr::TypeOf(path) => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::UnsupportedOperator,
                format!("typeof {} was preserved symbolically", path.path.join(".")),
                property_name,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::UnsupportedOperator,
                "indexed access was preserved symbolically".to_string(),
                property_name,
            );
            record_partial_markers(object, diagnostics, property_name);
            record_partial_markers(index, diagnostics, property_name);
        }
        TypeExpr::Infer { name } => {
            push_diagnostic(
                diagnostics,
                ExpansionStopReason::UnsupportedOperator,
                format!("infer {name} was preserved symbolically"),
                property_name,
            );
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        record_partial_markers(&prop.ty, diagnostics, property_name);
                    }
                    ObjectMember::Method(method) => {
                        record_partial_markers(
                            &TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                            diagnostics,
                            property_name,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        record_partial_markers(&sig.key_type, diagnostics, property_name);
                        record_partial_markers(&sig.value_type, diagnostics, property_name);
                    }
                    ObjectMember::CallSignature(sig) | ObjectMember::ConstructSignature(sig) => {
                        for param in &sig.parameters {
                            record_partial_markers(&param.ty, diagnostics, property_name);
                        }
                        if let Some(ret) = &sig.return_type {
                            record_partial_markers(ret, diagnostics, property_name);
                        }
                    }
                }
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::Rest(element) => {
            record_partial_markers(element, diagnostics, property_name);
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                record_partial_markers(&element.ty, diagnostics, property_name);
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            for ty in types.iter() {
                record_partial_markers(ty, diagnostics, property_name);
            }
        }
        TypeExpr::Function(func) => {
            for param in &func.parameters {
                record_partial_markers(&param.ty, diagnostics, property_name);
            }
            if let Some(ret) = &func.return_type {
                record_partial_markers(ret, diagnostics, property_name);
            }
        }
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => {}
    }
}

fn push_diagnostic(
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    reason: ExpansionStopReason,
    context: String,
    property_name: Option<&str>,
) {
    if diagnostics.iter().any(|diagnostic| {
        diagnostic.reason == reason && diagnostic.property_name.as_deref() == property_name
    }) {
        return;
    }

    diagnostics.push(ExpansionDiagnostic {
        reason,
        context,
        property_name: property_name.map(str::to_string),
    });
}

fn is_infinite_source(source: &TypeExpr) -> bool {
    matches!(
        source,
        TypeExpr::Primitive(crate::type_expr::PrimitiveName::String)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Number)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Symbol)
    )
}
