//! Component-meta registry publication.
//!
//! This module owns registry queueing, route merging, and publication
//! policy for routed component-meta type publication.
//!
//! See architectural rule 10: "Component-meta publication stays in resolver_core."

use std::collections::VecDeque;
use std::sync::Arc;

use crate::resolver_core::ResolverContext;
use crate::resolver_core::RouteDemand;
use crate::types::FileAnalysisSnapshot;
use verter_semantic::analysis::type_expr::{FunctionExpr, ObjectMember, PrimitiveName, TypeExpr};

/// Work item for the unified registry publication queue.
///
/// Combines initial entries and transitive references into a single
/// queue that the resolver-core registry publisher processes uniformly.
#[derive(Debug, Clone)]
pub enum RegistryWorkItem {
    /// Initial registry entry from the component's direct type analysis.
    InitialEntry {
        index: usize,
        declaration_source: String,
        requested_name: String,
    },
    /// Transitive type reference discovered from props/emits/slots surfaces.
    TransitiveRef {
        name: String,
        source_hint: Option<String>,
        route: crate::resolver_core::RouteDemand,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingComponentMetaRegistryRef {
    pub(crate) name: String,
    pub(crate) source_hint: Option<String>,
    pub(crate) exported_name: Option<String>,
    pub(crate) route: RouteDemand,
}

pub(crate) fn upsert_component_meta_registry_entry(
    owner_canonical: &str,
    resolved_type_registry: &mut Vec<
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
    >,
    resolved_type_registry_meta: &mut Vec<crate::resolver_core::ResolvedTypeRegistryMeta>,
    published_names: &mut rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: String,
    type_expr: verter_semantic::analysis::type_expr::TypeExpr,
    declaration: crate::resolver_core::ResolvedTypeDeclaration,
    collection_expr: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) {
    let declaration_source_hint =
        (!declaration.canonical_source.is_empty()).then(|| declaration.canonical_source.clone());
    let collect_nested_refs = should_collect_component_meta_registry_nested_refs(
        owner_canonical,
        declaration_source_hint.as_deref(),
    );
    if let Some(index) = resolved_type_registry
        .iter()
        .position(|entry| entry.name == name)
    {
        let existing = resolved_type_registry[index].type_expr.clone();
        let preferred =
            merge_component_meta_registry_candidates(Some(existing.clone()), Some(type_expr))
                .unwrap_or(existing.clone());
        if preferred != existing {
            resolved_type_registry[index].type_expr = preferred.clone();
            if let Some(meta) = resolved_type_registry_meta.get_mut(index) {
                *meta = crate::resolver_core::ResolvedTypeRegistryMeta {
                    name: name.clone(),
                    declaration,
                };
            }
            if collect_nested_refs {
                collect_component_meta_registry_refs(
                    collection_expr.unwrap_or(&preferred),
                    published_names,
                    queued_names,
                    referenced_names,
                    declaration_source_hint.as_deref(),
                    false,
                );
            }
        }
        return;
    }

    if collect_nested_refs {
        collect_component_meta_registry_refs(
            collection_expr.unwrap_or(&type_expr),
            published_names,
            queued_names,
            referenced_names,
            declaration_source_hint.as_deref(),
            false,
        );
    }
    resolved_type_registry.push(
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
            name: name.clone(),
            type_expr,
            type_expansion: None,
        },
    );
    resolved_type_registry_meta.push(crate::resolver_core::ResolvedTypeRegistryMeta {
        name: name.clone(),
        declaration,
    });
    published_names.insert(name);
}

pub(crate) fn should_collect_component_meta_registry_nested_refs(
    owner_canonical: &str,
    source_hint: Option<&str>,
) -> bool {
    match source_hint.filter(|source| !source.is_empty()) {
        Some(source) => source == owner_canonical,
        None => true,
    }
}

pub(crate) fn owner_component_meta_registry_import_root(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    _snapshot: &FileAnalysisSnapshot,
    local_name: &str,
) -> Option<(String, String)> {
    ctx.resolve_owner_direct_import(owner_canonical, local_name)
}

pub(crate) fn enqueue_component_meta_registry_ref(
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: &str,
    source_hint: Option<&str>,
    exported_name: Option<&str>,
    route: RouteDemand,
) {
    if published_names.contains(name) {
        return;
    }
    let source_hint = source_hint
        .filter(|source| !source.is_empty())
        .map(str::to_string);
    let exported_name = exported_name
        .filter(|exported| !exported.is_empty())
        .map(str::to_string);
    if !queued_names.insert(name.to_string()) {
        if let Some(existing) = referenced_names.iter_mut().find(|pending| {
            pending.name == name
                && pending.source_hint == source_hint
                && pending.exported_name == exported_name
                && component_meta_registry_can_merge_pending_route(&pending.route, &route)
        }) {
            existing.route = crate::resolver_core::merge_route_demands(&existing.route, &route);
        } else {
            referenced_names.push_back(PendingComponentMetaRegistryRef {
                name: name.to_string(),
                source_hint,
                exported_name,
                route,
            });
        }
        return;
    }
    referenced_names.push_back(PendingComponentMetaRegistryRef {
        name: name.to_string(),
        source_hint,
        exported_name,
        route,
    });
}

fn component_meta_registry_can_merge_pending_route(
    existing: &RouteDemand,
    incoming: &RouteDemand,
) -> bool {
    let merged = crate::resolver_core::merge_route_demands(existing, incoming);
    route_demand_keeps_exact_deep_member_path(existing, &merged)
        && route_demand_keeps_exact_deep_member_path(incoming, &merged)
}

fn route_demand_keeps_exact_deep_member_path(
    requested: &RouteDemand,
    merged: &RouteDemand,
) -> bool {
    match requested {
        RouteDemand::MemberPath(path) if path.len() > 1 => match merged {
            RouteDemand::Whole => true,
            RouteDemand::MemberPath(merged_path) => merged_path == path,
            _ => false,
        },
        _ => true,
    }
}

pub(crate) fn choose_preferred_component_meta_registry_candidate(
    left: Option<verter_semantic::analysis::type_expr::TypeExpr>,
    right: Option<verter_semantic::analysis::type_expr::TypeExpr>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_non_object = component_meta_registry_has_non_object_top_level_surface(&left);
            let right_non_object = component_meta_registry_has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            if component_meta_registry_indexed_ref_penalty(&left)
                != component_meta_registry_indexed_ref_penalty(&right)
            {
                return Some(
                    if component_meta_registry_indexed_ref_penalty(&left)
                        < component_meta_registry_indexed_ref_penalty(&right)
                    {
                        left
                    } else {
                        right
                    },
                );
            }

            choose_preferred_imported_type_body(Some(left), Some(right))
        }
        (left, right) => choose_preferred_imported_type_body(left, right),
    }
}

