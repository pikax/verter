//! Retention observability of the semantic graph store: the sizes of the
//! resident structures this store keeps between requests, so a long editing
//! session can be measured structure by structure.
//!
//! The other resident sizes already have accessors beside their owners:
//! [`SemanticGraphStore::node_count`], [`SemanticGraphStore::memo_entry_count`]
//! and, for the signature kernel,
//! [`SignatureStore::interned_len`](crate::signature_kernel::SignatureStore::interned_len)
//! and [`SignatureStore::retained_len`](crate::signature_kernel::SignatureStore::retained_len).

use super::*;

impl SemanticGraphStore {
    /// Number of resident union member views; each is keyed by its union's
    /// node id and bounded per store.
    #[must_use]
    pub fn union_view_count(&self) -> usize {
        self.union_views.lock().len()
    }

    /// Number of `unresolved_reach` entries: the per-node structure bits
    /// the unresolved-reach walk records.
    #[must_use]
    pub fn unresolved_reach_count(&self) -> usize {
        self.unresolved_reach.lock().len()
    }
}
