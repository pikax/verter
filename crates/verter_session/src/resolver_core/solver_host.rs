//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! ModuleFactsDb caches.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    RequestStatus, ResolvedRootIdentity, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::host_manage::component_meta_trace_event;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

use super::prepared_decl::ImportBinding;

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Declaration-scoped same-file prepared declarations (via `PreparedDeclBundle`)
/// 2. Import bindings (local name → canonical_id + exported name)
/// 3. Host's ModuleFactsDb prepared decl caches (cross-file)
pub struct SessionSolverHost<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Canonical file scope for declaration-scoped solving.
    scope_canonical_id: Option<String>,
    /// Same-file type names visible in the active declaration scope.
    scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in the active declaration scope.
    scope_value_names: FxHashSet<String>,
    /// Script-setup generic bindings visible in the active declaration scope.
    scope_type_bindings: FxHashMap<String, Arc<PreparedTypeDecl>>,
    /// Import bindings: local name → (canonical_id, exported_name).
    /// Read from the host-owned `PreparedDeclBundle`.
    import_bindings: FxHashMap<String, ImportBinding>,
}

impl<'a> SessionSolverHost<'a> {
    pub fn new(host: &'a VerterHost, store_view: Option<&'a HostStoreView>) -> Self {
        Self {
            host,
            store_view,
            scope_canonical_id: None,
            scope_type_names: FxHashSet::default(),
            scope_value_names: FxHashSet::default(),
            scope_type_bindings: FxHashMap::default(),
            import_bindings: FxHashMap::default(),
        }
    }

    /// Create a solver host scoped to one declaration file's cached
    /// `PreparedDeclBundle`.
    ///
    /// All declaration-scope data (symbol names, import bindings, script-setup
    /// generics) is read from the host-owned bundle — no inline reconstruction
    /// or `ensure_module_facts_in_view` probing. This keeps the solver hot path
    /// on a single cached read per declaration scope.
    pub fn with_declaration_scope(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        declaration_canonical_id: &str,
    ) -> Self {
        if let Some(bundle) = host
            .prepared_decl_bundle_in_view(declaration_canonical_id, store_view)
            .or_else(|| {
                Self::prepared_decl_bundle_from_cached_facts(
                    host,
                    store_view,
                    declaration_canonical_id,
                )
            })
        {
            // Include script-setup generic param names in type_names so the
            // solver recognises them as in-scope.
            let mut scope_type_names = bundle.scope_type_names.clone();
            for param_name in bundle.script_setup_type_bindings.keys() {
                scope_type_names.insert(param_name.clone());
            }

            Self {
                host,
                store_view,
                scope_canonical_id: Some(declaration_canonical_id.to_string()),
                scope_type_names,
                scope_value_names: bundle.scope_value_names.clone(),
                scope_type_bindings: bundle.script_setup_type_bindings.clone(),
                import_bindings: bundle.import_bindings.clone(),
            }
        } else {
            Self::new(host, store_view)
        }
    }

    fn prepared_decl_bundle_from_cached_facts(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let facts = host.ensure_module_facts_in_view(canonical_id, store_view)?;
        let state = facts.shallow_state.as_ref();
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.import_targets.is_empty()
            && state.exports.is_empty()
            && state.wildcard_reexports.is_empty()
        {
            return None;
        }

        let mut dep_edges = FxHashMap::default();
        for target in state.import_targets.values() {
            if !target.canonical_id.is_empty() {
                dep_edges
                    .entry(target.source_specifier.clone())
                    .or_insert_with(|| target.canonical_id.clone());
            }
        }
        for export in state.exports.values() {
            if let crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                canonical_id,
                ..
            } = export
            {
                if !canonical_id.is_empty() {
                    dep_edges
                        .entry(source_specifier.clone())
                        .or_insert_with(|| canonical_id.clone());
                }
            }
        }
        for wildcard in &state.wildcard_reexports {
            if !wildcard.canonical_id.is_empty() {
                dep_edges
                    .entry(wildcard.source_specifier.clone())
                    .or_insert_with(|| wildcard.canonical_id.clone());
            }
        }

