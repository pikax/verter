//! Symbolic-penalty + materialization-improvement scoring helpers.
//!
//! domain 10 — TypeExpr-walking helpers used by the materialization
//! pipeline to choose between candidate surfaces. These are pure leaf helpers:
//! no shared state, no host access, no graph access.

pub(crate) fn count_symbolic_carriers_in_expr(expr: &verter_type_expr::TypeExpr) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::Primitive(_) | TypeExpr::Literal(_) => {}
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            TypeExpr::Function(function) => {
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                score += 1;
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                score += 1;
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                score += 1;
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                score += 1;
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::RecursiveRef { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            // Synthetic slot-binding carrier IS itself a symbolic
            // carrier (an unresolved binding identity); count it as one.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => {
                score += 1;
            }
        }
    }

    score
}

fn count_generic_detail_in_expr(expr: &verter_type_expr::TypeExpr) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::TypeParameter(parameter) => {
                score += 1;
                if let Some(default) = parameter.default.as_deref() {
                    stack.push(default);
                }
                if let Some(constraint) = parameter.constraint.as_deref() {
                    stack.push(constraint);
                }
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            for type_parameter in method.function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for type_parameter in function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            TypeExpr::Function(function) => {
                for type_parameter in function.type_parameters.iter().rev() {
                    score += 1;
                    if let Some(default) = type_parameter.default.as_deref() {
                        stack.push(default);
                    }
                    if let Some(constraint) = type_parameter.constraint.as_deref() {
                        stack.push(constraint);
                    }
                }
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. }
            | TypeExpr::RecursiveRef { type_arguments, .. } => {
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            // Synthetic carriers carry no generic detail.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => {}
        }
    }

    score
}

fn type_expr_has_structural_top_level(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_has_structural_top_level(inner),
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Unknown { .. }
        // Synthetic carriers are intrinsic terminal carriers, not a
        // concrete structural shape — treat as non-structural.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

pub(crate) fn compare_type_expr_improvement(
    candidate: &verter_type_expr::TypeExpr,
    current: &verter_type_expr::TypeExpr,
) -> bool {
    if matches!(current, verter_type_expr::TypeExpr::Unknown { .. })
        && !matches!(candidate, verter_type_expr::TypeExpr::Unknown { .. })
    {
        return true;
    }

    let candidate_score = count_symbolic_carriers_in_expr(candidate);
    let current_score = count_symbolic_carriers_in_expr(current);

    candidate_score < current_score
        || (type_expr_has_structural_top_level(candidate)
            && !type_expr_has_structural_top_level(current))
        || (candidate_score == current_score
            && count_generic_detail_in_expr(candidate) > count_generic_detail_in_expr(current))
}

// promoted from `pub(super)` to `pub(crate)` so the moved
// `host_manage::component_meta_methods.rs` (formerly the in-tree
// `host_methods.rs`) reaches the function via the
// `crate::meta_resolve::component_meta_registry_prefers_structural_materialization`
// re-export. Without the promotion, the parent shell's `pub(crate) use`
// would be rejected (E0364).
pub(crate) fn component_meta_registry_prefers_structural_materialization(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Function(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_) => true,
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers are intrinsic terminals — already final at
        // the projector surface; no structural materialisation needed.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => false,
    }
}
