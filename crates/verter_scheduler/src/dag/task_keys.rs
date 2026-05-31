//! Task-key / profile-hash conversion utilities — child module of `dag`.
//!
//! Houses the conversion helpers that lower a [`TaskKind`] into the
//! `(Option<Hash16>, FileStageKey, WorkKind)` triple consumed by DAG
//! admission, plus the legacy `u64` ↔ [`Hash16`] profile-hash codec.
//! The functions are stateless; they live here so the main `dag.rs`
//! file stays focused on the readiness DAG (admission, dedup,
//! gating, fan-out) rather than the task-shape mapping it consumes.
//!
//! Visibility note: every function is `pub` so the existing
//! `crate::dag::*` re-exports continue to work without callers
//! needing to know about the submodule split.

use super::{FileStageKey, Hash16, WorkKind};
use crate::stage::TaskKind;

/// Convert a [`TaskKind`] to its [`WorkKind`]/[`FileStageKey`] pair.
///
/// `Source` lowers to a `Load`+`Parse` superset; the driver's I/O pool
/// runs the load and the CPU pool runs the parse, but for DAG
/// admission they share one identity (the `Source` file-stage). The
/// adapter returns `WorkKind::Load` so dispatch routes through the I/O
/// pool first; the parse step is intrinsic to the executor's source
/// stage and does not re-enter the DAG as a separate node.
pub fn dag_keys_for_task(task: TaskKind) -> (Option<Hash16>, FileStageKey, WorkKind) {
    match task {
        TaskKind::Source => (None, FileStageKey::Source, WorkKind::Load),
        TaskKind::Analysis => (None, FileStageKey::Analysis, WorkKind::Analysis),
        TaskKind::Artifact { profile_hash } => (
            Some(profile_hash_to_bytes(profile_hash)),
            // For artifact, FileStageKey is unused — the caller routes
            // through `WorkNodeIdentity::Artifact`. Return Analysis as
            // a structural placeholder for non-artifact callers; the
            // artifact site never reads it.
            FileStageKey::Analysis,
            WorkKind::Artifact,
        ),
    }
}

/// Encode the legacy `u64` profile hash as a [`Hash16`].
pub fn profile_hash_to_bytes(profile_hash: u64) -> Hash16 {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&profile_hash.to_le_bytes());
    out
}

/// Decode a [`Hash16`] produced by [`profile_hash_to_bytes`] back to
/// the original `u64`. The upper 8 bytes are ignored — only the lower
/// 8 bytes participate in the round-trip.
pub fn profile_hash_from_bytes(bytes: Hash16) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(buf)
}
