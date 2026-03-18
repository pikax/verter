//! Per-file node: ArcSwap snapshots, generation counter, pending requests.
//!
//! Each [`FileNode`] holds the current stage snapshots for a single file.
//! Snapshots are immutable once committed (Arc-wrapped). Replacement is
//! atomic via ArcSwap. Generation fencing ensures coherence:
//!
//! ```text
//! source.generation ≥ analysis.generation ≥ artifact.generation
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use dashmap::DashMap;
use parking_lot::Mutex;

use crate::job::{CompletionSender, CompletionState, RequestResult};
use crate::stage::{Priority, TargetStage, TaskKind};

/// Opaque host-specific data stored inside snapshots.
///
/// The scheduler is domain-agnostic — it coordinates stages and tracks
/// generations but doesn't know about SFC structure, analysis types, or
/// compilation output. The host stores its concrete data via this trait.
pub trait SnapshotData: Send + Sync + std::fmt::Debug + 'static {
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Unit data for tests and cases where no host-specific data is needed.
#[derive(Debug, Clone)]
pub struct EmptyData;

impl SnapshotData for EmptyData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Immutable source snapshot — committed after Source stage.
pub struct SourceSnapshot {
    /// Raw file content.
    pub source: Arc<str>,
    /// Full-content hash for change detection.
    pub whole_hash: [u8; 16],
    /// Semantic hash (ignoring whitespace, comments in non-significant positions).
    pub semantic_hash: [u8; 16],
    /// Generation this snapshot was produced at.
    pub generation: u64,
    /// Host-specific data (parse results, descriptors, etc.).
    pub data: Arc<dyn SnapshotData>,
}

impl Clone for SourceSnapshot {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            whole_hash: self.whole_hash,
            semantic_hash: self.semantic_hash,
            generation: self.generation,
            data: Arc::clone(&self.data),
        }
    }
}

impl std::fmt::Debug for SourceSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceSnapshot")
            .field("generation", &self.generation)
            .field("source_len", &self.source.len())
            .finish()
    }
}

impl SourceSnapshot {
    /// Create a snapshot with no host-specific data (tests, WASM).
    pub fn new_empty(source: Arc<str>, generation: u64) -> Self {
        Self {
            source,
            whole_hash: [0; 16],
            semantic_hash: [0; 16],
            generation,
            data: Arc::new(EmptyData),
        }
    }

    /// Downcast the host data to a concrete type.
    pub fn downcast_data<T: 'static>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
}

/// Immutable analysis snapshot — committed after Analysis stage.
pub struct AnalysisSnapshot {
    /// Generation this snapshot was produced at.
    pub generation: u64,
    /// Host-specific analysis data (script analysis, exports, styles, etc.).
    pub data: Arc<dyn SnapshotData>,
}

impl Clone for AnalysisSnapshot {
    fn clone(&self) -> Self {
        Self {
            generation: self.generation,
            data: Arc::clone(&self.data),
        }
    }
}

impl std::fmt::Debug for AnalysisSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisSnapshot")
            .field("generation", &self.generation)
            .finish()
    }
}

impl AnalysisSnapshot {
    /// Create a snapshot with no host-specific data (tests).
    pub fn new_empty(generation: u64) -> Self {
        Self {
            generation,
            data: Arc::new(EmptyData),
        }
    }

    /// Downcast the host data to a concrete type.
    pub fn downcast_data<T: 'static>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
}

/// Immutable artifact snapshot — committed after Artifact stage.
pub struct ArtifactSnapshot {
    /// Generation this snapshot was produced at.
    pub generation: u64,
    /// Profile hash identifying which compile variant produced this.
    pub profile_hash: u64,
    /// Host-specific compilation output (virtual files, TSX, diagnostics).
    pub data: Arc<dyn SnapshotData>,
}

impl Clone for ArtifactSnapshot {
    fn clone(&self) -> Self {
        Self {
            generation: self.generation,
            profile_hash: self.profile_hash,
            data: Arc::clone(&self.data),
        }
    }
}

impl ArtifactSnapshot {
    /// Downcast the host data to a concrete type.
    pub fn downcast_data<T: 'static>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
}

impl std::fmt::Debug for ArtifactSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactSnapshot")
            .field("generation", &self.generation)
            .field("profile_hash", &self.profile_hash)
            .finish()
    }
}