/// Maximum recursion depth for nested object merging to prevent stack overflow
/// on deeply nested `ComponentConfig` types.
const MAX_REGISTRY_MERGE_DEPTH: u8 = 8;

pub(crate) fn merge_component_meta_registry_candidates(
    left: Option<verter_semantic::analysis::type_expr::TypeExpr>,
    right: Option<verter_semantic::analysis::type_expr::TypeExpr>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    merge_component_meta_registry_candidates_bounded(left, right, 0)
}

fn merge_component_meta_registry_candidates_bounded(
    left: Option<verter_semantic::analysis::type_expr::TypeExpr>,
    right: Option<verter_semantic::analysis::type_expr::TypeExpr>,
    depth: u8,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    fn merge_member_types(left: &TypeExpr, right: &TypeExpr, depth: u8) -> TypeExpr {
        if depth >= MAX_REGISTRY_MERGE_DEPTH {
            return left.clone();
        }
        merge_component_meta_registry_candidates_bounded(
            Some(left.clone()),
            Some(right.clone()),
            depth + 1,
        )
        .unwrap_or_else(|| left.clone())
    }

    fn merge_object_members(left: &TypeExpr, right: &TypeExpr, depth: u8) -> Option<TypeExpr> {
        let (TypeExpr::Object(left_obj), TypeExpr::Object(right_obj)) = (left, right) else {
            return None;
        };

        let mut merged_members = left_obj.properties.to_vec();
        for right_member in &right_obj.properties {
            match right_member {
                ObjectMember::Property(right_property) => {
                    if let Some(ObjectMember::Property(existing_property)) =
                        merged_members.iter_mut().find(|member| {
                            matches!(
                                member,
                                ObjectMember::Property(property)
                                    if property.name == right_property.name
                            )
                        })
                    {
                        existing_property.ty =
                            merge_member_types(&existing_property.ty, &right_property.ty, depth);
                        existing_property.optional =
                            existing_property.optional && right_property.optional;
                        existing_property.readonly =
                            existing_property.readonly && right_property.readonly;
                    } else {
                        merged_members.push(ObjectMember::Property(ObjectProperty {
                            name: right_property.name.clone(),
                            ty: right_property.ty.clone(),
                            optional: right_property.optional,
                            readonly: right_property.readonly,
                        }));
                    }
                }
                _ => {
                    if !merged_members.contains(right_member) {
                        merged_members.push(right_member.clone());
                    }
                }
            }
        }

        Some(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: merged_members,
        })))
    }

    match (left, right) {
        (Some(left), Some(right)) => merge_object_members(&left, &right, depth).or_else(|| {
            choose_preferred_component_meta_registry_candidate(Some(left), Some(right))
        }),
        (left, right) => choose_preferred_component_meta_registry_candidate(left, right),
    }
}