        Some(Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                state,
                dep_edges,
                FxHashMap::default(),
            ),
        ))
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
                return Some(ResolvedRootIdentity::new(
                    resolved_id,
                    &target.imported_name,
                ));
            }
        }

        let bundle =
            Self::prepared_decl_bundle_from_cached_facts(self.host, self.store_view, canonical_id)
                .or_else(|| {
                    self.host
                        .prepared_decl_bundle_in_view(canonical_id, self.store_view)
                })?;
        let binding = bundle.import_bindings.get(local_name)?;
        Some(ResolvedRootIdentity::new(
            &binding.canonical_id,
            &binding.exported_name,
        ))
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
            _ => None,
        }
    }
}

impl TypeSolverHost for SessionSolverHost<'_> {
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        if let Some(scope_canonical_id) = self.scope_canonical_id.as_deref() {
            if root_identity.canonical_id == scope_canonical_id {
                if let Some(bound) = self.scope_type_bindings.get(&root_identity.symbol_name) {
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

        if let Some(bundle) = Self::prepared_decl_bundle_from_cached_facts(
            self.host,
            self.store_view,
            &root_identity.canonical_id,
        ) {
            if let Some(prepared) = bundle.prepared_type_decls.get(&root_identity.symbol_name) {
                component_meta_trace_event!(
                    "solver_resolve_prepared_type_decl_result",
                    format!(
                        "root={}::{} source=rebuilt_cached_scope_bundle hit=true store_view={}",
                        root_identity.canonical_id,
                        root_identity.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(Arc::clone(prepared));
            }
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
        if self.scope_type_names.contains(name) || self.scope_type_bindings.contains_key(name) {
            return UtilitySource::Shadowed;
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn root_identity(&self, canonical_id: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
        if let Some(scope_canonical_id) = self.scope_canonical_id.as_deref() {
            if self.scope_type_bindings.contains_key(symbol_name)
                || self.scope_type_names.contains(symbol_name)
                || self.scope_value_names.contains(symbol_name)
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

        // 3. Check import bindings: local name → (canonical_id, exported_name).
        // This is the targeted resolution path for the owner file's direct imports.
        // It handles renamed and default imports where the local name differs
        // from the exported name.
        if let Some(binding) = self.import_bindings.get(symbol_name) {
            let resolved = ResolvedRootIdentity::new(&binding.canonical_id, &binding.exported_name);
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

        // 4. Handle namespace-qualified names: `Ns.Member` → split on first dot,
        // resolve prefix as namespace import, look up member in the target file.
        if let Some(dot_pos) = symbol_name.find('.') {
            let prefix = &symbol_name[..dot_pos];
            let member = &symbol_name[dot_pos + 1..];
            if let Some(binding) = self.import_bindings.get(prefix) {
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
        // imported prepared decl bodies — the solver does not yet propagate
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
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_semantic::analysis::type_solver::host::NoopSolverHost;
    use verter_semantic::analysis::Hash16;

    #[test]
    fn noop_host_returns_none() {
        let host = NoopSolverHost;
        let id = ResolvedRootIdentity::new("/t.ts", "T");
        assert!(host.resolve_prepared_type_decl(&id).is_none());
    }

    #[test]
    fn session_host_without_env() {
        let host = VerterHost::new_standalone(Default::default());
        let solver_host = SessionSolverHost::new(&host, None);
        let id = ResolvedRootIdentity::new("/t.ts", "T");
        assert!(solver_host.resolve_prepared_type_decl(&id).is_none());
    }

    #[test]
    fn declaration_scope_prefers_cached_prepared_decl_shape() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;

        let host = VerterHost::new_standalone(Default::default());
        let source = r#"
import type { Inner } from "./dep"
export interface Props { child: Inner }
"#;
        let allocator = oxc_allocator::Allocator::new();
        let analysis = Arc::new(analyze_external_type_source(source, &allocator));
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
        let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&analysis),
            Some(&env),
        ));
        host.seed_module_facts_for_test(
            "/decl.ts",
            Hash16::default(),
            Arc::<str>::from(source),
            None,
            None,
            None,
            analysis,
            state,
            None,
            Some(Arc::<str>::from(source)),
            FxHashMap::from_iter([(
                "./dep".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./dep".to_string(),
                    resolved_canonical_id: Some("/dep.ts".to_string()),
                    possible_canonical_ids: vec!["/dep.ts".to_string()],
                },
            )]),
        );

        let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");
        let id = ResolvedRootIdentity::new("/decl.ts", "Props");
        let decl = solver_host
            .resolve_prepared_type_decl(&id)
            .expect("declaration-scoped host should use cached prepared decls");
        assert_eq!(
            decl.name_resolution
                .get("Inner")
                .map(|identity| identity.canonical_id.as_str()),
            Some("/dep.ts"),
            "declaration-scoped solving should preserve cached name-resolution instead of rebuilding a local decl from EvalEnv",
        );
    }

    #[test]
    fn declaration_scope_root_identity_resolves_same_file_symbols_and_imports() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let source = r#"
import type { Theme } from "./theme"
export interface Props { theme: Theme }
export const defaults: Props = {} as Props
"#;
        let allocator = oxc_allocator::Allocator::new();
        let analysis = Arc::new(analyze_external_type_source(source, &allocator));
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
        let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&analysis),
            Some(&env),
        ));
        host.seed_module_facts_for_test(
            "/decl.ts",
            Hash16::default(),
            Arc::<str>::from(source),
            None,
            None,
            None,
            analysis,
            state,
            None,
            Some(Arc::<str>::from(source)),
            FxHashMap::from_iter([(
                "./theme".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/theme.ts".to_string()),
                    possible_canonical_ids: vec!["/theme.ts".to_string()],
                },
            )]),
        );

        let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");

        let props = solver_host
            .root_identity("", "Props")
            .expect("same-file type should resolve in declaration scope");
        assert_eq!(props.canonical_id, "/decl.ts");

        let defaults = solver_host
            .root_identity("", "defaults")
            .expect("same-file value should resolve in declaration scope");
        assert_eq!(defaults.canonical_id, "/decl.ts");

        let theme = solver_host
            .root_identity("", "Theme")
            .expect("import binding should resolve from declaration scope");
        assert_eq!(theme.canonical_id, "/theme.ts");
        assert_eq!(theme.symbol_name, "Theme");
    }

    #[test]
    fn explicit_canonical_root_identity_resolves_import_bindings_from_shallow_state() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let helper_source = "export type Prettify<T> = { [K in keyof T]: T[K] }";
        let helper_analysis = Arc::new(analyze_external_type_source(helper_source, &allocator));
        let helper_env = parse_and_build_env(helper_source);
        let helper_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&helper_analysis),
            Some(&helper_env),
        ));
        host.seed_module_facts_for_test(
            "/helper.d.ts",
            Hash16::default(),
            Arc::<str>::from(helper_source),
            None,
            None,
            None,
            helper_analysis,
            helper_state,
            None,
            Some(Arc::<str>::from(helper_source)),
            FxHashMap::default(),
        );

        let decl_source = r#"
