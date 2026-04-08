//! Imported type body preference and specificity scoring.
//!
//! Provides `choose_preferred_imported_type_body` for selecting the best body
//! when multiple resolved/declared bodies are available, and
//! `imported_type_body_specificity_score` for ranking type expressions by
//! structural richness.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_semantic::analysis::type_expr::{FunctionExpr, ObjectMember, PrimitiveName, TypeExpr};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportedSymbolDependency {
    pub local_name: String,
    pub canonical_id: String,
    pub exported_name: String,
}

#[derive(Debug, Clone)]
pub struct ComputedEvaluatedTypes {
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub discovered_dependencies: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Body preference and specificity scoring
// ---------------------------------------------------------------------------

pub fn choose_preferred_imported_type_body(
    resolved_body: Option<TypeExpr>,
    resolved_decl_body: Option<TypeExpr>,
) -> Option<TypeExpr> {
    match (resolved_body, resolved_decl_body) {
        (Some(left), Some(right)) => {
            let left_empty_object = is_empty_object_surface(&left);
            let right_empty_object = is_empty_object_surface(&right);
            if left_empty_object != right_empty_object {
                return Some(if left_empty_object { right } else { left });
            }

            let left_surface_props = extracted_surface_property_count(&left);
            let right_surface_props = extracted_surface_property_count(&right);
            if let (Some(left_count), Some(right_count)) = (left_surface_props, right_surface_props)
            {
                if left_count != right_count {
                    return Some(if left_count > right_count {
                        left
                    } else {
                        right
                    });
                }
            }

            let left_method_surface = method_surface_specificity_score(&left);
            let right_method_surface = method_surface_specificity_score(&right);
            if left_method_surface != right_method_surface {
                return Some(if left_method_surface > right_method_surface {
                    left
                } else {
                    right
                });
            }

            let left_top_level_branching = top_level_branching_surface_score(&left);
            let right_top_level_branching = top_level_branching_surface_score(&right);
            if left_top_level_branching != right_top_level_branching {
                return Some(if left_top_level_branching > right_top_level_branching {
                    left
                } else {
                    right
                });
            }

            let left_nested = contains_nested_resolution_targets(&left);
            let right_nested = contains_nested_resolution_targets(&right);
            if left_nested != right_nested {
                return Some(if left_nested { right } else { left });
            }

            let left_non_object = has_non_object_top_level_surface(&left);
            let right_non_object = has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            let left_bound_generic_penalty = bound_generic_ref_penalty(&left);
            let right_bound_generic_penalty = bound_generic_ref_penalty(&right);
            if left_bound_generic_penalty != right_bound_generic_penalty {
                return Some(
                    if left_bound_generic_penalty < right_bound_generic_penalty {
                        left
                    } else {
                        right
                    },
                );
            }

            if imported_type_body_specificity_score(&right)
                > imported_type_body_specificity_score(&left)
            {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(body), None) | (None, Some(body)) => Some(body),
        (None, None) => None,
    }
}

fn has_non_object_top_level_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => has_non_object_top_level_surface(inner),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            types.iter().any(has_non_object_top_level_surface)
                || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
        }
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Object(_) => false,
        _ => false,
    }
}

fn is_empty_object_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_empty_object_surface(inner),
        TypeExpr::Object(obj) => obj.properties.is_empty(),
        _ => false,
    }
}

fn contains_nested_resolution_targets(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_) => false,
        TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => contains_nested_resolution_targets(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| contains_nested_resolution_targets(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Object(_) => false,
        TypeExpr::Function(_) => false,
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Infer { .. } => false,
    }
}

fn extracted_surface_property_count(expr: &TypeExpr) -> Option<usize> {
    match expr {
        TypeExpr::Parenthesized(inner) => extracted_surface_property_count(inner),
        TypeExpr::Object(obj) => Some(
            obj.properties
                .iter()
                .filter(|member| {
                    matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_))
                })
                .count(),
        ),
        TypeExpr::Intersection(types) => {
            let mut total = 0usize;
            let mut saw_surface = false;
            for ty in types.iter() {
                let count = extracted_surface_property_count(ty)?;
                total += count;
                saw_surface = true;
            }
            saw_surface.then_some(total)
        }
        _ => None,
    }
}

