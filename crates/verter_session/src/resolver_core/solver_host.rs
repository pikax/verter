//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! prepared-declaration bundles and module-facts caches.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    BareRefOrigin, RequestStatus, ResolvedRootIdentity, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::host_manage::component_meta_trace_event;
use crate::VerterHost;

use super::prepared_decl::ImportBinding;

#[derive(Debug)]
pub(crate) struct DeclarationScopePayload {
    scope_type_names: FxHashSet<String>,
    scope_value_names: FxHashSet<String>,
    scope_type_bindings: FxHashMap<String, Arc<PreparedTypeDecl>>,
    import_bindings: FxHashMap<String, ImportBinding>,
}

impl DeclarationScopePayload {
    pub(crate) fn from_bundle(
        bundle: &crate::resolver_core::prepared_decl::PreparedDeclBundle,
    ) -> Self {
        // Include script-setup generic param names in type_names so the
        // solver recognises them as in-scope.
        let mut scope_type_names = bundle.scope_type_names.clone();
        for param_name in bundle.script_setup_type_bindings.keys() {
            scope_type_names.insert(param_name.clone());
        }

        Self {
            scope_type_names,
            scope_value_names: bundle.scope_value_names.clone(),
            scope_type_bindings: bundle.script_setup_type_bindings.clone(),
            import_bindings: bundle.import_bindings.clone(),
        }
    }
}

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Declaration-scoped same-file prepared declarations (via `PreparedDeclBundle`)
/// 2. Import bindings (local name -> canonical_id + exported name)
/// 3. Host-owned prepared decl caches for cross-file lookups
pub struct SessionSolverHost<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::host_request_view::RequestStoreView>,
    /// Canonical file scope for declaration-scoped solving.
    scope_canonical_id: Option<String>,
    /// Shared declaration-scope payload derived from the host-owned prepared bundle.
    scope_payload: Option<Arc<DeclarationScopePayload>>,
}

impl<'a> SessionSolverHost<'a> {
    pub fn new(
        host: &'a VerterHost,
        store_view: Option<&'a crate::host_request_view::RequestStoreView>,
    ) -> Self {
        Self {
            host,
            store_view,
            scope_canonical_id: None,
            scope_payload: None,
        }
    }

    /// Create a solver host scoped to one declaration file's cached
    /// `PreparedDeclBundle`.
    ///
    /// All declaration-scope data (symbol names, import bindings, script-setup
    /// generics) is read from the host-owned bundle — the single authority for
    /// declaration-scope bundle materialization and reuse.
    pub fn with_declaration_scope(
        host: &'a VerterHost,
        store_view: Option<&'a crate::host_request_view::RequestStoreView>,
        declaration_canonical_id: &str,
    ) -> Self {
        if let Some(bundle) =
            host.prepared_decl_bundle_in_view(declaration_canonical_id, store_view)
        {
            Self::from_scope_payload(
                host,
                store_view,
                declaration_canonical_id,
                Arc::new(DeclarationScopePayload::from_bundle(&bundle)),
            )
        } else {
            Self::new(host, store_view)
        }
    }

    /// Build a solver host from a pre-fetched `PreparedDeclBundle`, avoiding
    /// a repeated bundle lookup when the caller already has the bundle cached.
    pub fn from_bundle(
        host: &'a VerterHost,
        store_view: Option<&'a crate::host_request_view::RequestStoreView>,
        declaration_canonical_id: &str,
        bundle: &crate::resolver_core::prepared_decl::PreparedDeclBundle,
    ) -> Self {
        Self::from_scope_payload(
            host,
            store_view,
            declaration_canonical_id,
            Arc::new(DeclarationScopePayload::from_bundle(bundle)),
        )
    }

    pub(crate) fn from_scope_payload(
        host: &'a VerterHost,
        store_view: Option<&'a crate::host_request_view::RequestStoreView>,
        declaration_canonical_id: &str,
        scope_payload: Arc<DeclarationScopePayload>,
    ) -> Self {
        Self {
            host,
            store_view,
            scope_canonical_id: Some(declaration_canonical_id.to_string()),
            scope_payload: Some(scope_payload),
        }
    }