import { Prettify } from "./helper"
export type FancyProps = Prettify<{ open: boolean }>
"#;
        let decl_analysis = Arc::new(analyze_external_type_source(decl_source, &allocator));
        let decl_env = parse_and_build_env(decl_source);
        let decl_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&decl_analysis),
            Some(&decl_env),
        ));

        host.seed_module_facts_for_test(
            "/decl.d.ts",
            Hash16::default(),
            Arc::<str>::from(decl_source),
            None,
            None,
            None,
            decl_analysis,
            decl_state,
            None,
            Some(Arc::<str>::from(decl_source)),
            FxHashMap::from_iter([(
                "./helper".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./helper".to_string(),
                    resolved_canonical_id: Some("/helper.d.ts".to_string()),
                    possible_canonical_ids: vec!["/helper.d.ts".to_string()],
                },
            )]),
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prettify = solver_host.root_identity("/decl.d.ts", "Prettify").expect(
            "explicit canonical lookups should resolve import bindings from cached shallow state",
        );

        assert_eq!(prettify.canonical_id, "/helper.d.ts");
        assert_eq!(prettify.symbol_name, "Prettify");
    }

    #[test]
    fn explicit_canonical_root_identity_does_not_follow_uncached_import_bindings() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let helper_source = "export type Prettify<T> = { [K in keyof T]: T[K] }";
        let helper_analysis = Arc::new(analyze_external_type_source(helper_source, &allocator));
        let helper_env = parse_and_build_env(helper_source);
        let helper_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&helper_analysis),
            Some(&helper_env),
        ));
        host.seed_module_facts_for_test(
            "/helper.d.ts",
            Hash16::default(),
            Arc::<str>::from(helper_source),
            None,
            None,
            None,
            helper_analysis,
            helper_state,
            None,
            Some(Arc::<str>::from(helper_source)),
            FxHashMap::default(),
        );

        let decl_source = r#"
