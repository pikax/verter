//! Dependency-neutral env-hash bundle: four plain `Hash16` fields and no
//! host/session handles. `verter_session::session_view` re-exports this for
//! its view API.

use crate::analysis::types::Hash16;

/// Carries `[parse, resolve, type_, lib]` env-hash dimensions. The
/// `Default` impl returns an all-zero bundle and is reserved for test
/// fixtures + arch guards; production callers compose the bundle from the
/// workspace's published env-hash tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EnvHashes {
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}