    #[cfg(test)]
    pub(crate) fn debug_scope_storage_ptr(&self) -> usize {
        self.scope_payload
            .as_ref()
            .map(|payload| Arc::as_ptr(payload) as usize)
            .unwrap_or(0)
    }

    fn cached_module_facts(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ModuleFacts>> {
        self.host
            .ensure_module_facts_in_view(canonical_id, self.store_view)
    }

    fn symbol_exists_in_facts(
        &self,
        entry: &crate::resolver_core::ModuleFacts,
        symbol_name: &str,
    ) -> bool {
        entry.shallow_state.symbol(symbol_name).is_some()
            || entry.shallow_state.value_symbol(symbol_name).is_some()
    }

    fn resolve_import_binding_from_facts(
        &self,
        canonical_id: &str,
        local_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        if let Some(entry) = self.cached_module_facts(canonical_id) {
            let state = &entry.shallow_state;
            if let Some(target) = state.import_target(local_name) {
                let resolved_id = if target.canonical_id.is_empty() {
                    self.host.resolve_type_dependency_canonical_in_view(
                        canonical_id,
                        &target.source_specifier,
                        self.store_view,
                    )?
                } else {
                    target.canonical_id.clone()
                };
                return Some(
                    self.resolve_imported_type_root_identity(&resolved_id, &target.imported_name),
                );
            }
        }

        let bundle = self
            .host
            .prepared_decl_bundle_in_view(canonical_id, self.store_view)?;
        let binding = bundle.import_bindings.get(local_name)?;
        Some(
            self.resolve_imported_type_root_identity(&binding.canonical_id, &binding.exported_name),
        )
    }

    fn resolve_namespace_member_from_facts(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        let dot_pos = symbol_name.find('.')?;
        let prefix = &symbol_name[..dot_pos];
        let member = &symbol_name[dot_pos + 1..];
        let binding = self.resolve_import_binding_from_facts(canonical_id, prefix)?;

        let target_entry = self.cached_module_facts(&binding.canonical_id)?;
        if self.symbol_exists_in_facts(&target_entry, member) {
            return Some(ResolvedRootIdentity::new(&binding.canonical_id, member));
        }

        let target_state = &target_entry.shallow_state;
        match target_state.export_target(member) {
            Some(crate::resolver_core::ExportTarget::Local { symbol_name }) => Some(
                ResolvedRootIdentity::new(&binding.canonical_id, symbol_name),
            ),
            _ => self
                .host
                .resolve_named_type_export_target_in_view(
                    &binding.canonical_id,
                    member,
                    self.store_view,
                )
                .map(|(canonical_id, exported_name)| {
                    ResolvedRootIdentity::new(&canonical_id, &exported_name)
                })
                .or_else(|| {
                    self.host
                        .resolve_value_export_target_in_view(
                            &binding.canonical_id,
                            member,
                            self.store_view,
                        )
                        .map(|target| ResolvedRootIdentity::new(&target.canonical_id, &target.name))
                }),
        }
    }

    fn resolve_imported_type_root_identity(
        &self,
        canonical_id: &str,
        exported_name: &str,
    ) -> ResolvedRootIdentity {
        if canonical_id.is_empty() {
            return ResolvedRootIdentity::new(canonical_id, exported_name);
        }

        let (resolved_canonical_id, resolved_symbol_name) = self
            .host
            .resolve_imported_type_root_in_view(canonical_id, exported_name, self.store_view);
        ResolvedRootIdentity::new(resolved_canonical_id, resolved_symbol_name)
    }
}

impl TypeSolverHost for SessionSolverHost<'_> {
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        if let (Some(scope_canonical_id), Some(scope_payload)) = (
            self.scope_canonical_id.as_deref(),
            self.scope_payload.as_ref(),
        ) {
            if root_identity.canonical_id == scope_canonical_id {
                if let Some(bound) = scope_payload
                    .scope_type_bindings
                    .get(&root_identity.symbol_name)
                {
                    component_meta_trace_event!(
                        "solver_resolve_prepared_type_decl_result",
                        format!(
                            "root={}::{} source=scope_binding hit=true store_view={}",
                            root_identity.canonical_id,
                            root_identity.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(Arc::clone(bound));
                }
            }
        }

        // Resolve from the host-owned prepared decl cache.
        if let Some(prepared) = self.host.prepared_type_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        ) {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=direct_prepared hit=true store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return Some(prepared);
        }

        // Declaration-scoped name resolution and import bindings may point at
        // a shallow import target that is itself a barrel. Follow the cached
        // export route once here so the solver still reads the final prepared
        // declaration from host-owned cache state instead of stranding the
        // lookup on the barrel file.
        if root_identity.canonical_id.is_empty() {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=empty_canonical hit=false store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        let (final_canonical_id, final_symbol_name) = self.host.resolve_imported_type_root_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        );
        if final_canonical_id == root_identity.canonical_id
            && final_symbol_name == root_identity.symbol_name
        {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=root_resolve_same hit=false store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        let resolved = self.host.prepared_type_decl_in_view(
            &final_canonical_id,
            &final_symbol_name,
            self.store_view,
        );
        component_meta_trace_event!(
            "solver_resolve_prepared_type_decl_result",
            format!(
                "root={}::{} source=root_resolve target={}::{} hit={} store_view={}",
                root_identity.canonical_id,
                root_identity.symbol_name,
                final_canonical_id,
                final_symbol_name,
                resolved.is_some(),
                self.store_view.is_some()
            ),
        );
        resolved
    }

    fn resolve_prepared_value_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>> {
        // Resolve from the host-owned prepared decl cache.
        if let Some(prepared) = self.host.prepared_value_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        ) {
            return Some(prepared);
        }

        if root_identity.canonical_id.is_empty() {
            return None;
        }

        let target = self.host.resolve_value_export_target_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        )?;
        let final_canonical_id = target.canonical_id;
        let final_symbol_name = target.name;
        if final_canonical_id == root_identity.canonical_id
            && final_symbol_name == root_identity.symbol_name
        {
            return None;
        }

