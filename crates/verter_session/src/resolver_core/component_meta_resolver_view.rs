//! Query-local resolver view for component-meta.
//!
//! Caches route/declaration/resolvability lookups for one component-meta
//! query. All underlying data lives in host-owned shared caches. This view
//! is a thin per-query memo that avoids repeated lookups for the same
//! `(canonical_id, symbol_name)` pair within one query.

use rustc_hash::FxHashMap;

use super::declaration_metadata::ResolvedTypeDeclaration;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Cached import route: (resolved_canonical_id, resolved_exported_name, prepared alias).
type PreparedAliasEntry = Option<(String, String, super::CachedPreparedImportedTypeAlias)>;

/// Query-local resolver view for component-meta.
///
/// Wraps the host's resolution methods with per-query caching.
/// The view does NOT own any file state — all data comes from host-owned
/// shared caches (`imported_dependency_cache`, `shallow_file_state`, etc.).
pub struct ComponentMetaResolverView<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Cached import-route resolutions.
    routes: FxHashMap<(String, String), PreparedAliasEntry>,
    /// Cached type declarations.
    declarations: FxHashMap<(String, String), ResolvedTypeDeclaration>,
    /// Cached resolvability checks.
    resolvable: FxHashMap<(String, String), bool>,
    /// Cached owner collection expressions.
    owner_collection_exprs:
        FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>,
}

impl<'a> ComponentMetaResolverView<'a> {
    pub fn new(host: &'a VerterHost, store_view: Option<&'a HostStoreView>) -> Self {
        Self {
            host,
            store_view,
            routes: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
        }
    }

    /// Pre-seed routes for the initial registry entries from imported sources.
    pub fn pre_seed_routes(
        &mut self,
        registry_meta: &[super::component_meta::ResolvedTypeRegistryMeta],
        owner_canonical: &str,
    ) {
        for meta in registry_meta {
            let source = meta.declaration.canonical_source.as_str();
            if source.is_empty() || source == owner_canonical {
                continue;
            }
            let name = if meta.declaration.resolved_name.is_empty() {
                meta.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let _ = self.resolve_prepared_alias(source, name);
        }
    }

    /// Resolve a prepared import alias, cached per query.
    pub fn resolve_prepared_alias(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> PreparedAliasEntry {
        let key = (canonical_id.to_string(), exported_name.to_string());
        self.routes
            .entry(key)
            .or_insert_with_key(|_| {
                self.host.resolve_prepared_symbol_dependency_alias_in_view(
                    canonical_id,
                    exported_name,
                    self.store_view,
                )
            })
            .clone()
    }

    /// Resolve a type declaration, cached per query.
    /// Uses the host-level `resolve_type_declaration_in_view`.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        self.declarations
            .entry(key)
            .or_insert_with_key(|_| {
                crate::meta_resolve::resolve_type_declaration_in_view(
                    self.host,
                    canonical_source,
                    requested_name,
                    self.store_view,
                )
            })
            .clone()
    }

    /// Check if a registry ref can resolve, cached per query.
    pub fn can_resolve(
        &mut self,
        owner_canonical: &str,
        exported_name: &str,
        source_hint: Option<&str>,
    ) -> bool {
        if is_builtin_name(exported_name) {
            return false;
        }
        let source_key = source_hint
            .filter(|s| !s.is_empty())
            .unwrap_or(owner_canonical);
        let key = (source_key.to_string(), exported_name.to_string());
        *self.resolvable.entry(key).or_insert_with_key(|_| {
            can_resolve_ref(
                self.host,
                owner_canonical,
                exported_name,
                source_hint,
                self.store_view,
            )
        })
    }

    /// Get the owner's collection expression for a name, cached per query.
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        self.owner_collection_exprs
            .entry(name.to_string())
            .or_insert_with_key(|_| {
                self.host
                    .prepared_type_decl_in_view(owner_canonical, name, self.store_view)
                    .map(|prepared| prepared.body.clone())
            })
            .clone()
    }

    /// Number of cached routes. For diagnostics.
    pub fn routes_count(&self) -> usize {
        self.routes.len()
    }
}

fn is_builtin_name(name: &str) -> bool {
    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name).is_some()
        || matches!(name, "Array" | "ReadonlyArray" | "Promise")
}

fn can_resolve_ref(
    host: &VerterHost,
    owner_canonical: &str,
    exported_name: &str,
    source_hint: Option<&str>,
    store_view: Option<&HostStoreView>,
) -> bool {
    let source = source_hint
        .filter(|source| !source.is_empty())
        .unwrap_or(owner_canonical);

    if host
        .prepared_type_decl_in_view(source, exported_name, store_view)
        .is_some()
    {
        return true;
    }

    if let Some((resolved_id, resolved_name, _)) =
        host.resolve_prepared_symbol_dependency_alias_in_view(source, exported_name, store_view)
    {
        if (resolved_id != source || resolved_name != exported_name)
            && host
                .prepared_type_decl_in_view(&resolved_id, &resolved_name, store_view)
                .is_some()
        {
            return true;
        }
    }

    false
}
