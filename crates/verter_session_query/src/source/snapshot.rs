//! The identity of a retained parse snapshot.

use crate::analysis::types::Hash16;
use std::sync::Arc;

/// Content-generation identity of one retained parse snapshot: the
/// canonical file, its whole-content hash, and the R21 parse-env
/// dimension the parse runs under. Content-addressed by construction —
/// an edit produces a new key, so a stale snapshot can never answer a
/// new-content demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotKey {
    pub canonical: Arc<str>,
    pub whole_hash: Hash16,
    pub parse_env_hash: Hash16,
}
