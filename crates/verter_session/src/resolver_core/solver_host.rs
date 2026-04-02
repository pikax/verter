//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! `ImportedDependencyCacheEntry` caches AND owner-local type symbols.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    RequestStatus, ResolvedRootIdentity, SolverProjection, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_semantic::analysis::types::AnalyzedImport;

use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Import binding: maps a local import name to its resolved target.
#[derive(Debug, Clone)]
struct ImportBinding {
    canonical_id: String,
    exported_name: String,
}

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Owner-local `EvalEnv` type symbols (same-file declarations)
/// 2. Import bindings (local name → canonical_id + exported name)
/// 3. Host's `ImportedDependencyCacheEntry` prepared decl caches (cross-file)
pub struct SessionSolverHost<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Owner-local type environment. Types declared in the same file as the
    /// macro are resolved from here first.
    owner_env: Option<&'a EvalEnv>,
    /// Import bindings: local name → (canonical_id, exported_name).
    /// Built from the owner file's `AnalyzedImport` entries.
    import_bindings: FxHashMap<String, ImportBinding>,
}

impl<'a> SessionSolverHost<'a> {
    pub fn new(host: &'a VerterHost, store_view: Option<&'a HostStoreView>) -> Self {
        Self {
            host,
            store_view,
            owner_env: None,
            import_bindings: FxHashMap::default(),
        }
    }

    /// Create a solver host with access to the owner's local type environment.
    pub fn with_owner_env(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_env: &'a EvalEnv,
    ) -> Self {
        Self {
            host,
            store_view,
            owner_env: Some(owner_env),
            import_bindings: FxHashMap::default(),
        }
    }

    /// Create a solver host with owner env AND import bindings from analyzed imports.
    /// The import bindings allow bare-name resolution of imported symbols
    /// (e.g., `import { Foo } from './types'` → `Foo` resolves to the canonical
    /// ID of `./types` with exported name `Foo`).
    pub fn with_owner_env_and_imports(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_env: &'a EvalEnv,
        imports: &[AnalyzedImport],
    ) -> Self {
        let mut import_bindings = FxHashMap::default();
        for import in imports {
            let Some(ref canonical_id) = import.resolved_canonical_id else {
                continue;
            };
            for binding in &import.bindings {
                let exported_name = binding
                    .imported_name
                    .as_deref()
                    .unwrap_or("default")
                    .to_string();
                import_bindings.insert(
                    binding.name.clone(),
                    ImportBinding {
                        canonical_id: canonical_id.clone(),
                        exported_name,
                    },
                );
            }
        }
        Self {
            host,
            store_view,
            owner_env: Some(owner_env),
            import_bindings,
        }
    }
}

impl TypeSolverHost for SessionSolverHost<'_> {
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        // 1. Check owner-local env first
        if let Some(env) = self.owner_env {
            if let Some(decl) = env.type_symbols.get(&root_identity.symbol_name) {
                let mut prepared =
                    PreparedTypeDecl::new(root_identity.clone(), decl.kind, decl.body.clone());
                prepared.type_parameters = decl.type_parameters.clone();
                prepared.build_member_index();
                return Some(Arc::new(prepared));
            }
        }

        // 2. Fall back to host's prepared decl cache (cross-file)
        self.host.prepared_type_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        )
    }

    fn resolve_prepared_value_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>> {
        // 1. Check owner-local env first (same-file value declarations)
        if let Some(env) = self.owner_env {
            if let Some(val) = env.value_symbols.get(&root_identity.symbol_name) {
                let prepared = PreparedValueDecl {
                    root_identity: root_identity.clone(),
                    exported_name: None,
                    kind: val.kind,
                    type_annotation: val.type_annotation.clone(),
                    function_signature: val.function_signature.clone(),
                    object_shape: val.object_shape.clone(),
                    member_index: Default::default(),
                    enum_members: None,
                    external_deps: Vec::new(),
                };
                return Some(Arc::new(prepared));
            }
        }

        // 2. Fall back to host's prepared decl cache (cross-file)
        self.host.prepared_value_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
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
        // Check if owner-local env shadows the utility name
        if let Some(env) = self.owner_env {
            if env.type_symbols.contains_key(name) {
                return UtilitySource::Shadowed;
            }
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn root_identity(&self, canonical_id: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
        // 1. Check owner-local env (types and values)
        if let Some(env) = self.owner_env {
            if env.type_symbols.contains_key(symbol_name)
                || env.value_symbols.contains_key(symbol_name)
            {
                return Some(ResolvedRootIdentity::new("$owner", symbol_name));
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

        // Unresolved bare-name: the solver encountered a reference that is not
        // in the owner env, not at a known canonical_id, and not in the import
        // bindings. This is expected for transitive same-file deps inside
        // imported prepared decl bodies — the solver does not yet propagate
        // the defining file's canonical_id through resolution context.
        #[cfg(debug_assertions)]
        eprintln!(
            "[solver_host] unresolved bare name: symbol={symbol_name:?} canonical_id={canonical_id:?} (no import binding, no owner-local match)"
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
    use verter_semantic::analysis::type_solver::host::NoopSolverHost;

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
    fn session_host_with_owner_env_resolves_local_types() {
        let host = VerterHost::new_standalone(Default::default());
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(
            "interface Props { x: string }",
        );
        let solver_host = SessionSolverHost::with_owner_env(&host, None, &env);

        let id = ResolvedRootIdentity::new("$owner", "Props");
        let decl = solver_host.resolve_prepared_type_decl(&id);
        assert!(decl.is_some(), "should resolve owner-local type");
    }
}