/// Classification of a file by its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// Vue Single File Component (.vue).
    VueSfc,
    /// Non-Vue source file (.ts, .tsx, .js, .jsx, .d.ts, etc.).
    NonSfc,
}

/// Per-file state node. All snapshots are immutable + Arc-wrapped.
/// Replacement is atomic via ArcSwap.
pub struct FileNode {
    /// Canonical file identifier.
    pub canonical_id: String,
    /// File classification.
    pub file_kind: FileKind,
    /// Monotonically increasing generation counter.
    /// Bumped on each source update.
    pub(crate) generation: AtomicU64,
    /// Current source snapshot (None if not yet loaded).
    pub(crate) source: ArcSwap<Option<Arc<SourceSnapshot>>>,
    /// Current analysis snapshot (None if not yet analyzed).
    pub(crate) analysis: ArcSwap<Option<Arc<AnalysisSnapshot>>>,
    /// Per-profile artifact slots. DashMap provides concurrent per-key access.
    pub(crate) artifacts: DashMap<u64, Arc<ArtifactSnapshot>>,
    /// Generation-scoped pending source buffer. Set during admission for
    /// source-providing requests. The Source job reads from this slot.
    pub(crate) pending_source: ArcSwap<Option<(u64, Arc<str>)>>,
    /// Per-file pending request groups.
    pub(crate) pending_requests: PendingRequests,
}

impl FileNode {
    /// Create a new file node with generation 0.
    pub fn new(canonical_id: String, file_kind: FileKind) -> Self {
        Self {
            canonical_id,
            file_kind,
            generation: AtomicU64::new(0),
            source: ArcSwap::new(Arc::new(None)),
            analysis: ArcSwap::new(Arc::new(None)),
            artifacts: DashMap::new(),
            pending_source: ArcSwap::new(Arc::new(None)),
            pending_requests: PendingRequests::new(),
        }
    }

    /// Current generation (acquire ordering for cross-thread visibility).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Bump generation and return the new value.
    pub(crate) fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Returns the current source snapshot if it matches the node's generation.
    pub fn current_source(&self) -> Option<Arc<SourceSnapshot>> {
        let node_gen = self.generation.load(Ordering::Acquire);
        let guard = self.source.load();
        match guard.as_ref() {
            Some(s) if s.generation == node_gen => Some(Arc::clone(s)),
            _ => None,
        }
    }

    /// Returns the current analysis snapshot if generation-coherent.
    ///
    /// Coherence: `analysis.generation == source.generation == node.generation`.
    pub fn current_analysis(&self) -> Option<Arc<AnalysisSnapshot>> {
        let node_gen = self.generation.load(Ordering::Acquire);
        let src = self.source.load();
        let analysis = self.analysis.load();
        match (src.as_ref(), analysis.as_ref()) {
            (Some(s), Some(a)) if a.generation == s.generation && s.generation == node_gen => {
                Some(Arc::clone(a))
            }
            _ => None,
        }
    }

    /// Returns the current artifact for a profile if generation-coherent.
    ///
    /// Coherence: `artifact.generation == analysis.generation == source.generation == node.generation`.
    pub fn current_artifact(&self, profile_hash: u64) -> Option<Arc<ArtifactSnapshot>> {
        let node_gen = self.generation.load(Ordering::Acquire);
        let src_gen = match self.source.load().as_ref() {
            Some(s) => s.generation,
            None => return None,
        };
        let analysis_gen = match self.analysis.load().as_ref() {
            Some(a) => a.generation,
            None => return None,
        };
        if src_gen != node_gen || analysis_gen != node_gen {
            return None;
        }
        let entry = self.artifacts.get(&profile_hash)?;
        let art = Arc::clone(entry.value());
        if art.generation == node_gen {
            Some(art)
        } else {
            None
        }
    }

    /// Returns the most recent artifact regardless of generation (last-known-good).
    pub fn last_known_good_artifact(&self, profile_hash: u64) -> Option<Arc<ArtifactSnapshot>> {
        self.artifacts
            .get(&profile_hash)
            .map(|e| Arc::clone(e.value()))
    }
}

// ── Pending Requests ──

/// Per-file pending request storage.
pub struct PendingRequests {
    inner: Mutex<Vec<PendingRequestGroup>>,
}