fn method_surface_specificity_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => method_surface_specificity_score(inner),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Method(method) => {
                    2 + method_surface_specificity_score(&TypeExpr::Function(Arc::new(
                        method.function.clone(),
                    )))
                }
                ObjectMember::Property(prop) => {
                    usize::from(matches!(prop.ty, TypeExpr::Function(_)))
                        + method_surface_specificity_score(&prop.ty)
                }
                ObjectMember::IndexSignature(sig) => {
                    method_surface_specificity_score(&sig.key_type)
                        + method_surface_specificity_score(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    method_surface_specificity_score(&TypeExpr::Function(Arc::new(func.clone())))
                }
            })
            .sum(),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .map(|param| method_surface_specificity_score(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(method_surface_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Array { element, .. } | TypeExpr::KeyOf(element) | TypeExpr::Rest(element) => {
            method_surface_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| method_surface_specificity_score(&element.ty))
            .sum(),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().map(method_surface_specificity_score).sum(),
        TypeExpr::IndexedAccess { object, index } => {
            method_surface_specificity_score(object) + method_surface_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            method_surface_specificity_score(check)
                + method_surface_specificity_score(extends)
                + method_surface_specificity_score(true_type)
                + method_surface_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            method_surface_specificity_score(source)
                + method_surface_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(method_surface_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => 0,
    }
}

fn bound_generic_ref_penalty(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Infer { .. } => 0,
        TypeExpr::TypeOf(_) => 1,
        TypeExpr::TypeParameter(param) => {
            param
                .constraint
                .as_deref()
                .map(bound_generic_ref_penalty)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
        }
        TypeExpr::Ref { type_arguments, .. } => {
            usize::from(!type_arguments.is_empty())
                + type_arguments
                    .iter()
                    .map(bound_generic_ref_penalty)
                    .sum::<usize>()
        }
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => bound_generic_ref_penalty(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| bound_generic_ref_penalty(&element.ty))
            .sum(),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().map(bound_generic_ref_penalty).sum(),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(prop) => bound_generic_ref_penalty(&prop.ty),
                ObjectMember::IndexSignature(sig) => {
                    bound_generic_ref_penalty(&sig.key_type)
                        + bound_generic_ref_penalty(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters
                        .iter()
                        .map(|param| bound_generic_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + func
                            .return_type
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                        + func
                            .type_parameters
                            .iter()
                            .map(|param| {
                                param
                                    .constraint
                                    .as_deref()
                                    .map(bound_generic_ref_penalty)
                                    .unwrap_or_default()
                                    + param
                                        .default
                                        .as_deref()
                                        .map(bound_generic_ref_penalty)
                                        .unwrap_or_default()
                            })
                            .sum::<usize>()
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|param| bound_generic_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                        + method
                            .function
                            .type_parameters
                            .iter()
                            .map(|param| {
                                param
                                    .constraint
                                    .as_deref()
                                    .map(bound_generic_ref_penalty)
                                    .unwrap_or_default()
                                    + param
                                        .default
                                        .as_deref()
                                        .map(bound_generic_ref_penalty)
                                        .unwrap_or_default()
                            })
                            .sum::<usize>()
                }
            })
            .sum(),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .map(|param| bound_generic_ref_penalty(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
                + func
                    .type_parameters
                    .iter()
                    .map(|param| {
                        param
                            .constraint
                            .as_deref()
                            .map(bound_generic_ref_penalty)
                            .unwrap_or_default()
                            + param
                                .default
                                .as_deref()
                                .map(bound_generic_ref_penalty)
                                .unwrap_or_default()
                    })
                    .sum::<usize>()
        }
        TypeExpr::IndexedAccess { object, index } => {
            bound_generic_ref_penalty(object) + bound_generic_ref_penalty(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            bound_generic_ref_penalty(check)
                + bound_generic_ref_penalty(extends)
                + bound_generic_ref_penalty(true_type)
                + bound_generic_ref_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            bound_generic_ref_penalty(source)
                + bound_generic_ref_penalty(value)
                + name_type
                    .as_deref()
                    .map(bound_generic_ref_penalty)
                    .unwrap_or_default()
        }
    }
}

fn top_level_branching_surface_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => top_level_branching_surface_score(inner),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            let mut score = 0usize;
            for ty in types.iter() {
                match ty {
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    TypeExpr::Unknown { .. } => {}
                    _ => score += 1,
                }
            }
            if score >= 2 {
                score
            } else {
                0
            }
        }
        _ => 0,
    }
}

const SPECIFICITY_UNKNOWN: usize = 0;
const SPECIFICITY_TYPEOF: usize = 4;
const SPECIFICITY_TERMINAL: usize = 8;
const SPECIFICITY_REF_BASE: usize = 16;
const SPECIFICITY_TEMPLATE_LITERAL_BASE: usize = 20;
const SPECIFICITY_WRAPPER_BASE: usize = 24;
const SPECIFICITY_INDEXED_ACCESS_BASE: usize = 28;
const SPECIFICITY_MAPPED_BASE: usize = 32;
const SPECIFICITY_TUPLE_BASE: usize = 40;
const SPECIFICITY_FUNCTION_BASE: usize = 48;
const SPECIFICITY_UNION_BASE: usize = 56;
const SPECIFICITY_INTERSECTION_BASE: usize = 64;
const SPECIFICITY_OBJECT_BASE: usize = 96;
const SPECIFICITY_OBJECT_PROPERTY: usize = 12;
const SPECIFICITY_INDEX_SIGNATURE: usize = 6;
const SPECIFICITY_CALL_LIKE_MEMBER: usize = 10;

pub fn imported_type_body_specificity_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Unknown { .. } => SPECIFICITY_UNKNOWN,
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => SPECIFICITY_TERMINAL,
        TypeExpr::TypeOf(_) => SPECIFICITY_TYPEOF,
        TypeExpr::TypeParameter(param) => {
            SPECIFICITY_REF_BASE
                + param
                    .constraint
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Ref { type_arguments, .. } => {
            SPECIFICITY_REF_BASE
                + type_arguments
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => {
            SPECIFICITY_WRAPPER_BASE + imported_type_body_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => {
            SPECIFICITY_TUPLE_BASE
                + elements
                    .iter()
                    .map(|element| imported_type_body_specificity_score(&element.ty))
                    .sum::<usize>()
        }
        TypeExpr::Union(types) => {
            SPECIFICITY_UNION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Intersection(types) => {
            SPECIFICITY_INTERSECTION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Object(obj) => {
            SPECIFICITY_OBJECT_BASE
                + obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(prop) => {
                            SPECIFICITY_OBJECT_PROPERTY
                                + imported_type_body_specificity_score(&prop.ty)
                        }
                        ObjectMember::IndexSignature(sig) => {
                            SPECIFICITY_INDEX_SIGNATURE
                                + imported_type_body_specificity_score(&sig.key_type)
                                + imported_type_body_specificity_score(&sig.value_type)
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            SPECIFICITY_CALL_LIKE_MEMBER + imported_function_specificity_score(func)
                        }
                        ObjectMember::Method(method) => {
                            SPECIFICITY_CALL_LIKE_MEMBER
                                + imported_function_specificity_score(&method.function)
                        }
                    })
                    .sum::<usize>()
        }
        TypeExpr::Function(func) => {
            SPECIFICITY_FUNCTION_BASE + imported_function_specificity_score(func)
        }
        TypeExpr::IndexedAccess { object, index } => {
            SPECIFICITY_INDEXED_ACCESS_BASE
                + imported_type_body_specificity_score(object)
                + imported_type_body_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            SPECIFICITY_WRAPPER_BASE
                + imported_type_body_specificity_score(check)
                + imported_type_body_specificity_score(extends)
                + imported_type_body_specificity_score(true_type)
                + imported_type_body_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            SPECIFICITY_MAPPED_BASE
                + imported_type_body_specificity_score(source)
                + imported_type_body_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            SPECIFICITY_TEMPLATE_LITERAL_BASE
                + expressions
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Infer { .. } => SPECIFICITY_TYPEOF,
        TypeExpr::RecursiveRef { .. } => SPECIFICITY_REF_BASE,
    }
}

fn imported_function_specificity_score(func: &FunctionExpr) -> usize {
    let params = func
        .parameters
        .iter()
        .map(|param| imported_type_body_specificity_score(&param.ty))
        .sum::<usize>();
    let ret = func
        .return_type
        .as_deref()
        .map(imported_type_body_specificity_score)
        .unwrap_or_default();
    let generics = func
        .type_parameters
        .iter()
        .map(|param| {
            param
                .constraint
                .as_deref()
                .map(imported_type_body_specificity_score)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        })
        .sum::<usize>();
    params + ret + generics
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::{
        FunctionParam, ObjectExpr, ObjectProperty, PrimitiveName, TypeExpr,
    };

    #[test]
    fn choose_preferred_imported_type_body_keeps_meaningful_top_level_union_surface() {
        let flattened_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "path".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }));
        let symbolic_union = TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::named("St"),
            TypeExpr::named("vt"),
        ]);

        let preferred = choose_preferred_imported_type_body(
            Some(flattened_object.clone()),
            Some(symbolic_union.clone()),
        )
        .expect("preferred body should exist");

        assert_eq!(preferred, symbolic_union);
        assert_ne!(preferred, flattened_object);
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_method_signatures_over_function_properties() {
        let function = FunctionExpr {
            parameters: vec![FunctionParam {
                name: Some("props".to_string()),
                ty: TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "ui".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    })],
                })),
                optional: false,
                rest: false,
            }],
            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
            type_parameters: vec![],
        };
        let property_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "default".to_string(),
                ty: TypeExpr::Function(Arc::new(function.clone())),
                optional: true,
                readonly: false,
            })],
        }));
        let method_object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Method(
                verter_semantic::analysis::type_expr::MethodSignature {
                    name: "default".to_string(),
                    function,
                    optional: true,
                },
            )],
        }));

        let preferred =
            choose_preferred_imported_type_body(Some(property_object), Some(method_object.clone()))
                .expect("preferred body should exist");

        assert_eq!(preferred, method_object);
    }
}
