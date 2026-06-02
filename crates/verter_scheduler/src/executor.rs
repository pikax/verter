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
use crate::stage::TaskKind;

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

/// Borrowed context handed to [`StageExecutor::dispatch_cpu_task`] — the
/// CPU-path unifier for `Parse` / `Analysis` / `Artifact` / `CacheNode` work.
///
/// The scheduler dispatch path constructs this from the dequeued
/// [`ReadyJob`](crate::dag::ReadyJob): the canonical id and generation address
/// the file node, the available source/analysis snapshots feed the CPU stages
/// that need upstream state, and the cancellation token lets a long-running CPU
/// task observe supersession. The struct borrows for the dispatch lifetime
/// only — it never enters a host-owned cache.
pub struct CpuTaskContext<'a> {
    /// Canonical id of the file this CPU task is for. Empty for
    /// [`TaskKind::CacheNode`](crate::stage::TaskKind) work, which is not
    /// addressed by a file node.
    pub canonical_id: &'a str,
    /// Generation the work was admitted at.
    pub generation: u64,
    /// Committed source snapshot, when available (present for `Parse` /
    /// `Analysis` / `Artifact`; absent for `CacheNode`).
    pub source: Option<&'a SourceSnapshot>,
    /// Committed analysis snapshot, when available (present for `Artifact`).
    pub analysis: Option<&'a AnalysisSnapshot>,
    /// Cooperative cancellation flag for this work item.
    pub cancellation: &'a CancellationToken,
}

/// Result of a [`StageExecutor::dispatch_cpu_task`] CPU stage.
///
/// The scheduler treats the produced snapshot as opaque — it commits whichever
/// snapshot variant the stage produced and fans out completion. `Parse` and
/// `Analysis` produce an [`AnalysisSnapshot`]-bearing outcome path, `Artifact`
/// an [`ArtifactSnapshot`], and `CacheNode` produces no snapshot (the cache
/// layer owns its own storage).
#[derive(Debug)]
pub enum CpuTaskOutcome {
    /// A source/parse stage committed a [`SourceSnapshot`].
    Source(Arc<SourceSnapshot>),
    /// An analysis stage committed an [`AnalysisSnapshot`].
    Analysis(Arc<AnalysisSnapshot>),
    /// An artifact stage committed an [`ArtifactSnapshot`].
    Artifact(Arc<ArtifactSnapshot>),
    /// Cache-node materialisation completed; the cache layer owns the result.
    CacheNode,
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
    /// The cache layer above the scheduler recovers its concrete executor
    /// through this hook so it can run a cache node without the scheduler ever
    /// importing or naming a session-side type — the scheduler exposes the
    /// hook only; it does not depend on `verter_session`.
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

    /// CPU-path unifier for `Parse` / `Analysis` / `Artifact` / `CacheNode`
    /// work. `Load` stays on the I/O path and never reaches this method.
    ///
    /// The default is a loud, typed "unsupported" error (not a silent
    /// success): an executor that has not opted into the unified CPU dispatch
    /// must fail explicitly. The host executor overrides this to drive its CPU
    /// stages through one entry point.
    fn dispatch_cpu_task(
        &self,
        _task_kind: &TaskKind,
        _ctx: CpuTaskContext<'_>,
    ) -> Result<CpuTaskOutcome, StageError> {
        Err(StageError {
            message: "dispatch_cpu_task is not implemented by this StageExecutor — \
                      unified CPU dispatch requires an executor that overrides it"
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