impl Default for PendingRequests {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingRequests {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    /// Register a sender for a request. If a matching group exists (same
    /// generation + target), the sender is added to it and a priority upgrade
    /// is returned if the new priority is higher. Otherwise, a new group is created.
    pub fn register(
        &self,
        generation: u64,
        target: TargetStage,
        priority: Priority,
        sender: CompletionSender<RequestResult>,
    ) -> Option<Priority> {
        let mut groups = self.inner.lock();
        // Try to find an existing group
        for group in groups.iter_mut() {
            if group.generation == generation && group.target == target {
                return group.add_sender(sender, priority);
            }
        }
        // Create new group
        groups.push(PendingRequestGroup {
            generation,
            target,
            priority,
            senders: vec![sender],
        });
        None
    }

    /// Signal all groups matching the given generation and completed task kind.
    ///
    /// - Groups whose target is satisfied → signal `Ready(result)` and remove.
    /// - Groups with older generation → signal `Superseded` and remove.
    /// - Groups with target not yet reached → leave in place.
    ///
    /// Returns the number of groups signaled.
    pub fn signal_stage_complete(
        &self,
        generation: u64,
        completed: &TaskKind,
        result: &RequestResult,
    ) -> usize {
        let mut groups = self.inner.lock();
        let mut signaled = 0;
        groups.retain_mut(|group| {
            if group.generation < generation {
                // Stale generation — superseded
                group.signal_all(CompletionState::Superseded);
                signaled += 1;
                false
            } else if group.generation == generation && group.target.is_satisfied_by(completed) {
                // Target reached — signal ready
                group.signal_all(CompletionState::Ready(result.clone()));
                signaled += 1;
                false
            } else {
                true // keep — target not yet reached or future generation
            }
        });
        signaled
    }

    /// Signal all pending groups with `Superseded` for generations older than `current_gen`.
    pub fn supersede_old_generations(&self, current_gen: u64) -> usize {
        let mut groups = self.inner.lock();
        let mut count = 0;
        groups.retain_mut(|group| {
            if group.generation < current_gen {
                group.signal_all(CompletionState::Superseded);
                count += 1;
                false
            } else {
                true
            }
        });
        count
    }

    /// Signal all pending groups at a generation with `Failed`.
    pub fn signal_failed(&self, generation: u64, error: crate::job::SchedulerError) {
        let mut groups = self.inner.lock();
        groups.retain_mut(|group| {
            if group.generation == generation {
                group.signal_all(CompletionState::Failed(error.clone()));
                false
            } else {
                true
            }
        });
    }

    /// Signal only pending groups whose target matches the failed stage.
    ///
    /// Used for Artifact failures: only the specific profile that failed is
    /// signaled; other profiles at the same generation remain pending.
    pub fn signal_failed_for_stage(
        &self,
        generation: u64,
        failed_stage: &TaskKind,
        error: crate::job::SchedulerError,
    ) {
        let mut groups = self.inner.lock();
        groups.retain_mut(|group| {
            if group.generation == generation && group.target.is_satisfied_by(failed_stage) {
                group.signal_all(CompletionState::Failed(error.clone()));
                false
            } else {
                true
            }
        });
    }

    /// Signal all pending groups with `Shutdown`. Used during scheduler drop.
    pub fn signal_shutdown(&self) {
        let mut groups = self.inner.lock();
        for group in groups.drain(..) {
            for sender in group.senders {
                sender.send(CompletionState::Shutdown);
            }
        }
    }

    /// Get the highest priority among all pending request groups at a generation.
    /// Used by the driver to preserve priority across stage transitions.
    pub fn highest_priority_for_generation(&self, generation: u64) -> Option<Priority> {
        let groups = self.inner.lock();
        groups
            .iter()
            .filter(|g| g.generation == generation)
            .map(|g| g.priority)
            .min() // min ordinal = highest priority
    }

    /// Get pending artifact profile hashes and their priorities for a generation.
    /// Used by the driver after Analysis completes to enqueue Artifact jobs.
    pub fn get_pending_artifact_profiles(&self, generation: u64) -> Vec<(u64, Priority)> {
        let groups = self.inner.lock();
        groups
            .iter()
            .filter_map(|group| {
                if group.generation == generation {
                    if let TargetStage::Artifact { profile_hash } = &group.target {
                        return Some((*profile_hash, group.priority));
                    }
                }
                None
            })
            .collect()
    }

