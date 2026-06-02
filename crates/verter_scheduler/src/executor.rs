//! Stage executor trait — allows hosts to plug in real parse/analysis/compile logic.
//!
//! The scheduler is domain-agnostic. It coordinates file stages, tracks generations,
//! and manages priority queues. The [`StageExecutor`] trait is the injection point
//! where the host provides the actual work for each stage.

use std::sync::Arc;

use crate::cache_id::SchedulerCacheId;
use crate::cancellation::CancellationToken;
use crate::dag::{Hash16, PinId};
use crate::node::{AnalysisSnapshot, ArtifactSnapshot, FileKind, SourceSnapshot};

/// Errors from stage execution.
#[derive(Debug, Clone)]
pub struct StageError {
    pub message: String,
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StageError {}

/// Dependency information extracted from a source snapshot.
///
/// Returned by [`StageExecutor::extract_deps`] after Source completes.
/// The scheduler uses this to record forward/reverse edges and register
/// blockers for files that need deps not yet analyzed.
#[derive(Debug, Clone, Default)]
pub struct ExtractedDeps {
    /// Canonical IDs of files this file depends on (forward deps).
    pub forward_deps: Vec<String>,
    /// Files that must reach Analysis before this file's Artifact can proceed.
    pub blocker_ids: Vec<String>,
}

/// Trait for plugging host-specific stage logic into the scheduler.
///
/// The scheduler calls these methods during stage execution on its worker
/// threads (rayon CPU pool for parse/analysis/compile, I/O pool for source
/// loading). Each method receives immutable inputs and returns an immutable
/// snapshot.
///
/// The default implementation provides stub snapshots suitable for tests.
/// The host overrides with real parse/analysis/compile logic.
pub trait StageExecutor: Send + Sync + 'static {
    /// Execute the Source stage: parse file content into a SourceSnapshot.
    ///
    /// Called after content is loaded (from overlay or disk). The implementation
    /// should tokenize/parse the content and populate the snapshot's `data` field
    /// with host-specific parse results.
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_kind: FileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<SourceSnapshot, StageError> {
        Ok(SourceSnapshot::new_empty(content, generation))
    }

    /// Execute the Analysis stage: analyze a parsed source snapshot.
    ///
    /// Called after Source is committed. Receives the current source snapshot.
    /// The implementation should run script analysis, style analysis, etc.
    fn execute_analysis(
        &self,
        _canonical_id: &str,
        _source: &SourceSnapshot,
        generation: u64,
    ) -> Result<AnalysisSnapshot, StageError> {
        Ok(AnalysisSnapshot::new_empty(generation))
    }

    /// Extract dependency information from a committed source snapshot.
    ///
    /// Called by the driver after Source stage commits. The scheduler uses the
    /// returned deps to record forward/reverse edges and register blockers.
    /// Default: no deps (standalone files).
    fn extract_deps(&self, _canonical_id: &str, _source: &SourceSnapshot) -> ExtractedDeps {
        ExtractedDeps::default()
    }

    /// Execute the Artifact stage: compile for a specific profile.
    ///
    /// Called after Analysis is committed. Receives the current source and
    /// analysis snapshots, plus the profile hash identifying the compilation variant.
    fn execute_artifact(
        &self,
        _canonical_id: &str,
        _source: &SourceSnapshot,
        _analysis: &AnalysisSnapshot,
        profile_hash: u64,
        generation: u64,
    ) -> Result<ArtifactSnapshot, StageError> {
        Ok(ArtifactSnapshot {
            generation,
            profile_hash,
            data: Arc::new(crate::node::EmptyData),
        })
    }

    /// Downcast hook on the scheduler's `dyn StageExecutor` trait object.
    ///
    /// The hook exists so a cache layer owned above the scheduler can recover
    /// its concrete executor from the erased trait object and run a cache node
    /// without the scheduler ever importing or naming a session-side type — the
    /// scheduler exposes the downcast point only; it does not depend on
    /// `verter_session`. No in-tree caller downcasts through it yet; the host
    /// cache-materialisation layer that owns the concrete executor will be that
    /// caller.
    ///
    /// This is a required method (mirroring [`SnapshotData::as_any`]) rather
    /// than a provided one: a `{ self }` default body cannot coerce `&Self`
    /// into `&dyn Any` without a `Self: Sized` bound, and that bound would
    /// remove the method from the trait object's vtable — defeating the
    /// downcast. Every impl writes the one-line body `{ self }`.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Materialise a session-owned cache node (CPU-bound).
    ///
    /// Receives the full cache identity from
    /// [`WorkNodeIdentity::CacheNode`](crate::dag::WorkNodeIdentity)
    /// (`cache_id` + `key_hash` + `view_epoch` + `snapshot_pin_id`) plus the
    /// cooperative cancellation token. No CPU permit is passed — permit
    /// acquisition/release is scheduling capacity control owned by the
    /// scheduler, not business execution.
    ///
    /// The default is a loud, typed "unsupported" error rather than a silent
    /// success: an executor that has not opted into cache-node execution must
    /// fail explicitly so a mis-wired cache-node dispatch surfaces at the call
    /// site instead of pretending the work happened. The host executor
    /// overrides this with the real cache-materialisation path.
    fn execute_cache_node(
        &self,
        _cache_id: SchedulerCacheId,
        _key_hash: Hash16,
        _view_epoch: u64,
        _snapshot_pin_id: PinId,
        _cancellation: &CancellationToken,
    ) -> Result<(), StageError> {
        Err(StageError {
            message: "execute_cache_node is not implemented by this StageExecutor — \
                      cache-node dispatch requires an executor that overrides it"
                .to_string(),
        })
    }
}

/// Default executor that produces stub snapshots (for tests and WASM).
#[derive(Debug)]
pub struct DefaultExecutor;

impl StageExecutor for DefaultExecutor {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