fn choose_preferred_imported_type_body(
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

            let left_non_object = component_meta_registry_has_non_object_top_level_surface(&left);
            let right_non_object = component_meta_registry_has_non_object_top_level_surface(&right);
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

fn imported_type_body_specificity_score(expr: &TypeExpr) -> usize {
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

pub(crate) fn component_meta_registry_has_non_object_top_level_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            component_meta_registry_has_non_object_top_level_surface(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types
                .iter()
                .any(component_meta_registry_has_non_object_top_level_surface)
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

pub(crate) fn component_meta_registry_has_explicit_object_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            component_meta_registry_has_explicit_object_surface(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(component_meta_registry_has_explicit_object_surface),
        TypeExpr::Object(_) => true,
        _ => false,
    }
}

pub(crate) fn component_meta_registry_raw_member_path_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    path: &[String],
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    /// Plan §4.19 / §6.10 sub-task 4 — inlined replacement for the
    /// retired free helper. Navigates into a `TypeExpr::Object` by a
    /// single property name, unwrapping `Parenthesized`. Returns
    /// `None` if `expr` is not an Object or no member matches.
    fn navigate_object_member<'a>(expr: &'a TypeExpr, member_name: &str) -> Option<&'a TypeExpr> {
        match expr {
            TypeExpr::Parenthesized(inner) => navigate_object_member(inner, member_name),
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => {
                    Some(&property.ty)
                }
                _ => None,
            }),
            _ => None,
        }
    }

    if path.is_empty() {
        return Some(expr.clone());
    }

    let mut leaf = expr;
    for member_name in path {
        leaf = navigate_object_member(leaf, member_name)?;
    }

    Some(path.iter().rfold(leaf.clone(), |child, member_name| {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: member_name.clone(),
                ty: child,
                optional: true,
                readonly: false,
            })],
        }))
    }))
}

pub(crate) fn component_meta_registry_expr_references_name(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    target_name: &str,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        }
        | TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            name.as_ref() == target_name
                || type_arguments
                    .iter()
                    .any(|arg| component_meta_registry_expr_references_name(arg, target_name))
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::Rest(element)
        | TypeExpr::KeyOf(element) => {
            component_meta_registry_expr_references_name(element, target_name)
        }
        TypeExpr::IndexedAccess { object, index } => {
            component_meta_registry_expr_references_name(object, target_name)
                || component_meta_registry_expr_references_name(index, target_name)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| component_meta_registry_expr_references_name(&element.ty, target_name)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| component_meta_registry_expr_references_name(ty, target_name)),
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => {
                component_meta_registry_expr_references_name(&property.ty, target_name)
            }
            ObjectMember::IndexSignature(signature) => {
                component_meta_registry_expr_references_name(&signature.key_type, target_name)
                    || component_meta_registry_expr_references_name(
                        &signature.value_type,
                        target_name,
                    )
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function.parameters.iter().any(|param| {
                    component_meta_registry_expr_references_name(&param.ty, target_name)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    component_meta_registry_expr_references_name(return_type, target_name)
                })
            }
            ObjectMember::Method(method) => {
                method.function.parameters.iter().any(|param| {
                    component_meta_registry_expr_references_name(&param.ty, target_name)
                }) || method
                    .function
                    .return_type
                    .as_deref()
                    .is_some_and(|return_type| {
                        component_meta_registry_expr_references_name(return_type, target_name)
                    })
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .any(|param| component_meta_registry_expr_references_name(&param.ty, target_name))
                || function.return_type.as_deref().is_some_and(|return_type| {
                    component_meta_registry_expr_references_name(return_type, target_name)
                })
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            component_meta_registry_expr_references_name(check, target_name)
                || component_meta_registry_expr_references_name(extends, target_name)
                || component_meta_registry_expr_references_name(true_type, target_name)
                || component_meta_registry_expr_references_name(false_type, target_name)
        }
        TypeExpr::Mapped {
            source,
            name_type,
            value,
            ..
        } => {
            component_meta_registry_expr_references_name(source, target_name)
                || name_type.as_deref().is_some_and(|name_type| {
                    component_meta_registry_expr_references_name(name_type, target_name)
                })
                || component_meta_registry_expr_references_name(value, target_name)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|expr| component_meta_registry_expr_references_name(expr, target_name)),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