    /// Number of pending groups (for testing/diagnostics).
    pub fn pending_count(&self) -> usize {
        self.inner.lock().len()
    }
}

/// Groups all callers waiting for the same `(generation, target)`.
struct PendingRequestGroup {
    generation: u64,
    target: TargetStage,
    priority: Priority,
    senders: Vec<CompletionSender<RequestResult>>,
}

impl PendingRequestGroup {
    /// Add another caller's sender to this group.
    /// Returns a priority upgrade if the new caller has higher priority.
    fn add_sender(
        &mut self,
        sender: CompletionSender<RequestResult>,
        priority: Priority,
    ) -> Option<Priority> {
        self.senders.push(sender);
        let old = self.priority;
        self.priority = std::cmp::min(self.priority, priority);
        if self.priority < old {
            Some(self.priority)
        } else {
            None
        }
    }

    /// Signal all callers in this group.
    fn signal_all(&mut self, state: CompletionState<RequestResult>) {
        for sender in self.senders.drain(..) {
            sender.send(state.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::completion_pair;

    #[test]
    fn file_node_generation_monotonic() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        assert_eq!(node.generation(), 0);

        let g1 = node.bump_generation();
        assert_eq!(g1, 1);
        assert_eq!(node.generation(), 1);

        let g2 = node.bump_generation();
        assert_eq!(g2, 2);
        assert_eq!(node.generation(), 2);
    }

    #[test]
    fn current_source_returns_none_when_empty() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_source_returns_some_when_generation_matches() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        let gen = node.bump_generation();

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), gen));
        node.source.store(Arc::new(Some(snap)));

