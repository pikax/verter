//! Shared identity/utility classification types used by the host ↔ dispatch
//! seam.
//!
//! Describes resolved declaration identities and name-classification facts
//! that `verter_session` publishes to dispatch and
//! `component_meta_query_engine`.

// ---------------------------------------------------------------------------
// Root identity
// ---------------------------------------------------------------------------

/// Canonical identity for a resolved declaration root.
///
/// `canonical_id` always names the defining file, never a barrel hop.
/// This is the cache key for prepared declarations.
///
/// Fields are shared `Arc<str>` allocations so the session layer's
/// intern pool can hand every identity for the same `(path, name)` one
/// allocation instead of a fresh `String` pair per mint. Equality and
/// hashing remain CONTENT-based (`Arc<str>` derives delegate to `str`) —
/// allocation sharing is never an identity semantic, and pointer
/// identity never enters a cache key.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
)]
pub struct ResolvedRootIdentity {
    pub canonical_id: std::sync::Arc<str>,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub symbol_name: std::sync::Arc<str>,
}

impl ResolvedRootIdentity {
    pub fn new(
        canonical_id: impl Into<std::sync::Arc<str>>,
        symbol_name: impl Into<std::sync::Arc<str>>,
    ) -> Self {
        Self::new_in_owner(
            canonical_id,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
    }

    pub fn new_in_owner(
        canonical_id: impl Into<std::sync::Arc<str>>,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: impl Into<std::sync::Arc<str>>,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            owner,
            symbol_name: symbol_name.into(),
        }
    }
}

impl std::fmt::Display for ResolvedRootIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
            write!(f, "{}::{}", self.canonical_id, self.symbol_name)
        } else {
            write!(
                f,
                "{}::{:?}({})::{}",
                self.canonical_id,
                self.owner.kind(),
                self.owner.ordinal(),
                self.symbol_name
            )
        }
    }
}

#[cfg(test)]
mod owner_identity_tests {
    use super::ResolvedRootIdentity;
    use std::collections::HashMap;
    use verter_type_expr::TopLevelOwnerId;

    #[test]
    fn resolved_root_identity_discriminates_owner_in_hash_map_and_serde() {
        let module = ResolvedRootIdentity::new_in_owner(
            "/src/App.vue",
            TopLevelOwnerId::module(0),
            "Shared",
        );
        let instance = ResolvedRootIdentity::new_in_owner(
            "/src/App.vue",
            TopLevelOwnerId::instance(0),
            "Shared",
        );
        assert_ne!(module, instance);
        let mut memo = HashMap::new();
        memo.insert(module.clone(), "module");
        memo.insert(instance.clone(), "instance");
        assert_eq!(memo.len(), 2);
        assert_ne!(
            serde_json::to_string(&module).unwrap(),
            serde_json::to_string(&instance).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<ResolvedRootIdentity>(
                &serde_json::to_string(&instance).unwrap()
            )
            .unwrap(),
            instance
        );
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

    #[test]
    fn root_identity_shares_provided_arc_allocations() {
        // Interned identity flow: a caller holding `Arc<str>` identity parts
        // (the session intern pool) must be able to construct the identity
        // WITHOUT copying — the fields hold the same allocations.
        let canonical: std::sync::Arc<str> = std::sync::Arc::from("/src/types.ts");
        let symbol: std::sync::Arc<str> = std::sync::Arc::from("MyProps");
        let id = ResolvedRootIdentity::new(
            std::sync::Arc::clone(&canonical),
            std::sync::Arc::clone(&symbol),
        );
        assert!(std::sync::Arc::ptr_eq(&id.canonical_id, &canonical));
        assert!(std::sync::Arc::ptr_eq(&id.symbol_name, &symbol));
    }

    #[test]
    fn root_identity_equality_and_hash_stay_content_based() {
        use std::hash::{BuildHasher, RandomState};
        // Identities built from DIFFERENT allocation sources (borrowed str vs
        // shared Arc) are equal and hash identically: dedup is an allocation
        // concern only, never an identity semantic.
        let from_str = ResolvedRootIdentity::new("/src/types.ts", "MyProps");
        let arc: std::sync::Arc<str> = std::sync::Arc::from("/src/types.ts");
        let from_arc = ResolvedRootIdentity::new(arc, "MyProps");
        assert_eq!(from_str, from_arc);
        let state = RandomState::new();
        let hash_of = |id: &ResolvedRootIdentity| state.hash_one(id);
        assert_eq!(hash_of(&from_str), hash_of(&from_arc));
        // And distinct content must NOT collapse.
        let other = ResolvedRootIdentity::new("/src/types.ts", "OtherProps");
        assert_ne!(from_str, other);
    }
}