pub(crate) fn component_meta_registry_indexed_ref_penalty(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> usize {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { object, index } => {
            let local_penalty = matches!(object.as_ref(), TypeExpr::Ref { .. }) as usize;
            local_penalty
                + component_meta_registry_indexed_ref_penalty(object)
                + component_meta_registry_indexed_ref_penalty(index)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
        TypeExpr::Array { element, .. }
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element) => component_meta_registry_indexed_ref_penalty(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| component_meta_registry_indexed_ref_penalty(&element.ty))
            .sum(),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(prop) => {
                    component_meta_registry_indexed_ref_penalty(&prop.ty)
                }
                ObjectMember::IndexSignature(sig) => {
                    component_meta_registry_indexed_ref_penalty(&sig.key_type)
                        + component_meta_registry_indexed_ref_penalty(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + func
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
            })
            .sum(),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            component_meta_registry_indexed_ref_penalty(check)
                + component_meta_registry_indexed_ref_penalty(extends)
                + component_meta_registry_indexed_ref_penalty(true_type)
                + component_meta_registry_indexed_ref_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            component_meta_registry_indexed_ref_penalty(source)
                + component_meta_registry_indexed_ref_penalty(value)
                + name_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
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

pub(crate) fn collect_component_meta_registry_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_member_refs: bool,
) {
    use verter_semantic::analysis::type_expr::TypeExpr;

    if let Some((root_name, route)) = component_meta_registry_public_utility_route(expr) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    if let Some((root_name, route)) = component_meta_registry_public_indexed_access_route(expr) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            for ty in types.iter() {
                collect_component_meta_registry_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        // Registry publication stays shallow: object/function member types remain
        // inline on the owning helper instead of spawning separate registry
        // entries for every nested support type. Routed helper refs still need
        // to preserve their member path here; imported deep member-path roots
        // are filtered later in `meta_resolve` once the consuming field surface
        // has already been projected concretely.
        TypeExpr::Object(obj) => {
            use verter_semantic::analysis::type_expr::ObjectMember;

            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        TypeExpr::Function(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            if !allow_plain_member_refs {
                return;
            }
            collect_component_meta_registry_refs(
                check,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                extends,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            collect_component_meta_registry_refs(
                source,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_component_meta_registry_refs(
                    name_type,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        TypeExpr::TypeParameter(_) => {}
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. } => {}
    }
}

pub(crate) fn collect_component_meta_registry_function_surface_refs(
    func: &verter_semantic::analysis::type_expr::FunctionExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    for param in &func.parameters {
        collect_component_meta_registry_member_surface_refs(
            &param.ty,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_component_meta_registry_member_surface_refs(
            return_type,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
}

pub(crate) fn collect_component_meta_registry_public_field_refs(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    let parsed_raw = (!component_meta_registry_field_expr_has_actionable_route(&field.r#type))
        .then(|| {
            field
                .raw_type
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
        })
        .flatten()
        .filter(|raw| {
            let deep_indexed_path = component_meta_registry_public_indexed_access_route(raw)
                .is_some_and(|(_, route)| {
                    matches!(
                        route,
                        RouteDemand::MemberPath(ref path) if path.len() > 1,
                    )
                });
            !deep_indexed_path || component_meta_registry_has_explicit_object_surface(&field.r#type)
        });
    let expr = parsed_raw.as_ref().unwrap_or(&field.r#type);

    let skip_direct_plain_ref = component_meta_registry_ref_name(expr).is_some_and(|name| {
        ctx.prepared_type_decl(owner_canonical, name)
            .is_some_and(|prepared| {
                matches!(
                    prepared.body,
                    verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_),
                )
            })
            || owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
                .and_then(|(canonical_id, exported_name)| {
                    (!canonical_id.is_empty() && !canonical_id.contains("/node_modules/")).then(
                        || ctx.prepared_type_decl(canonical_id.as_str(), exported_name.as_str()),
                    )
                })
                .flatten()
                .is_some_and(|prepared| {
                    component_meta_registry_has_non_object_top_level_surface(&prepared.body)
                        && !component_meta_registry_has_explicit_object_surface(&prepared.body)
                })
            || owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
                .is_some_and(|(canonical_id, _)| canonical_id.contains("/node_modules/"))
            || ctx
                .resolve_type_declaration_for_dep(owner_canonical, name)
                .canonical_source
                .contains("/node_modules/")
    });
    let skip_imported_generic_non_object_ref = component_meta_registry_direct_public_ref(expr)
        .is_some_and(|(name, type_arguments)| {
            if type_arguments.is_empty() {
                return false;
            }
            let Some((canonical_id, exported_name)) =
                owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name)
            else {
                return false;
            };
            if canonical_id.is_empty() || canonical_id.contains("/node_modules/") {
                return false;
            }
            ctx.prepared_type_decl(canonical_id.as_str(), exported_name.as_str())
                .is_some_and(|prepared| {
                    component_meta_registry_has_non_object_top_level_surface(&prepared.body)
                        && !component_meta_registry_has_explicit_object_surface(&prepared.body)
                })
        });
    let direct_ref = component_meta_registry_direct_public_ref(expr);
    if (skip_direct_plain_ref || skip_imported_generic_non_object_ref)
        && direct_ref.is_some_and(|(name, _)| {
            let local_type_parameter =
                ctx.prepared_type_decl(owner_canonical, name)
                    .is_some_and(|prepared| {
                        matches!(
                            prepared.body,
                            verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_),
                        )
                    });
            let import_root =
                owner_component_meta_registry_import_root(ctx, owner_canonical, snapshot, name);
            let package_backed = import_root
                .as_ref()
                .is_some_and(|(canonical_id, _)| canonical_id.contains("/node_modules/"))
                || ctx
                    .resolve_type_declaration_for_dep(owner_canonical, name)
                    .canonical_source
                    .contains("/node_modules/");
            !local_type_parameter && !package_backed
        })
    {
        if let Some((name, _)) = direct_ref {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name,
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
    }
    if !skip_direct_plain_ref && !skip_imported_generic_non_object_ref {
        collect_component_meta_registry_public_surface_refs(
            expr,
            published_names,
            queued_names,
            output,
            source_hint,
        );
    }

    collect_component_meta_registry_public_indexed_access_roots(
        ctx,
        owner_canonical,
        expr,
        published_names,
        queued_names,
        output,
        source_hint,
    );
}

pub(crate) fn component_meta_registry_ref_name(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<&str> {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => Some(name.as_ref()),
        TypeExpr::Parenthesized(inner) => component_meta_registry_ref_name(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_direct_public_ref(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<(&str, &[verter_semantic::analysis::type_expr::TypeExpr])> {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => Some((name.as_ref(), type_arguments.as_ref())),
        TypeExpr::Parenthesized(inner) => component_meta_registry_direct_public_ref(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_field_expr_has_actionable_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    component_meta_registry_direct_public_ref(expr).is_some()
        || component_meta_registry_public_utility_route(expr).is_some()
        || component_meta_registry_public_indexed_access_route(expr).is_some()
}

pub(crate) fn component_meta_registry_string_literal_keys(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<Vec<String>> {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
        TypeExpr::Union(types) => {
            let mut keys = Vec::new();
            for ty in types.iter() {
                keys.extend(component_meta_registry_string_literal_keys(ty)?);
            }
            keys.sort();
            keys.dedup();
            Some(keys)
        }
        TypeExpr::Parenthesized(inner) => component_meta_registry_string_literal_keys(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_public_utility_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => component_meta_registry_public_utility_route(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 2 && matches!(name.as_ref(), "Pick" | "Omit") => {
            let root_name = component_meta_registry_ref_name(&type_arguments[0])?.to_string();
            let members = component_meta_registry_string_literal_keys(&type_arguments[1])?;
            if members.is_empty() {
                return None;
            }
            let route = if name.as_ref() == "Pick" {
                RouteDemand::Pick(members)
            } else {
                RouteDemand::Omit(members)
            };
            Some((root_name, route))
        }
        _ => None,
    }
}

pub(crate) fn component_meta_registry_public_indexed_access_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn collect_path(expr: &TypeExpr, path: &mut Vec<String>) -> Option<String> {
        use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::IndexedAccess { object, index } => {
                let root = collect_path(object, path)?;
                let TypeExpr::Literal(LiteralValue::String(member)) = index.as_ref() else {
                    return None;
                };
                path.push(member.clone());
                Some(root)
            }
            TypeExpr::Parenthesized(inner) => collect_path(inner, path),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => Some(name.to_string()),
            _ => None,
        }
    }

    let mut path = Vec::new();
    let root = collect_path(expr, &mut path)?;
    if path.is_empty() {
        return None;
    }
    Some((root, RouteDemand::MemberPath(path)))
}

pub(crate) fn collect_component_meta_registry_public_surface_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::Parenthesized(element) => {
            collect_component_meta_registry_public_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Array { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. } => {}
        TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => {}
    }
}

pub(crate) fn collect_component_meta_registry_public_indexed_access_roots(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    let Some((root_name, route)) = component_meta_registry_public_indexed_access_route(expr) else {
        return;
    };
    let Some(prepared) = ctx.prepared_type_decl(owner_canonical, root_name.as_str()) else {
        return;
    };
    if matches!(
        prepared.body,
        verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_),
    ) {
        return;
    }
    enqueue_component_meta_registry_ref(
        published_names,
        queued_names,
        output,
        root_name.as_str(),
        source_hint,
        None,
        route,
    );
}

pub(crate) fn collect_component_meta_registry_member_surface_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_refs: bool,
) {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    if let Some((root_name, route)) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
    {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            None,
            route,
        );
        return;
    }

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } if allow_plain_refs => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
                RouteDemand::Whole,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_member_surface_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
            collect_component_meta_registry_member_surface_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_member_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_member_surface_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            for ty in types.iter() {
                collect_component_meta_registry_member_surface_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_component_meta_registry_member_surface_refs(
                check,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                extends,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_component_meta_registry_member_surface_refs(
                source,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_component_meta_registry_member_surface_refs(
                    name_type,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Function(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Ref { .. } => {}
    }
}

// Plan §4.19 / §6.6 / E + §6.10 sub-task 4 — both legacy
// member-path helpers retired. The shared object-member navigation
// logic is inlined into the body of
// `component_meta_registry_raw_member_path_surface` (its only
// surviving caller). The retired symbols are listed in the
// `RETIRED_SYMBOLS` array of the static-grep gate test (commit I).

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use super::{
        choose_preferred_imported_type_body, collect_component_meta_registry_refs,
        component_meta_registry_field_expr_has_actionable_route,
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_raw_member_path_surface, enqueue_component_meta_registry_ref,
        imported_type_body_specificity_score, merge_component_meta_registry_candidates,
        owner_component_meta_registry_import_root, RouteDemand,
    };
    use crate::types::{AnalysisLevel, DependencyResolution, HostConfig};
    use crate::VerterHost;
    use verter_semantic::analysis::type_expr::{
        FunctionExpr, FunctionParam, LiteralValue, MethodSignature, ObjectExpr, ObjectMember,
        ObjectProperty, PrimitiveName, TypeExpr, ValueRef,
    };

    fn object_with_props(names: &[&str]) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: names
                .iter()
                .map(|name| {
                    ObjectMember::Property(ObjectProperty {
                        name: (*name).to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    })
                })
                .collect(),
        }))
    }

    fn empty_object() -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: Vec::new(),
        }))
    }

    #[test]
    fn indexed_access_route_preserves_full_member_path() {
        let expr = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
            "Button['variants']['color']",
        );

        assert_eq!(
            component_meta_registry_public_indexed_access_route(&expr),
            Some((
                "Button".to_string(),
                RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
            ))
        );
    }

    #[test]
    fn collect_registry_refs_preserves_indexed_access_member_path() {
        let expr =
            verter_semantic::analysis::type_expr_lower::parse_type_annotation("Button['ui']");
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();

        collect_component_meta_registry_refs(
            &expr,
            &published_names,
            &mut queued_names,
            &mut output,
            Some("/src/Button.vue"),
            false,
        );

        let pending = output
            .pop_front()
            .expect("indexed-access helper should enqueue a registry ref");
        assert_eq!(pending.name, "Button");
        assert_eq!(pending.source_hint.as_deref(), Some("/src/Button.vue"));
        assert_eq!(pending.exported_name, None);
        assert_eq!(
            pending.route,
            RouteDemand::MemberPath(vec!["ui".to_string()]),
            "indexed-access helper refs should preserve the requested member path instead of widening to Whole",
        );
        assert!(
            output.is_empty(),
            "indexed-access helper refs should enqueue only the routed root helper"
        );
    }

    #[test]
    fn enqueue_registry_ref_keeps_deep_member_paths_separate_from_top_level_picks() {
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();

        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            None,
            RouteDemand::Pick(vec!["slots".to_string()]),
        );
        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            None,
            RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
        );

        assert_eq!(
            output.len(),
            2,
            "deep member-path requests must not be collapsed into a top-level pick for the same root"
        );
        assert_eq!(
            output[0].route,
            RouteDemand::Pick(vec!["slots".to_string()]),
        );
        assert_eq!(
            output[1].route,
            RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]),
        );
    }

    #[test]
    fn merge_registry_candidates_combines_partial_object_routes() {
        let left = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "slots".to_string(),
                ty: object_with_props(&["base", "label"]),
                optional: true,
                readonly: false,
            })],
        }));
        let right = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "variants".to_string(),
                ty: TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "color".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::string_literal("primary"),
                            TypeExpr::string_literal("secondary"),
                        ]),
                        optional: false,
                        readonly: false,
                    })],
                })),
                optional: true,
                readonly: false,
            })],
        }));

        let merged = merge_component_meta_registry_candidates(Some(left), Some(right))
            .expect("partial route candidates should merge");
        let TypeExpr::Object(shape) = merged else {
            panic!("merged partial route candidates should stay object-shaped");
        };

        assert_eq!(
            shape.properties.len(),
            2,
            "merged object should have exactly 2 properties (slots, variants)"
        );
        assert!(shape.properties.iter().any(|member| {
            matches!(member, ObjectMember::Property(property) if property.name == "slots")
        }));
        let variants = shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variants" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("merged surface should keep variants");
        let TypeExpr::Object(variants_shape) = variants else {
            panic!("variants should stay object-shaped after merge, got {variants:?}");
        };
        assert_eq!(
            variants_shape.properties.len(),
            1,
            "variants should have exactly 1 property (color)"
        );
        assert!(variants_shape.properties.iter().any(|member| {
            matches!(member, ObjectMember::Property(property) if property.name == "color")
        }));
    }

    #[test]
    fn field_expr_actionable_route_recognizes_direct_generic_refs() {
        let expr = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
            "GetModelValue<T, VK, true>",
        );

        assert!(
            component_meta_registry_field_expr_has_actionable_route(&expr),
            "direct public generic refs should reuse the existing symbolic field expr instead of reparsing raw_type",
        );
    }

    #[test]
    fn field_expr_actionable_route_rejects_expanded_object_surfaces() {
        let expr = object_with_props(&["base", "label"]);

        assert!(
            !component_meta_registry_field_expr_has_actionable_route(&expr),
            "expanded object surfaces still need raw_type reparsing when the raw annotation carries a routed helper like Button['ui']",
        );
    }

    #[test]
    fn owner_import_root_resolves_named_imports_through_barrels() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { AvatarProps } from './types'