        let result = node.current_source();
        assert!(result.is_some());
        assert_eq!(&*result.unwrap().source, "hello");
    }

    #[test]
    fn current_source_returns_none_when_generation_stale() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        let gen = node.bump_generation();

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), gen));
        node.source.store(Arc::new(Some(snap)));

        // Advance generation — snapshot is now stale
        node.bump_generation();
        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_analysis_coherence() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        let gen = node.bump_generation();

        // No source → analysis must be None
        assert!(node.current_analysis().is_none());

        // Set source at gen
        let src = Arc::new(SourceSnapshot::new_empty(Arc::from("x"), gen));
        node.source.store(Arc::new(Some(src)));

        // No analysis → still None
        assert!(node.current_analysis().is_none());

        // Set analysis at gen
        let analysis = Arc::new(AnalysisSnapshot::new_empty(gen));
        node.analysis.store(Arc::new(Some(analysis)));

        // Now coherent
        assert!(node.current_analysis().is_some());

        // Advance generation — breaks coherence
        node.bump_generation();
        assert!(node.current_analysis().is_none());
    }

    #[test]
    fn current_artifact_coherence() {
        let node = FileNode::new("test.vue".into(), FileKind::VueSfc);
        let gen = node.bump_generation();
        let ph: u64 = 0xABCD;

        // Set source + analysis at gen
        node.source
            .store(Arc::new(Some(Arc::new(SourceSnapshot::new_empty(
                Arc::from("x"),
                gen,
            )))));
        node.analysis
            .store(Arc::new(Some(Arc::new(AnalysisSnapshot::new_empty(gen)))));

        // No artifact yet
        assert!(node.current_artifact(ph).is_none());

        // Insert artifact at gen
        let art = Arc::new(ArtifactSnapshot {
            generation: gen,
            profile_hash: ph,
            data: Arc::new(EmptyData),
        });
        node.artifacts.insert(ph, art);

        // Now coherent
        assert!(node.current_artifact(ph).is_some());

        // Wrong profile hash
        assert!(node.current_artifact(0x9999).is_none());

        // Advance generation — stale
        node.bump_generation();
        assert!(node.current_artifact(ph).is_none());

        // But last_known_good still returns it
        assert!(node.last_known_good_artifact(ph).is_some());
    }

    #[test]
    fn pending_requests_register_and_signal() {
        let pending = PendingRequests::new();
        let (handle, sender) = completion_pair::<RequestResult>();

        // Register a request for Analysis at gen 1
        let upgrade = pending.register(1, TargetStage::Analysis, Priority::Interactive, sender);
        assert!(upgrade.is_none()); // first registration, no upgrade
        assert_eq!(pending.pending_count(), 1);

        // Signal stage complete for Analysis at gen 1
        let result = RequestResult::Analysis(Arc::new(AnalysisSnapshot::new_empty(1)));
        let count = pending.signal_stage_complete(1, &TaskKind::Analysis, &result);
        assert_eq!(count, 1);
        assert_eq!(pending.pending_count(), 0);

        // Handle should be resolved
        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
    }

    #[test]
    fn pending_requests_dedup_group() {
        let pending = PendingRequests::new();
        let (h1, s1) = completion_pair::<RequestResult>();
        let (h2, s2) = completion_pair::<RequestResult>();

        // Two callers for the same (gen, target)
        pending.register(1, TargetStage::Analysis, Priority::Background, s1);
        let upgrade = pending.register(1, TargetStage::Analysis, Priority::Critical, s2);

        // Should be grouped — only 1 pending group
        assert_eq!(pending.pending_count(), 1);
        // Priority upgrade from Background to Critical
        assert_eq!(upgrade, Some(Priority::Critical));

        // Signal — both handles resolve
        let result = RequestResult::Analysis(Arc::new(AnalysisSnapshot::new_empty(1)));
        pending.signal_stage_complete(1, &TaskKind::Analysis, &result);

        assert!(h1.try_get().unwrap().is_ready());
        assert!(h2.try_get().unwrap().is_ready());
    }

    #[test]
    fn pending_requests_supersede_old_generations() {
        let pending = PendingRequests::new();
        let (h_old, s_old) = completion_pair::<RequestResult>();
        let (h_new, s_new) = completion_pair::<RequestResult>();

        pending.register(1, TargetStage::Analysis, Priority::Interactive, s_old);
        pending.register(2, TargetStage::Analysis, Priority::Interactive, s_new);
        assert_eq!(pending.pending_count(), 2);

        // Supersede gen < 2
        let count = pending.supersede_old_generations(2);
        assert_eq!(count, 1);
        assert_eq!(pending.pending_count(), 1);

        // Old handle is Superseded
        match h_old.try_get().unwrap() {
            CompletionState::Superseded => {}
            other => panic!("expected Superseded, got {:?}", other),
        }

        // New handle still pending
        assert!(!h_new.is_resolved());
    }

    #[test]
    fn pending_requests_shutdown() {
        let pending = PendingRequests::new();
        let (h1, s1) = completion_pair::<RequestResult>();
        let (h2, s2) = completion_pair::<RequestResult>();

        pending.register(1, TargetStage::Source, Priority::Background, s1);
        pending.register(2, TargetStage::Analysis, Priority::Critical, s2);

        pending.signal_shutdown();
        assert_eq!(pending.pending_count(), 0);

        match h1.try_get().unwrap() {
            CompletionState::Shutdown => {}
            other => panic!("expected Shutdown, got {:?}", other),
        }
        match h2.try_get().unwrap() {
            CompletionState::Shutdown => {}
            other => panic!("expected Shutdown, got {:?}", other),
        }
    }

    #[test]
    fn pending_requests_target_not_yet_reached_stays() {
        let pending = PendingRequests::new();
        let (handle, sender) = completion_pair::<RequestResult>();

        // Register for Artifact target
        pending.register(
            1,
            TargetStage::Artifact { profile_hash: 42 },
            Priority::Interactive,
            sender,
        );

        // Signal Source complete — should NOT satisfy Artifact target
        let result = RequestResult::Source(Arc::new(SourceSnapshot::new_empty(Arc::from("x"), 1)));
        let count = pending.signal_stage_complete(1, &TaskKind::Source, &result);
        assert_eq!(count, 0);
        assert_eq!(pending.pending_count(), 1);
        assert!(!handle.is_resolved());

        // Signal Analysis complete — still not enough for Artifact
        let result = RequestResult::Analysis(Arc::new(AnalysisSnapshot::new_empty(1)));
        let count = pending.signal_stage_complete(1, &TaskKind::Analysis, &result);
        assert_eq!(count, 0);
        assert_eq!(pending.pending_count(), 1);
        assert!(!handle.is_resolved());

        // Signal Artifact complete — now satisfied
        let result = RequestResult::Artifact(Arc::new(ArtifactSnapshot {
            generation: 1,
            profile_hash: 42,
            data: Arc::new(EmptyData),
        }));
        let count =
            pending.signal_stage_complete(1, &TaskKind::Artifact { profile_hash: 42 }, &result);
        assert_eq!(count, 1);
        assert_eq!(pending.pending_count(), 0);
        assert!(handle.is_resolved());
    }
}