import { Prettify } from "./helper"
export type FancyProps = Prettify<{ open: boolean }>
"#;
        let decl_analysis = Arc::new(analyze_external_type_source(decl_source, &allocator));
        let decl_env = parse_and_build_env(decl_source);
        let decl_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&decl_analysis),
            Some(&decl_env),
        ));

        host.seed_module_facts_for_test(
            "/decl.d.ts",
            Hash16::default(),
            Arc::<str>::from(decl_source),
            None,
            None,
            None,
            decl_analysis,
            decl_state,
            None,
            Some(Arc::<str>::from(decl_source)),
            FxHashMap::default(),
        );

        let solver_host = SessionSolverHost::new(&host, None);
        assert!(
            solver_host
                .root_identity("/decl.d.ts", "Prettify")
                .is_none(),
            "canonical-scoped root lookups must stay cache-only and refuse uncached import routing",
        );
    }

    #[test]
    fn prepared_type_decl_lookup_routes_barrel_targets_before_cache_lookup() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let barrel_source = "export { Props } from './props'";
        let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
        let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&barrel_analysis),
            None,
        ));

        host.seed_module_facts_for_test(
            "/types/index.ts",
            Hash16::default(),
            Arc::<str>::from(barrel_source),
            None,
            None,
            None,
            barrel_analysis,
            barrel_state,
            None,
            Some(Arc::<str>::from(barrel_source)),
            FxHashMap::from_iter([(
                "./props".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./props".to_string(),
                    resolved_canonical_id: Some("/types/props.ts".to_string()),
                    possible_canonical_ids: vec!["/types/props.ts".to_string()],
                },
            )]),
        );

        let props_source = "export interface Props { label: string }";
        let props_analysis = Arc::new(analyze_external_type_source(props_source, &allocator));
        let props_env = parse_and_build_env(props_source);
        let props_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&props_analysis),
            Some(&props_env),
        ));
        host.seed_module_facts_for_test(
            "/types/props.ts",
            Hash16::default(),
            Arc::<str>::from(props_source),
            None,
            None,
            None,
            props_analysis,
            props_state,
            None,
            Some(Arc::<str>::from(props_source)),
            FxHashMap::default(),
        );

        let root = host.resolve_imported_type_root_in_view("/types/index.ts", "Props", None);
        assert_eq!(
            root,
            ("/types/props.ts".to_string(), "Props".to_string()),
            "barrel root resolution should route to the defining declaration target",
        );
        assert!(
            host.prepared_type_decl_in_view("/types/props.ts", "Props", None)
                .is_some(),
            "the defining prepared decl should be available directly once the root resolves",
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prepared = solver_host
            .resolve_prepared_type_decl(&ResolvedRootIdentity::new("/types/index.ts", "Props"))
            .expect("barrel lookup should route to the defining prepared type decl");
        assert_eq!(prepared.root_identity.canonical_id, "/types/props.ts");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
    }

    #[test]
    fn prepared_value_decl_lookup_routes_barrel_targets_before_cache_lookup() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/theme/index.ts".to_string(),
            Arc::from("export { theme } from './theme'"),
        );
        ws.inject_file(
            "/theme/theme.ts".to_string(),
            Arc::from("export const theme: { color: string } = { color: 'blue' }"),
        );

        let host = VerterHost::new(crate::HostConfig::default(), ws);
        host.set_import_dependencies(
            "/theme/index.ts",
            vec![crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/theme/theme.ts".to_string()),
                possible_canonical_ids: vec!["/theme/theme.ts".to_string()],
            }],
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prepared = solver_host
            .resolve_prepared_value_decl(&ResolvedRootIdentity::new("/theme/index.ts", "theme"))
            .expect("barrel lookup should route to the defining prepared value decl");
        assert_eq!(prepared.root_identity.canonical_id, "/theme/theme.ts");
        assert_eq!(prepared.root_identity.symbol_name, "theme");
    }

    #[test]
    fn member_projection_chases_generic_alias_slots_through_helper_context() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::type_expr::{
            LiteralValue, ObjectMember, PrimitiveName, TypeExpr,
        };
        use verter_semantic::analysis::type_solver::solve::solve_type_with_trace;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let config_source = r#"
