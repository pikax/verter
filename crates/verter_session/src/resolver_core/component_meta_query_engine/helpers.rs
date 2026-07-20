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

use super::ResolvedImportedRegistrySymbol;
use crate::resolver_core::ResolverContext;

/// Thin `Option<&str>` wrapper over [`is_package_canonical`]. Its only
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
            canonical_dependencies.insert(identity.canonical_id.as_ref().to_string());
        }
    }
    canonical_dependencies
}

/// Outcome of [`resolve_imported_registry_symbol_with_budget`].
///
/// The wildcard-route fuse-exhaustion case (`allow_route()` returned
/// `false` because the per-request `wildcard_route_fanout` fuse was
/// already spent) is a GENUINE PARTIAL — the symbol was NEVER actually
/// looked up, so its absence is an artefact of the budget, not of the
/// type graph. It MUST be distinguished from a genuine ABSENT (the
/// symbol was looked up and is not exported / has no prepared decl):
/// admitting a fuse-trip `None` into `ImportedRegistryDb` as a warm
/// negative would poison subsequent identical requests that DO have
/// budget. The cold-path caller marks the request partial sticky on
/// `FuseTripped` and returns `ReturnOnly(None)` rather than a cacheable
/// negative.
pub(super) enum ImportedRegistrySymbolResolution {
    /// The symbol was looked up. `Some` = resolved; `None` = genuinely
    /// absent (not exported / no prepared decl). Either is a complete,
    /// cacheable verdict.
    Resolved(Option<ResolvedImportedRegistrySymbol>),
    /// The wildcard route was needed but the per-request fuse was
    /// exhausted, so the symbol was NOT looked up. A genuine partial —
    /// do NOT admit a warm negative.
    FuseTripped,
}

pub(super) fn resolve_imported_registry_symbol_with_budget<F>(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    source_owner: verter_type_expr::TopLevelOwnerId,
    exported_name: &str,
    mut allow_route: F,
) -> ImportedRegistrySymbolResolution
where
    F: FnMut() -> bool,
{
    let resolved = if ctx
        .prepared_type_decl_return_only(canonical_id, source_owner, exported_name)
        .is_some()
    {
        verter_semantic::analysis::type_solver::host::ResolvedRootIdentity::new_in_owner(
            canonical_id,
            source_owner,
            exported_name,
        )
    } else if source_owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
        if let Some(crate::resolver_core::ExportTarget::Local { owner, symbol_name }) = ctx
            .shallow_file_state(canonical_id)
            .and_then(|state| state.export_target(exported_name).cloned())
        {
            verter_semantic::analysis::type_solver::host::ResolvedRootIdentity::new_in_owner(
                canonical_id,
                owner,
                symbol_name,
            )
        } else {
            if !allow_route() {
                // Fuse exhaustion — the route was NOT taken, so this is a
                // partial, not an absent. Distinguish it explicitly.
                return ImportedRegistrySymbolResolution::FuseTripped;
            }
            let (resolved, route_facts) =
                ctx.resolve_imported_type_root_with_facts(canonical_id, exported_name);
            ctx.observe_borrowed_signature(&route_facts);
            let Some(resolved) = resolved else {
                return ImportedRegistrySymbolResolution::Resolved(None);
            };
            resolved
        }
    } else {
        return ImportedRegistrySymbolResolution::Resolved(None);
    };

    let Some(prepared) = ctx.prepared_type_decl_return_only(
        &resolved.canonical_id,
        resolved.owner,
        &resolved.symbol_name,
    ) else {
        return ImportedRegistrySymbolResolution::Resolved(None);
    };

    ImportedRegistrySymbolResolution::Resolved(Some(ResolvedImportedRegistrySymbol {
        canonical_id: resolved.canonical_id.to_string(),
        owner: resolved.owner,
        exported_name: resolved.symbol_name.to_string(),
        body: prepared.body_facts.clone(),
        canonical_dependencies: prepared_type_decl_canonical_dependencies(
            resolved.canonical_id.as_ref(),
            prepared.as_ref(),
        ),
    }))
}