        self.host.prepared_value_decl_in_view(
            &final_canonical_id,
            &final_symbol_name,
            self.store_view,
        )
    }

    fn utility_source(&self, name: &str) -> UtilitySource {
        if let Some(scope_payload) = self.scope_payload.as_ref() {
            if scope_payload.scope_type_names.contains(name)
                || scope_payload.scope_type_bindings.contains_key(name)
            {
                return UtilitySource::Shadowed;
            }
        }
        // Phase 2.1: dispatch intrinsics through the project-global
        // `IntrinsicRegistry`. A `Found` hit means the SDK declares the
        // name as `= intrinsic` and the registry owns its implementation
        // — these are not shadowable and must always classify as Builtin.
        // Everything else falls back to the solver's utility-name table.
        if let crate::intrinsic_registry::IntrinsicLookup::Found(_) = self
            .host
            .project_type_store()
            .intrinsic_registry()
            .lookup(name)
        {
            return UtilitySource::Builtin;
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn bare_ref_origin(&self, name: &str) -> BareRefOrigin {
        if let Some(scope_payload) = self.scope_payload.as_ref() {
            if scope_payload.import_bindings.contains_key(name) {
                return BareRefOrigin::Imported;
            }
            if scope_payload.scope_type_bindings.contains_key(name)
                || scope_payload.scope_type_names.contains(name)
                || scope_payload.scope_value_names.contains(name)
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }

    fn root_identity(&self, canonical_id: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
        if let (Some(scope_canonical_id), Some(scope_payload)) = (
            self.scope_canonical_id.as_deref(),
            self.scope_payload.as_ref(),
        ) {
            if scope_payload.scope_type_bindings.contains_key(symbol_name)
                || scope_payload.scope_type_names.contains(symbol_name)
                || scope_payload.scope_value_names.contains(symbol_name)
            {
                let resolved = ResolvedRootIdentity::new(scope_canonical_id, symbol_name);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=scope result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
        }

        // 2. If canonical_id is provided and non-empty, resolve within that
        // file's cached shallow/prepared scope before giving up.
        if !canonical_id.is_empty() {
            if let Some(entry) = self.cached_module_facts(canonical_id) {
                if self.symbol_exists_in_facts(&entry, symbol_name) {
                    let resolved = ResolvedRootIdentity::new(canonical_id, symbol_name);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=explicit_cached_scope result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
            }

            if self
                .cached_module_facts(canonical_id)
                .map(|entry| entry.shallow_state.clone())
                .is_some_and(|state| {
                    matches!(
                        state.export_target(symbol_name),
                        Some(crate::resolver_core::ExportTarget::Local { .. })
                    )
                })
            {
                let resolved = ResolvedRootIdentity::new(canonical_id, symbol_name);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_cached_export_scope result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            if let Some(resolved) =
                self.resolve_import_binding_from_facts(canonical_id, symbol_name)
            {
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_import_binding result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            if let Some(resolved) =
                self.resolve_namespace_member_from_facts(canonical_id, symbol_name)
            {
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_namespace_binding result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            if let Some((resolved_canonical, resolved_symbol)) =
                self.host.resolve_named_type_export_target_in_view(
                    canonical_id,
                    symbol_name,
                    self.store_view,
                )
            {
                let resolved = ResolvedRootIdentity::new(&resolved_canonical, &resolved_symbol);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_export_route result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            component_meta_trace_event!(
                "solver_root_identity_result",
                format!(
                    "requested_canonical={} requested_symbol={} source=explicit_canonical hit=false store_view={}",
                    canonical_id,
                    symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        // 3. Check import bindings: local name -> (canonical_id, exported_name).
        // This is the targeted resolution path for the owner file's direct imports.
        // It handles renamed and default imports where the local name differs
        // from the exported name.
        if let Some(scope_payload) = self.scope_payload.as_ref() {
            if let Some(binding) = scope_payload.import_bindings.get(symbol_name) {
                let resolved = self.resolve_imported_type_root_identity(
                    &binding.canonical_id,
                    &binding.exported_name,
                );
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=import_binding result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
        }

        // 4. Handle namespace-qualified names: `Ns.Member` -> split on first dot,
        // resolve prefix as namespace import, look up member in the target file.
        if let Some(dot_pos) = symbol_name.find('.') {
            let prefix = &symbol_name[..dot_pos];
            let member = &symbol_name[dot_pos + 1..];
            if let Some(binding) = self
                .scope_payload
                .as_ref()
                .and_then(|scope_payload| scope_payload.import_bindings.get(prefix))
            {
                if self
                    .host
                    .prepared_type_decl_in_view(&binding.canonical_id, member, self.store_view)
                    .is_some()
                {
                    let resolved = ResolvedRootIdentity::new(&binding.canonical_id, member);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_prepared_type result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if self
                    .host
                    .prepared_value_decl_in_view(&binding.canonical_id, member, self.store_view)
                    .is_some()
                {
                    let resolved = ResolvedRootIdentity::new(&binding.canonical_id, member);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_prepared_value result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if let Some((canonical_id, exported_name)) =
                    self.host.resolve_named_type_export_target_in_view(
                        &binding.canonical_id,
                        member,
                        self.store_view,
                    )
                {
                    let resolved = ResolvedRootIdentity::new(&canonical_id, &exported_name);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_named_export result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if let Some(target) = self.host.resolve_value_export_target_in_view(
                    &binding.canonical_id,
                    member,
                    self.store_view,
                ) {
                    let resolved = ResolvedRootIdentity::new(&target.canonical_id, &target.name);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_value_export result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
            }
        }

        // Unresolved bare-name: the solver encountered a reference that is not
        // in the owner env, not at a known canonical_id, and not in the import
        // bindings. This is expected for transitive same-file deps inside
        // imported prepared decl bodies -- the solver does not yet propagate
        // the defining file's canonical_id through resolution context.
        component_meta_trace_event!(
            "solver_root_identity_result",
            format!(
                "requested_canonical={} requested_symbol={} source=unresolved_bare_name hit=false store_view={}",
                canonical_id,
                symbol_name,
                self.store_view.is_some()
            ),
        );
        None
    }

    fn request_status(&self) -> RequestStatus {
        RequestStatus::Running
    }
}

#[cfg(test)]
#[path = "solver_host_tests.rs"]
mod solver_host_tests;
