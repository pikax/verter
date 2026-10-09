//! The identity of a retained parse snapshot.

use crate::analysis::types::Hash16;
use std::sync::Arc;

/// Content-generation identity of one retained parse snapshot: the
/// canonical file, its whole-content hash, and the R21 parse-env
/// dimension the parse runs under. Content-addressed by construction —
/// an edit produces a new key, so a stale snapshot can never answer a
/// new-content demand.
///
/// A public value used to REQUEST or IDENTIFY a snapshot, not a
/// certificate: any crate may build one, and holding a key is not a lease
/// on, or proof of, a retained snapshot. Possession grants nothing; the
/// snapshot owner decides what a key addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotKey {
    pub canonical: Arc<str>,
    pub whole_hash: Hash16,
    pub parse_env_hash: Hash16,
}
