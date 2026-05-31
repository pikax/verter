//! Predicate and utility helpers used by `ComponentMetaQueryEngine`
//! impl methods to classify type expressions, route demands, prepared
//! type declarations, and registry symbols.
//!
//! Pure free functions with no engine-state dependencies; they access
//! only the parent module's re-exported types
//! (`ResolvedImportedRegistrySymbol`, `RouteDemand`) plus shared
//! semantic types from `verter_semantic`.
//!
//! Visibility: every symbol is `pub(super)` — the parent `mod.rs`
//! engine impl calls them without re-exporting them outside the
//! folder module.

use std::collections::BTreeSet;

use verter_type_expr::TypeExpr;

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

/// Thin `Option<&str>` wrapper over [`is_package_canonical`]. After the
/// prepared-structural-substitution slow-lane deletion its only remaining
/// consumer is the workspace-classification guard test, so it is gated to
/// test builds (the production path uses [`is_package_canonical`] on a
/// concrete `&str`).
#[cfg(test)]
pub(super) fn is_package_source(ctx: &dyn ResolverContext, source: Option<&str>) -> bool {
    source.is_some_and(|s| ctx.workspace_is_package_backed(s))
}

pub(super) fn is_package_canonical(ctx: &dyn ResolverContext, canonical_id: &str) -> bool {
    ctx.workspace_is_package_backed(canonical_id)
}

pub(super) fn strip_parens_expr(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => strip_parens_expr(inner),
        other => other,
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
    type_params: &[verter_type_expr::TypeParam],
) -> bool {
    type_expr_references_names(expr, &|name| {
        type_params.iter().any(|param| param.name == name)
    })
}

pub(super) fn projected_surface_member_names(expr: &TypeExpr) -> Option<Vec<String>> {
    use verter_type_expr::ObjectMember;

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

