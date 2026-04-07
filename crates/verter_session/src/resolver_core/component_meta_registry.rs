//! Component-meta registry publication.
//!
//! This module owns registry queueing, route merging, and publication
//! policy for routed component-meta type publication.
//!
//! See architectural rule 10: "Component-meta publication stays in resolver_core."

use std::collections::VecDeque;

use crate::resolver_core::RouteDemand;
use crate::types::FileAnalysisSnapshot;
use crate::VerterHost;

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
        let preferred = choose_preferred_component_meta_registry_candidate(
            Some(existing.clone()),
            Some(type_expr),
        )
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

pub(crate) fn owner_component_meta_registry_import_binding(
    snapshot: &FileAnalysisSnapshot,
    local_name: &str,
) -> Option<(String, String)> {
    snapshot.imports.iter().find_map(|import| {
        let canonical_id = import.resolved_canonical_id.as_ref()?;
        let binding = import
            .bindings
            .iter()
            .find(|binding| binding.name == local_name)?;
        let exported_name = binding
            .imported_name
            .clone()
            .unwrap_or_else(|| local_name.to_string());
        Some((canonical_id.clone(), exported_name))
    })
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
        if let Some(existing) = referenced_names
            .iter_mut()
            .find(|pending| pending.name == name)
        {
            if existing.source_hint.is_none() {
                existing.source_hint = source_hint;
            }
            if existing.exported_name.is_none() {
                existing.exported_name = exported_name;
            }
            existing.route = crate::resolver_core::merge_route_demands(&existing.route, &route);
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

            crate::resolver_core::choose_preferred_imported_type_body(Some(left), Some(right))
        }
        (left, right) => crate::resolver_core::choose_preferred_imported_type_body(left, right),
    }
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
        // entries for every nested support type. We still need to notice
        // direct member-surface helper refs such as `Button['variants']['color']`
        // or `LocalConfig<string>['slot']`, because compat display/schema output
        // depends on those helpers being present in the registry.
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
    host: &VerterHost,
    owner_canonical: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    let parsed_raw = field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
    let expr = parsed_raw.as_ref().unwrap_or(&field.r#type);

    collect_component_meta_registry_public_surface_refs(
        expr,
        published_names,
        queued_names,
        output,
        source_hint,
    );

    collect_component_meta_registry_public_indexed_access_roots(
        host,
        owner_canonical,
        store_view,
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
    host: &VerterHost,
    owner_canonical: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    use verter_semantic::analysis::type_eval::TypeDeclKind;
    let Some((root_name, route)) = component_meta_registry_public_indexed_access_route(expr) else {
        return;
    };
    let Some(prepared) =
        host.prepared_type_decl_in_view(owner_canonical, root_name.as_str(), store_view)
    else {
        return;
    };
    if !matches!(prepared.kind, TypeDeclKind::Interface | TypeDeclKind::Class) {
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

#[cfg(test)]
mod tests {
    use super::{component_meta_registry_public_indexed_access_route, RouteDemand};

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
}
