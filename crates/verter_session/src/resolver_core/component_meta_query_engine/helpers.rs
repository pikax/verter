//! Predicate and utility helpers extracted from
//! `component_meta_query_engine/mod.rs` in the prior cutover.3.
//!
//! These helpers are pure free functions used by the engine impl
//! methods to classify type expressions, route demands, prepared type
//! declarations, and registry symbols. They have no engine-state
//! dependencies and access only the parent module's re-exported types
//! (`ResolvedImportedRegistrySymbol`, `RouteDemand`) plus shared
//! semantic types from `verter_semantic`.
//!
//! Visibility: every symbol is `pub(super)` — the parent `mod.rs`
//! engine impl calls them without re-exporting them outside the
//! folder module.

use std::collections::BTreeSet;

use verter_semantic::analysis::type_expr::TypeExpr;

use super::surface::type_expr_references_names;
use super::ResolvedImportedRegistrySymbol;
use crate::resolver_core::ResolverContext;

#[allow(dead_code)]
pub(super) fn routed_expr_surface_key_expr(
    root_symbol: &str,
    route: &super::super::RouteDemand,
) -> Option<TypeExpr> {
    match route {
        super::super::RouteDemand::Whole => Some(TypeExpr::named(root_symbol)),
        super::super::RouteDemand::MemberPath(path) if !path.is_empty() => Some(path.iter().fold(
            TypeExpr::named(root_symbol),
            |object, member| TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(object),
                index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
            },
        )),
        super::super::RouteDemand::Pick(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Pick"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        super::super::RouteDemand::Omit(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Omit"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        _ => None,
    }
}

pub(super) fn is_package_source(source: Option<&str>) -> bool {
    source.is_some_and(|s| s.contains("/node_modules/"))
}

pub(super) fn is_package_canonical(canonical_id: &str) -> bool {
    canonical_id.contains("/node_modules/") || canonical_id.contains("\\node_modules\\")
}

pub(super) fn strip_parens_expr(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => strip_parens_expr(inner),
        other => other,
    }
}

pub(super) fn prepared_member_body_stays_shallow(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::TypeOf(_) => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            prepared_member_body_stays_shallow(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            !types.is_empty() && types.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Array { element, .. } => prepared_member_body_stays_shallow(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| prepared_member_body_stays_shallow(&element.ty)),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Function(function) => {
            function.type_parameters.iter().all(|parameter| {
                parameter
                    .constraint
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
                    && parameter
                        .default
                        .as_deref()
                        .is_none_or(prepared_member_body_stays_shallow)
            }) && function
                .parameters
                .iter()
                .all(|parameter| prepared_member_body_stays_shallow(&parameter.ty))
                && function
                    .return_type
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
        }
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. } => false,
    }
}

pub(super) fn prepared_decl_keeps_raw_symbolic_non_object_alias(
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    expr: &TypeExpr,
) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            prepared
                .name_resolution
                .get(name.as_ref())
                .is_some_and(|resolved| resolved.canonical_id.contains("/node_modules/"))
                && type_arguments
                    .iter()
                    .all(|arg| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, arg))
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, element)
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &element.ty)
        }),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types
            .iter()
            .all(|ty| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, ty)),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .all(|param| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &param.ty))
                && func.return_type.as_deref().is_none_or(|return_type| {
                    prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, return_type)
                })
                && func.type_parameters.iter().all(|param| {
                    param.constraint.as_deref().is_none_or(|constraint| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, constraint)
                    }) && param.default.as_deref().is_none_or(|default| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, default)
                    })
                })
        }
        TypeExpr::Object(object) => object.properties.is_empty(),
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_) => false,
    }
}

pub(super) fn is_builtin_name(name: &str) -> bool {
    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name).is_some()
        || matches!(name, "Array" | "ReadonlyArray" | "Promise")
}

pub(super) fn prepared_type_decl_canonical_dependencies(
    resolved_id: &str,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
) -> BTreeSet<String> {
    let mut canonical_dependencies = BTreeSet::from([resolved_id.to_string()]);
    if let Some((defining_file, _)) = prepared.cache_deps.defining_file.as_ref() {
        canonical_dependencies.insert(defining_file.clone());
    }
    for (participant, _) in &prepared.cache_deps.barrel_participants {
        canonical_dependencies.insert(participant.clone());
    }
    for dep in &prepared.external_deps {
        if !dep.canonical_id.is_empty() {
            canonical_dependencies.insert(dep.canonical_id.clone());
        }
    }
    for identity in prepared.name_resolution.values() {
        if !identity.canonical_id.is_empty() {
            canonical_dependencies.insert(identity.canonical_id.clone());
        }
    }
    canonical_dependencies
}

pub(super) fn resolve_imported_registry_symbol_with_budget<F>(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exported_name: &str,
    mut allow_route: F,
) -> Option<ResolvedImportedRegistrySymbol>
where
    F: FnMut() -> bool,
{
    let (resolved_id, resolved_name) = if ctx
        .prepared_type_decl(canonical_id, exported_name)
        .is_some()
    {
        (canonical_id.to_string(), exported_name.to_string())
    } else {
        if !allow_route() {
            return None;
        }
        ctx.resolve_named_type_export_target_shallow(canonical_id, exported_name)?
    };

    let prepared = ctx.prepared_type_decl(&resolved_id, &resolved_name)?;

    Some(ResolvedImportedRegistrySymbol {
        canonical_id: resolved_id.clone(),
        exported_name: resolved_name,
        body: prepared.body.clone(),
        canonical_dependencies: prepared_type_decl_canonical_dependencies(
            resolved_id.as_str(),
            prepared.as_ref(),
        ),
    })
}

pub(super) fn type_expr_references_type_params(
    expr: &TypeExpr,
    type_params: &[verter_semantic::analysis::type_expr::TypeParam],
) -> bool {
    type_expr_references_names(expr, &|name| {
        type_params.iter().any(|param| param.name == name)
    })
}

pub(super) fn projected_surface_member_names(expr: &TypeExpr) -> Option<Vec<String>> {
    use verter_semantic::analysis::type_expr::ObjectMember;

    match expr {
        TypeExpr::Object(object) => {
            let mut members = Vec::new();
            for member in object.properties.iter() {
                match member {
                    ObjectMember::Property(property) => members.push(property.name.clone()),
                    ObjectMember::Method(method) => members.push(method.name.clone()),
                    _ => {}
                }
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
            let mut members = Vec::new();
            for part in parts.iter() {
                members.extend(projected_surface_member_names(part)?);
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Parenthesized(inner) => projected_surface_member_names(inner),
        _ => None,
    }
}

pub(super) fn string_literal_keys_type_expr(mut keys: Vec<String>) -> Option<TypeExpr> {
    keys.sort();
    keys.dedup();
    match keys.len() {
        0 => None,
        1 => Some(TypeExpr::string_literal(keys.pop().unwrap())),
        _ => Some(TypeExpr::Union(std::sync::Arc::from(
            keys.into_iter()
                .map(TypeExpr::string_literal)
                .collect::<Vec<_>>(),
        ))),
    }
}
