//! Host seam for the native type solver.
//!
//! `TypeSolverHost` is the load-bearing boundary between `verter_session`
//! (file readiness, frontier, prepared declaration cache) and the solver
//! (query-local arena, relations, projections).
//!
//! The solver may only ask for already-resolved canonical roots or host-owned
//! prepared declarations. It must not reopen route discovery from raw import
//! specifiers or source text.
//!
//! `verter_session::resolver_core` is the intended implementer.

use std::sync::Arc;

use super::prepared::{PreparedTypeDecl, PreparedValueDecl};

// ---------------------------------------------------------------------------
// Root identity
// ---------------------------------------------------------------------------

/// Canonical identity for a resolved declaration root.
///
/// `canonical_id` always names the defining file, never a barrel hop.
/// This is the cache key for prepared declarations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedRootIdentity {
    pub canonical_id: String,
    pub symbol_name: String,
}

impl ResolvedRootIdentity {
    pub fn new(canonical_id: impl Into<String>, symbol_name: impl Into<String>) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            symbol_name: symbol_name.into(),
        }
    }
}

impl std::fmt::Display for ResolvedRootIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.canonical_id, self.symbol_name)
    }
}

// ---------------------------------------------------------------------------
// Utility source classification
// ---------------------------------------------------------------------------

/// Whether a named type reference is a built-in TS utility, a user-shadowed
/// name, or an unknown (local/imported) declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilitySource {
    /// Compiler-provided built-in (Partial, Required, Record, etc.).
    Builtin,
    /// User has shadowed the name with their own declaration.
    Shadowed,
    /// Not a recognized utility name.
    Unknown,
}

// ---------------------------------------------------------------------------
// Request status
// ---------------------------------------------------------------------------

/// Operational status of the current solver request, queried through the host
/// so the solver can detect cancellation without coupling to runtime details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Running,
    Cancelled,
}

// ---------------------------------------------------------------------------
// TypeSolverHost trait
// ---------------------------------------------------------------------------

/// Host seam for the native solver.
///
/// The solver may only ask for already-resolved canonical roots or host-owned
/// prepared declarations. It must not reopen route discovery from raw import
/// specifiers or source text.
///
/// # Contract
///
/// - `resolve_prepared_type_decl`: returns a prepared type declaration for a
///   canonical root identity. Returns `None` if the source is unavailable or
///   the symbol does not exist.
///
/// - `resolve_prepared_value_decl`: same for value declarations (needed for
///   `typeof`).
///
/// - `utility_source`: classifies whether a name is a built-in utility, a
///   user-shadowed name, or unknown.
///
/// - `root_identity`: resolves a (canonical_id, symbol_name) pair into a
///   stable declaration identity. This accounts for re-exports and barrel hops.
///
/// - `request_status`: allows the solver to poll for external cancellation.
pub trait TypeSolverHost {
    /// Look up a prepared type declaration by its canonical root identity.
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>>;

    /// Look up a prepared value declaration by its canonical root identity.
    fn resolve_prepared_value_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>>;

    /// Classify whether a type name is a built-in utility.
    fn utility_source(&self, name: &str) -> UtilitySource;

    /// Resolve a (canonical_id, symbol_name) to a stable root identity,
    /// following re-exports to the defining file.
    fn root_identity(&self, canonical_id: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
        let _ = (canonical_id, symbol_name);
        None
    }

    /// Poll for request cancellation.
    fn request_status(&self) -> RequestStatus {
        RequestStatus::Running
    }
}

// ---------------------------------------------------------------------------
// EvalEnv-backed solver host
// ---------------------------------------------------------------------------

/// Solver host that resolves type declarations from an `EvalEnv`'s
/// `type_symbols` table. Used for standalone expansion (no session/host).
///
/// Clones the type_symbols map on construction so the caller can still
/// mutate the original `EvalEnv` while the solver runs.
pub struct EvalEnvSolverHost {
    type_symbols: rustc_hash::FxHashMap<String, crate::analysis::type_eval::TypeDeclInfo>,
    value_symbols: rustc_hash::FxHashMap<String, crate::analysis::type_eval::ValueDeclInfo>,
    type_bindings:
        rustc_hash::FxHashMap<String, std::sync::Arc<crate::analysis::type_expr::TypeExpr>>,
}

