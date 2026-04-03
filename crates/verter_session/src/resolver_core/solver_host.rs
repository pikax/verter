//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! `ImportedDependencyCacheEntry` caches.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::TypeDeclKind;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    RequestStatus, ResolvedRootIdentity, SolverProjection, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Import binding: maps a local import name to its resolved target.
#[derive(Debug, Clone)]
struct ImportBinding {
    canonical_id: String,
    exported_name: String,
}

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Declaration-scoped same-file prepared declarations
/// 2. Import bindings (local name → canonical_id + exported name)
/// 3. Host's `ImportedDependencyCacheEntry` prepared decl caches (cross-file)
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
    /// Built from the owner file's `AnalyzedImport` entries.
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

    /// Create a solver host scoped to one declaration file's cached shallow
    /// state.
    ///
    /// Reads same-file symbol names and import targets from the host-owned
    /// `ShallowFileState` for the declaration file. This keeps declaration-
    /// scoped solving on the prepared/cache-backed path instead of rebuilding
    /// any owner-local eval state.
    pub fn with_declaration_scope(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        declaration_canonical_id: &str,
    ) -> Self {
        let mut import_bindings = FxHashMap::default();
        let mut scope_type_names = FxHashSet::default();
        let mut scope_value_names = FxHashSet::default();
        let mut scope_type_bindings = FxHashMap::default();
        let dependency_resolutions = host
            .dependency_resolutions_for_eval_in_view(declaration_canonical_id, store_view)
            .unwrap_or_default();

        if let Some(state) = host.shallow_file_state_in_view(declaration_canonical_id, store_view) {
            scope_type_names.extend(state.symbols.keys().cloned());
            scope_value_names.extend(state.value_symbols.keys().cloned());
            for (local_name, (source_specifier, imported_name)) in state.import_targets.iter() {
                let resolved_id = dependency_resolutions
                    .get(source_specifier)
                    .and_then(|resolution| {
                        resolution
                            .effective_target()
                            .map(str::to_string)
                            .or_else(|| resolution.resolved_canonical_id.clone())
                    })
                    .or_else(|| {
                        host.resolve_type_dependency_canonical_shallow_in_view(
                            declaration_canonical_id,
                            source_specifier,
                            store_view,
                        )
                    });
                if let Some(resolved_id) = resolved_id {
                    import_bindings.insert(
                        local_name.clone(),
                        ImportBinding {
                            canonical_id: resolved_id,
                            exported_name: imported_name.clone(),
                        },
                    );
                }
            }

            if let Some((raw_source, cached_parse, _)) =
                host.current_eval_state_in_view(declaration_canonical_id, store_view)
            {
                for param in VerterHost::sfc_script_setup_type_params(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ) {
                    let mut prepared = PreparedTypeDecl::new(
                        ResolvedRootIdentity::new(declaration_canonical_id, &param.name),
                        TypeDeclKind::Alias,
                        TypeExpr::type_parameter(param.clone()),
                    );
                    for local_name in state.symbols.keys() {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(declaration_canonical_id, local_name),
                        );
                    }
                    for local_name in state.value_symbols.keys() {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(declaration_canonical_id, local_name),
                        );
                    }
                    for (local_name, binding) in &import_bindings {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(
                                &binding.canonical_id,
                                &binding.exported_name,
                            ),
                        );
                    }
                    scope_type_names.insert(param.name.clone());
                    scope_type_bindings.insert(param.name.clone(), Arc::new(prepared));
                }
            }
        }
        Self {
            host,
            store_view,
            scope_canonical_id: Some(declaration_canonical_id.to_string()),
            scope_type_names,
            scope_value_names,
            scope_type_bindings,
            import_bindings,
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
            return Some(prepared);
        }

        // Declaration-scoped name resolution and import bindings may point at
        // a shallow import target that is itself a barrel. Follow the cached
        // export route once here so the solver still reads the final prepared
        // declaration from host-owned cache state instead of stranding the
        // lookup on the barrel file.
        if root_identity.canonical_id.is_empty() {
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
            return None;
        }

        self.host.prepared_type_decl_in_view(
            &final_canonical_id,
            &final_symbol_name,
            self.store_view,
        )
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

        let Some(target) = self.host.resolve_value_export_target_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        ) else {
            return None;
        };
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

    fn resolve_member_projection(
        &self,
        root_identity: &ResolvedRootIdentity,
        member: &str,
    ) -> Option<SolverProjection<TypeExpr>> {
        let prepared = self.resolve_prepared_type_decl(root_identity)?;
        let m = prepared.member(member)?;
        Some(SolverProjection::exact_concrete(m.ty.clone()))
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
                return Some(ResolvedRootIdentity::new(scope_canonical_id, symbol_name));
            }
        }

        // 2. If canonical_id is provided and non-empty, use it directly
        if !canonical_id.is_empty() {
            if self
                .host
                .prepared_type_decl_in_view(canonical_id, symbol_name, self.store_view)
                .is_some()
            {
                return Some(ResolvedRootIdentity::new(canonical_id, symbol_name));
            }
            if self
                .host
                .prepared_value_decl_in_view(canonical_id, symbol_name, self.store_view)
                .is_some()
            {
                return Some(ResolvedRootIdentity::new(canonical_id, symbol_name));
            }
            return None;
        }

        // 3. Check import bindings: local name → (canonical_id, exported_name).
        // This is the targeted resolution path for the owner file's direct imports.
        // It handles renamed and default imports where the local name differs
        // from the exported name.
        if let Some(binding) = self.import_bindings.get(symbol_name) {
            return Some(ResolvedRootIdentity::new(
                &binding.canonical_id,
                &binding.exported_name,
            ));
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
                    return Some(ResolvedRootIdentity::new(&binding.canonical_id, member));
                }
                if self
                    .host
                    .prepared_value_decl_in_view(&binding.canonical_id, member, self.store_view)
                    .is_some()
                {
                    return Some(ResolvedRootIdentity::new(&binding.canonical_id, member));
                }
                if let Some((canonical_id, exported_name)) =
                    self.host.resolve_named_type_export_target_in_view(
                        &binding.canonical_id,
                        member,
                        self.store_view,
                    )
                {
                    return Some(ResolvedRootIdentity::new(&canonical_id, &exported_name));
                }
                if let Some(target) = self.host.resolve_value_export_target_in_view(
                    &binding.canonical_id,
                    member,
                    self.store_view,
                ) {
                    return Some(ResolvedRootIdentity::new(
                        &target.canonical_id,
                        &target.name,
                    ));
                }
            }
        }

        // Unresolved bare-name: the solver encountered a reference that is not
        // in the owner env, not at a known canonical_id, and not in the import
        // bindings. This is expected for transitive same-file deps inside
        // imported prepared decl bodies — the solver does not yet propagate
        // the defining file's canonical_id through resolution context.
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
        let dep_edges = FxHashMap::from_iter([("./dep".to_string(), "/dep.ts".to_string())]);
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );

        host.imported_dependency_cache.lock().insert(
            "/decl.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/decl.ts".into(),
                raw_source: Arc::<str>::from(source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(analysis),
                shallow_file_state: Some(state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./dep".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./dep".to_string(),
                        resolved_canonical_id: Some("/dep.ts".to_string()),
                        possible_canonical_ids: vec!["/dep.ts".to_string()],
                    },
                )]),
            }),
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
        let dep_edges = FxHashMap::from_iter([("./theme".to_string(), "/theme.ts".to_string())]);
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );
        let prepared_value_decls = crate::resolver_core::build_prepared_value_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );

        host.imported_dependency_cache.lock().insert(
            "/decl.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/decl.ts".into(),
                raw_source: Arc::<str>::from(source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(analysis),
                shallow_file_state: Some(state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls,
                dependency_resolutions: FxHashMap::from_iter([(
                    "./theme".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./theme".to_string(),
                        resolved_canonical_id: Some("/theme.ts".to_string()),
                        possible_canonical_ids: vec!["/theme.ts".to_string()],
                    },
                )]),
            }),
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

        host.imported_dependency_cache.lock().insert(
            "/types/index.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/index.ts".into(),
                raw_source: Arc::<str>::from(barrel_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(barrel_analysis),
                shallow_file_state: Some(barrel_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(barrel_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./props".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./props".to_string(),
                        resolved_canonical_id: Some("/types/props.ts".to_string()),
                        possible_canonical_ids: vec!["/types/props.ts".to_string()],
                    },
                )]),
            }),
        );

        let props_source = "export interface Props { label: string }";
        let props_analysis = Arc::new(analyze_external_type_source(props_source, &allocator));
        let props_env = parse_and_build_env(props_source);
        let props_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&props_analysis),
            Some(&props_env),
        ));
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/types/props.ts",
            &props_state,
            None,
        );

        host.imported_dependency_cache.lock().insert(
            "/types/props.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/props.ts".into(),
                raw_source: Arc::<str>::from(props_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(props_analysis),
                shallow_file_state: Some(props_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(props_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::default(),
            }),
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
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let barrel_source = "export { theme } from './theme'";
        let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
        let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&barrel_analysis),
            None,
        ));

        host.imported_dependency_cache.lock().insert(
            "/theme/index.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/theme/index.ts".into(),
                raw_source: Arc::<str>::from(barrel_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(barrel_analysis),
                shallow_file_state: Some(barrel_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(barrel_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./theme".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./theme".to_string(),
                        resolved_canonical_id: Some("/theme/theme.ts".to_string()),
                        possible_canonical_ids: vec!["/theme/theme.ts".to_string()],
                    },
                )]),
            }),
        );

        let theme_source = "export const theme: { color: string } = { color: 'blue' }";
        let theme_analysis = Arc::new(analyze_external_type_source(theme_source, &allocator));
        let theme_env = parse_and_build_env(theme_source);
        let theme_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&theme_analysis),
            Some(&theme_env),
        ));
        let prepared_value_decls = crate::resolver_core::build_prepared_value_decl_cache(
            "/theme/theme.ts",
            &theme_state,
            None,
        );

        host.imported_dependency_cache.lock().insert(
            "/theme/theme.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/theme/theme.ts".into(),
                raw_source: Arc::<str>::from(theme_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(theme_analysis),
                shallow_file_state: Some(theme_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(theme_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls,
                dependency_resolutions: FxHashMap::default(),
            }),
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prepared = solver_host
            .resolve_prepared_value_decl(&ResolvedRootIdentity::new("/theme/index.ts", "theme"))
            .expect("barrel lookup should route to the defining prepared value decl");
        assert_eq!(prepared.root_identity.canonical_id, "/theme/theme.ts");
        assert_eq!(prepared.root_identity.symbol_name, "theme");
    }
}