export interface Props {
  avatar?: AvatarProps
}

defineProps<Props>()
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from("export * from './Alert.vue'\nexport * from './Avatar.vue'\n"),
        );
        ws.inject_file(
            "/src/Alert.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AlertProps {
  title?: string
}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/types.ts",
            vec![
                DependencyResolution {
                    specifier: "./Alert.vue".to_string(),
                    resolved_canonical_id: Some("/src/Alert.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                DependencyResolution {
                    specifier: "./Avatar.vue".to_string(),
                    resolved_canonical_id: Some("/src/Avatar.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let snapshot = host
            .get_raw_analysis_snapshot("/src/App.vue")
            .expect("app snapshot should exist");

        let resolved = owner_component_meta_registry_import_root(
            &host,
            "/src/App.vue",
            &snapshot,
            "AvatarProps",
        );

        assert_eq!(
            resolved,
            Some(("/src/Avatar.vue".to_string(), "AvatarProps".to_string())),
            "registry import roots should collapse direct named owner imports to the canonical defining file instead of keeping the barrel canonical",
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_more_specific_shapes() {
        let resolved_body = Some(TypeExpr::named("Props"));
        let decl_body = Some(object_with_props(&["label", "count"]));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "the body with the richer concrete surface should win"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_keeps_existing_body_on_equal_specificity() {
        let left = object_with_props(&["label"]);
        let right = object_with_props(&["count"]);

        let chosen = choose_preferred_imported_type_body(Some(left.clone()), Some(right));

        assert_eq!(
            chosen,
            Some(left),
            "equal scores should preserve the first successful resolution"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_rejects_empty_object_placeholders() {
        let resolved_body = Some(empty_object());
        let decl_body = Some(TypeExpr::union(vec![
            TypeExpr::Literal(LiteralValue::String("to".to_string())),
            TypeExpr::Literal(LiteralValue::String("replace".to_string())),
        ]));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "empty-object placeholders must not outrank concrete literal-union aliases"
        );
    }

    #[test]
    fn imported_type_body_specificity_prefers_object_surfaces_over_refs_and_typeof() {
        let typeof_score = imported_type_body_specificity_score(&TypeExpr::TypeOf(ValueRef {
            path: vec!["theme".to_string()],
        }));
        let ref_score = imported_type_body_specificity_score(&TypeExpr::named("Props"));
        let object_score = imported_type_body_specificity_score(&object_with_props(&["label"]));

        assert!(
            typeof_score < ref_score && ref_score < object_score,
            "specificity ordering should keep typeof < ref < object, got typeof={typeof_score} ref={ref_score} object={object_score}"
        );
    }

    #[test]
    fn imported_type_body_specificity_rewards_richer_object_surfaces() {
        let small = imported_type_body_specificity_score(&object_with_props(&["label"]));
        let large = imported_type_body_specificity_score(&object_with_props(&["label", "count"]));

        assert!(
            large > small,
            "object surfaces with more top-level members should score higher, got small={small} large={large}"
        );
    }

    #[test]
    fn choose_preferred_imported_type_body_prefers_richer_object_surface_with_nested_members() {
        let resolved_body = Some(object_with_props(&["next"]));
        let decl_body = Some(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "base".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "current".to_string(),
                    ty: TypeExpr::named("T"),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "next".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: true,
                    readonly: false,
                }),
            ],
        })));

        let chosen = choose_preferred_imported_type_body(resolved_body, decl_body.clone());

        assert_eq!(
            chosen, decl_body,
            "a richer concrete object surface should beat a smaller local-eval object even when one member type stays symbolic"
        );
    }

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
            properties: vec![ObjectMember::Method(MethodSignature {
                name: "default".to_string(),
                function,
                optional: true,
            })],
        }));

        let preferred =
            choose_preferred_imported_type_body(Some(property_object), Some(method_object.clone()))
                .expect("preferred body should exist");

        assert_eq!(preferred, method_object);
    }

    #[test]
    fn raw_member_path_surface_projects_explicit_object_members_without_widening() {
        let raw = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "ui".to_string(),
                    ty: TypeExpr::Object(Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "base".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                        })],
                    })),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "label".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
            ],
        }));

        let projected = component_meta_registry_raw_member_path_surface(&raw, &["ui".to_string()])
            .expect("explicit object surface should project the requested member path");

        let TypeExpr::Object(shape) = projected else {
            panic!("member path projection should stay object-shaped, got {projected:?}");
        };
        let member_names: Vec<_> = shape
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            vec!["ui"],
            "raw member path projection should stay on the requested member instead of widening to siblings"
        );
    }
}
