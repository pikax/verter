//! Typed-IR `typeof` dependency traversal: a pure, budget-bounded walk over a
//! lowered [`TypeExpr`] that collects every `typeof <value>` reference and
//! decides whether an inferred type is declaration-safe. Iterative (an
//! explicit work list), so a deep owned IR never overflows the stack.

use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_type_expr::facts::TypeDependencyPathFact;
use verter_type_expr::TypeExpr;

pub trait TypeofDependencyCollector {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef);
}

/// Runaway-safety fuse for session-owned semantic-inference tree walks.
/// Parser syntax depth is substantially lower; this bound protects mutated or
/// synthesized owned IR while leaving ordinary authored programs untouched.
pub const SEMANTIC_INFERENCE_TRAVERSAL_BUDGET: usize = 4_096;

impl TypeofDependencyCollector for FxHashSet<String> {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef) {
        if let Some(root) = value_ref.path.first() {
            self.insert(root.clone());
        }
    }
}

impl TypeofDependencyCollector for BTreeSet<TypeDependencyPathFact> {
    fn record(&mut self, value_ref: &verter_type_expr::ValueRef) {
        if let Some(path) = TypeDependencyPathFact::from_segments(value_ref.path.iter().cloned()) {
            self.insert(path);
        }
    }
}

/// Collect every `typeof <value>` dependency reachable in `expr`. Callers
/// choose either root-name or full typed-path retention through the collector.
pub fn collect_typeof_roots<C: TypeofDependencyCollector>(
    expr: &TypeExpr,
    out: &mut C,
) -> Result<(), verter_type_expr::facts::InferenceUnavailableReason> {
    let mut pending = vec![expr];
    let mut visited = 0usize;
    while let Some(current) = pending.pop() {
        visited = visited.saturating_add(1);
        if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            return Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded);
        }
        if let TypeExpr::TypeOf(value_ref) = current {
            out.record(value_ref);
        }
        push_type_expr_children(current, &mut pending);
    }
    Ok(())
}

/// Whether every rendered leaf is declaration-safe. Inferred declaration
/// splices must never hide an implicit `any`/unknown lowering inside a nested
/// function, collection, object, or generic argument.
pub fn type_expr_is_declaration_safe(
    expr: &TypeExpr,
) -> Result<bool, verter_type_expr::facts::InferenceUnavailableReason> {
    let mut pending = vec![expr];
    let mut visited = 0usize;
    while let Some(current) = pending.pop() {
        visited = visited.saturating_add(1);
        if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            return Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded);
        }
        match current {
            TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Any | verter_type_expr::PrimitiveName::Unknown,
            )
            | TypeExpr::Unknown { .. }
            | TypeExpr::SyntheticSlotBinding(_) => return Ok(false),
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function)
                if function.return_type.is_none() =>
            {
                return Ok(false);
            }
            _ => push_type_expr_children(current, &mut pending),
        }
    }
    Ok(true)
}

fn push_type_expr_children<'a>(expr: &'a TypeExpr, pending: &mut Vec<&'a TypeExpr>) {
    let push_type_param = |parameter: &'a verter_type_expr::TypeParam,
                           pending: &mut Vec<&'a TypeExpr>| {
        if let Some(constraint) = parameter.constraint.as_deref() {
            pending.push(constraint);
        }
        if let Some(default) = parameter.default.as_deref() {
            pending.push(default);
        }
    };
    let push_function = |function: &'a verter_type_expr::FunctionExpr,
                         pending: &mut Vec<&'a TypeExpr>| {
        for parameter in &function.parameters {
            pending.push(&parameter.ty);
        }
        if let Some(return_type) = function.return_type.as_deref() {
            pending.push(return_type);
        }
        if let Some(target) = function
            .predicate
            .as_deref()
            .and_then(|predicate| predicate.ty.as_deref())
        {
            pending.push(target);
        }
        for parameter in &function.type_parameters {
            push_type_param(parameter, pending);
        }
    };

    match expr {
        TypeExpr::TypeOf(value_ref) => pending.extend(value_ref.type_args.iter()),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => pending.extend(types.iter()),
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => pending.push(element),
        TypeExpr::Tuple { elements, .. } => {
            pending.extend(elements.iter().map(|element| &element.ty));
        }
        TypeExpr::Object(object) => {
            for member in &object.properties {
                match member {
                    verter_type_expr::ObjectMember::Property(property) => {
                        pending.push(&property.ty);
                    }
                    verter_type_expr::ObjectMember::IndexSignature(signature) => {
                        pending.push(&signature.key_type);
                        pending.push(&signature.value_type);
                    }
                    verter_type_expr::ObjectMember::CallSignature(function)
                    | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                        push_function(function, pending);
                    }
                    verter_type_expr::ObjectMember::Method(method) => {
                        push_function(&method.function, pending);
                    }
                    verter_type_expr::ObjectMember::Spread(spread) => {
                        pending.push(&spread.ty);
                    }
                }
            }
        }
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            push_function(function, pending);
        }
        TypeExpr::IndexedAccess { object, index } => {
            pending.push(object);
            pending.push(index);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            pending.push(check);
            pending.push(extends);
            pending.push(true_type);
            pending.push(false_type);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            pending.push(source);
            pending.push(value);
            if let Some(name_type) = name_type.as_deref() {
                pending.push(name_type);
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => pending.extend(expressions.iter()),
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            pending.extend(type_arguments.iter())
        }
        TypeExpr::IntrinsicApplication { arguments, .. } => pending.extend(arguments.iter()),
        TypeExpr::TypeParameter(parameter) => push_type_param(parameter, pending),
        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            pending.extend(type_arguments.iter());
            for frame in conditional_context.iter() {
                pending.push(&frame.check);
                pending.push(&frame.extends);
            }
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn deep_inference_type_tree_fails_with_typed_budget_without_recursion() {
        let mut expr = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
        for _ in 0..=SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
            expr = TypeExpr::Array {
                element: Arc::new(expr),
                readonly: false,
            };
        }

        assert_eq!(
            type_expr_is_declaration_safe(&expr),
            Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded),
            "deep inferred initializer/return types fail typed instead of overflowing"
        );
        let mut roots = FxHashSet::default();
        assert_eq!(
            collect_typeof_roots(&expr, &mut roots),
            Err(verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded),
        );
        assert!(roots.is_empty());

        // Avoid making the test's destructor itself recursively drop the
        // adversarial Arc chain; the production walkers never own or drop it.
        std::mem::forget(expr);
    }
}