impl EvalEnvSolverHost {
    pub fn new(env: &crate::analysis::type_eval::EvalEnv) -> Self {
        Self {
            type_symbols: env.type_symbols.clone(),
            value_symbols: env.value_symbols.clone(),
            type_bindings: env.type_bindings.clone(),
        }
    }
}

impl TypeSolverHost for EvalEnvSolverHost {
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        if let Some(decl) = self.type_symbols.get(&root_identity.symbol_name) {
            let mut prepared =
                PreparedTypeDecl::new(root_identity.clone(), decl.kind, decl.body.clone());
            prepared.type_parameters = decl.type_parameters.clone();
            prepared.build_member_index();
            prepared.classify_wrapper_shape();
            prepared.classify_projection();
            return Some(Arc::new(prepared));
        }

        let bound = self.type_bindings.get(&root_identity.symbol_name)?;
        let mut prepared = PreparedTypeDecl::new(
            root_identity.clone(),
            crate::analysis::type_eval::TypeDeclKind::Alias,
            bound.as_ref().clone(),
        );
        prepared.build_member_index();
        prepared.classify_wrapper_shape();
        prepared.classify_projection();
        Some(Arc::new(prepared))
    }

    fn resolve_prepared_value_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>> {
        let value = self.value_symbols.get(&root_identity.symbol_name)?;
        Some(Arc::new(PreparedValueDecl {
            root_identity: root_identity.clone(),
            exported_name: None,
            kind: value.kind,
            type_annotation: value.type_annotation.clone(),
            function_signature: value.function_signature.clone(),
            object_shape: value.object_shape.clone(),
            member_index: Default::default(),
            enum_members: None,
            external_deps: Vec::new(),
            name_resolution: Default::default(),
            cache_deps: Default::default(),
        }))
    }

    fn utility_source(&self, name: &str) -> UtilitySource {
        if self.type_symbols.contains_key(name) || self.type_bindings.contains_key(name) {
            UtilitySource::Shadowed
        } else if super::builtin::BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn root_identity(
        &self,
        _canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        if self.type_symbols.contains_key(symbol_name)
            || self.type_bindings.contains_key(symbol_name)
            || self.value_symbols.contains_key(symbol_name)
        {
            Some(ResolvedRootIdentity::new("$local", symbol_name))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Noop implementation for tests
// ---------------------------------------------------------------------------

/// No-op host that resolves nothing. Useful for unit testing solver internals.
pub struct NoopSolverHost;

impl TypeSolverHost for NoopSolverHost {
    fn resolve_prepared_type_decl(
        &self,
        _root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        None
    }

    fn resolve_prepared_value_decl(
        &self,
        _root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>> {
        None
    }

    fn utility_source(&self, _name: &str) -> UtilitySource {
        UtilitySource::Unknown
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_host_resolves_nothing() {
        let host = NoopSolverHost;
        let id = ResolvedRootIdentity::new("/types.ts", "Props");

        assert!(host.resolve_prepared_type_decl(&id).is_none());
        assert!(host.resolve_prepared_value_decl(&id).is_none());
        assert_eq!(host.utility_source("Partial"), UtilitySource::Unknown);
        assert!(host.root_identity("/types.ts", "Props").is_none());
        assert_eq!(host.request_status(), RequestStatus::Running);
    }

    #[test]
    fn root_identity_display() {
        let id = ResolvedRootIdentity::new("/src/types.ts", "MyProps");
        assert_eq!(format!("{}", id), "/src/types.ts::MyProps");
    }

    #[test]
    fn eval_env_host_resolves_value_declarations_for_typeof() {
        let mut env = crate::analysis::type_eval::EvalEnv::new();
        env.add_value(crate::analysis::type_eval::ValueDeclInfo {
            name: "as".to_string(),
            declaration_id: 0,
            kind: crate::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(crate::analysis::type_expr::TypeExpr::string_literal(
                "input",
            )),
            function_signature: None,
            object_shape: None,
        });

        let host = EvalEnvSolverHost::new(&env);
        let identity = host
            .root_identity("", "as")
            .expect("value bindings should produce root identities");
        let prepared = host
            .resolve_prepared_value_decl(&identity)
            .expect("value bindings should resolve through the eval host");

        assert_eq!(
            prepared.type_annotation,
            Some(crate::analysis::type_expr::TypeExpr::string_literal(
                "input"
            ))
        );
    }
}
