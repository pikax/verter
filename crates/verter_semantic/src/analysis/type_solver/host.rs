//! Shared identity/utility classification types used by the host ↔ dispatch
//! seam.
//!
//! These types survive the D-Cutover solver kernel retirement because they
//! describe resolved declaration identities and name-classification facts that
//! `verter_session` publishes to dispatch and `component_meta_query_engine`.
//! The `TypeSolverHost` trait itself, the `EvalEnvSolverHost`, and the
//! `NoopSolverHost` test double have all been retired along with the arena
//! solver kernel.

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

/// Whether a bare type name in the current scope is local or imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BareRefOrigin {
    Local,
    Imported,
    Unknown,
}

// ---------------------------------------------------------------------------
// Request status
// ---------------------------------------------------------------------------

/// Operational status of the current resolver request, queried through the
/// host so callers can detect cancellation without coupling to runtime
/// details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Running,
    Cancelled,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_identity_display() {
        let id = ResolvedRootIdentity::new("/src/types.ts", "MyProps");
        assert_eq!(format!("{}", id), "/src/types.ts::MyProps");
    }
}