export type Id<T> = {} & { [P in keyof T]: T[P] }
export type Theme = {
  slots: {
    item: string
  }
}
export type Noise = {
  boom: string
}
export type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<T['slots']>
export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}
"#;
        let config_analysis = Arc::new(analyze_external_type_source(config_source, &allocator));
        let config_env = parse_and_build_env(config_source);
        let config_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&config_analysis),
            Some(&config_env),
        ));
        host.seed_module_facts_for_test(
            "/types/config.ts",
            Hash16::default(),
            Arc::<str>::from(config_source),
            None,
            None,
            None,
            config_analysis,
            config_state,
            None,
            Some(Arc::<str>::from(config_source)),
            FxHashMap::default(),
        );

        let consumer_source = r#"
import type { ComponentConfig, Theme } from './config'
export type CheckboxGroup = ComponentConfig<Theme>
"#;
        let consumer_analysis = Arc::new(analyze_external_type_source(consumer_source, &allocator));
        let consumer_env = parse_and_build_env(consumer_source);
        let consumer_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&consumer_analysis),
            Some(&consumer_env),
        ));
        host.seed_module_facts_for_test(
            "/types/consumer.ts",
            Hash16::default(),
            Arc::<str>::from(consumer_source),
            None,
            None,
            None,
            consumer_analysis,
            consumer_state,
            None,
            Some(Arc::<str>::from(consumer_source)),
            FxHashMap::from_iter([(
                "./config".to_string(),
                crate::types::DependencyResolution {
                    specifier: "./config".to_string(),
                    resolved_canonical_id: Some("/types/config.ts".to_string()),
                    possible_canonical_ids: vec!["/types/config.ts".to_string()],
                },
            )]),
        );

        let solver_host =
            SessionSolverHost::with_declaration_scope(&host, None, "/types/consumer.ts");

        let (solved, trace) = solve_type_with_trace(
            &TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("CheckboxGroup")),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
            },
            &solver_host,
        );

        let TypeExpr::Object(slots) = solved.value else {
            panic!("expected object slots projection, got {:?}", solved.value);
        };
        let item = slots
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(prop) if prop.name == "item" => Some(prop),
                _ => None,
            })
            .expect("slots projection should contain item");
        assert!(
            !item.optional,
            "fixture keeps the projected slot member required"
        );
        assert!(matches!(
            item.ty,
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        assert!(
            !trace.iter().any(|identity| {
                identity.canonical_id == "/types/config.ts" && identity.symbol_name == "Noise"
            }),
            "solving CheckboxGroup['slots'] should stay on-route and never visit Noise"
        );
    }
}
