//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! `ImportedDependencyCacheEntry` caches AND owner-local type symbols.

use std::sync::Arc;

use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    RequestStatus, ResolvedRootIdentity, SolverProjection, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Owner-local `EvalEnv` type symbols (same-file declarations)
/// 2. Host's `ImportedDependencyCacheEntry` prepared decl caches (cross-file)
pub struct SessionSolverHost<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Owner-local type environment. Types declared in the same file as the
    /// macro are resolved from here first.
    owner_env: Option<&'a EvalEnv>,
}

impl<'a> SessionSolverHost<'a> {
    pub fn new(host: &'a VerterHost, store_view: Option<&'a HostStoreView>) -> Self {
        Self {
            host,
            store_view,
            owner_env: None,
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
        // Check owner-local env
        if let Some(env) = self.owner_env {
            if env.type_symbols.contains_key(symbol_name) {
                return Some(ResolvedRootIdentity::new("$owner", symbol_name));
            }
        }
        // Check host cache
        self.host
            .prepared_type_decl_in_view(canonical_id, symbol_name, self.store_view)?;
        Some(ResolvedRootIdentity::new(canonical_id, symbol_name))
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
