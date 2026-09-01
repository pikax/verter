//! Per-file node: ArcSwap snapshots and generation counter.
//!
//! Each [`FileNode`] holds the current stage snapshots for a single file.
//! Snapshots are immutable once committed (Arc-wrapped). Replacement is
//! atomic via ArcSwap. Generation fencing ensures coherence:
//!
//! ```text
//! source.generation ≥ analysis.generation ≥ artifact.generation
//! ```
//!
//! Request-group bookkeeping (caller senders, dedup, priority
//! inheritance, completion fan-out) lives on the [`SchedulerDag`] —
//! the single readiness authority. This node only owns the per-file
//! snapshot triple, the generation counter, and the per-profile
//! artifact slots.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use verter_language::FileLanguage;

use arc_swap::ArcSwap;
use dashmap::DashMap;

mod live_generation {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::source_root::SourcePublication;

    /// Opaque live-generation storage. The raw atomic never escapes this
    /// private module, and the only advancing operation requires the
    /// publication capability minted under the source-directory hold.
    #[derive(Debug)]
    pub(super) struct LiveGenerationCounter {
        raw: AtomicU64,
    }

    impl LiveGenerationCounter {
        pub(super) fn new(generation: u64) -> Self {
            Self {
                raw: AtomicU64::new(generation),
            }
        }

        pub(super) fn read(&self) -> u64 {
            self.raw.load(Ordering::Acquire)
        }

        pub(super) fn advance(&self, _publication: &SourcePublication) -> u64 {
            self.raw.fetch_add(1, Ordering::AcqRel) + 1
        }
    }
}

use live_generation::LiveGenerationCounter;

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

/// Per-file state node. All snapshots are immutable + Arc-wrapped.
/// Replacement is atomic via ArcSwap.
///
/// Request-group bookkeeping is OWNED BY THE DAG, not the node. The
/// node carries only the snapshot triple, the generation counter, the
/// per-profile artifact slots, and the per-file overlay source buffer.
#[deny(private_interfaces)]
pub struct FileNode {
    /// Canonical file identifier.
    pub canonical_id: String,
    /// File language (the resolved classification row).
    pub file_language: FileLanguage,
    /// Monotonically increasing generation counter.
    /// Bumped on each source update.
    ///
    /// The private child-module type is deliberately less visible than this
    /// crate. Together with `deny(private_interfaces)`, widening this field to
    /// `pub(crate)` is a compile error; the raw atomic remains unreachable even
    /// if that field visibility is accidentally edited.
    generation: LiveGenerationCounter,
    /// Current source snapshot (None if not yet loaded).
    pub(crate) source: ArcSwap<Option<Arc<SourceSnapshot>>>,
    /// Generation whose Source completion has been integrated into scheduler
    /// dependency/blocker state. Snapshot publication happens first on a
    /// worker; only the driver advances this fence under the scheduler lock.
    source_integrated_generation: AtomicU64,
    /// Disambiguates "not integrated" from integrated generation zero.
    source_integration_ready: AtomicBool,
    /// Current analysis snapshot (None if not yet analyzed).
    pub(crate) analysis: ArcSwap<Option<Arc<AnalysisSnapshot>>>,
    /// Per-profile artifact slots. DashMap provides concurrent per-key access.
    pub(crate) artifacts: DashMap<u64, Arc<ArtifactSnapshot>>,
    /// Generation-scoped pending source buffer. Set during admission for
    /// source-providing requests. The Source job reads from this slot.
    pub(crate) pending_source: ArcSwap<Option<(u64, Arc<str>)>>,
    /// Process-unique id for THIS node object.
    ///
    /// The generation identifies a version of a file's content; the
    /// incarnation identifies the node OBJECT serving it. Two different
    /// nodes for the same canonical can coexist at the SAME generation
    /// (a replacement publishes a fresh node starting at generation 0/
    /// floor), so a generation comparison alone cannot tell a dispatched
    /// node from its replacement. Work carries this id from dispatch so
    /// its completion can prove it is still publishing for the
    /// incarnation it actually ran against.
    ///
    /// Monotonic and never reused, so it cannot ABA the way a reclaimed
    /// pointer address can.
    incarnation_id: u64,
}

/// Source of process-unique [`FileNode::incarnation_id`] values.
static NEXT_INCARNATION_ID: AtomicU64 = AtomicU64::new(1);

impl FileNode {
    /// Create a new file node with generation 0.
    pub fn new(canonical_id: String, file_language: FileLanguage) -> Self {
        Self::new_at(canonical_id, file_language, 0)
    }

    /// Create a file node whose generation is assigned at construction.
    ///
    /// Use this for unpublished nodes (first insertion, replacement
    /// incarnation, generation-floor restart). Live generation advances
    /// on an already-published node must go through
    /// [`crate::source_root::SourcePublication::bump_node_generation`].
    pub(crate) fn new_at(
        canonical_id: String,
        file_language: FileLanguage,
        generation: u64,
    ) -> Self {
        Self {
            canonical_id,
            file_language,
            generation: LiveGenerationCounter::new(generation),
            source: ArcSwap::new(Arc::new(None)),
            source_integrated_generation: AtomicU64::new(0),
            source_integration_ready: AtomicBool::new(false),
            analysis: ArcSwap::new(Arc::new(None)),
            artifacts: DashMap::new(),
            pending_source: ArcSwap::new(Arc::new(None)),
            incarnation_id: NEXT_INCARNATION_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Process-unique id of this node object. See
    /// [`FileNode::incarnation_id`].
    pub fn incarnation_id(&self) -> u64 {
        self.incarnation_id
    }

    /// Current generation (acquire ordering for cross-thread visibility).
    pub fn generation(&self) -> u64 {
        self.generation.read()
    }

    /// Bump generation and return the new value.
    ///
    /// The `_proof` argument is a [`crate::source_root::SourcePublication`],
    /// which exists only inside [`crate::source_root::SchedulerSourceDirectory::publish_transition`].
    /// A bare atomic bump from outside that hold does not compile.
    pub(crate) fn bump_generation(&self, proof: &crate::source_root::SourcePublication) -> u64 {
        self.source_integration_ready
            .store(false, Ordering::Release);
        self.generation.advance(proof)
    }

    /// Returns the current source snapshot if it matches the node's generation.
    pub fn current_source(&self) -> Option<Arc<SourceSnapshot>> {
        let node_gen = self.generation.read();
        let guard = self.source.load();
        match guard.as_ref() {
            Some(s) if s.generation == node_gen => Some(Arc::clone(s)),
            _ => None,
        }
    }

    /// Returns the current Source only after its completion has integrated all
    /// dependency facts under the scheduler lock.
    pub fn current_integrated_source(&self) -> Option<Arc<SourceSnapshot>> {
        let guard = self.source.load();
        let snapshot = guard.as_ref().as_ref()?;
        let node_gen = self.generation.read();
        if snapshot.generation != node_gen
            || !self.source_integration_ready.load(Ordering::Acquire)
            || self.source_integrated_generation.load(Ordering::Relaxed) != node_gen
        {
            return None;
        }
        Some(Arc::clone(snapshot))
    }

    /// Publish the integration fence for `generation` after dependency state
    /// has been integrated under the scheduler lock. Returns false for a
    /// stale/missing snapshot.
    pub(crate) fn mark_source_integrated(&self, generation: u64) -> bool {
        if self.generation.read() != generation || self.current_source().is_none() {
            return false;
        }
        self.source_integrated_generation
            .store(generation, Ordering::Relaxed);
        self.source_integration_ready.store(true, Ordering::Release);
        true
    }

    /// Returns the current analysis snapshot if generation-coherent.
    ///
    /// Coherence: `analysis.generation == source.generation == node.generation`.
    pub fn current_analysis(&self) -> Option<Arc<AnalysisSnapshot>> {
        let node_gen = self.generation.read();
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
        let node_gen = self.generation.read();
        let src_gen = self.source.load().as_deref()?.generation;
        let analysis_gen = self.analysis.load().as_deref()?.generation;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_node_generation_is_assigned_at_construction() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        assert_eq!(node.generation(), 0);
        let started = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 2);
        assert_eq!(started.generation(), 2);
    }

    #[test]
    fn current_source_returns_none_when_empty() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_source_returns_some_when_generation_matches() {
        let node = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 1);

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), 1));
        node.source.store(Arc::new(Some(snap)));

        let result = node.current_source();
        assert!(result.is_some());
        assert_eq!(&*result.unwrap().source, "hello");
    }

    #[test]
    fn generation_zero_source_is_not_integrated_until_marked() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), 0));
        node.source.store(Arc::new(Some(snap)));

        assert!(node.current_source().is_some());
        assert!(node.current_integrated_source().is_none());
        assert!(node.mark_source_integrated(0));
        assert!(node.current_integrated_source().is_some());
    }

    #[test]
    fn current_source_returns_none_when_generation_stale() {
        let node = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 2);

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), 1));
        node.source.store(Arc::new(Some(snap)));

        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_analysis_coherence() {
        let node = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 1);

        // No source → analysis must be None
        assert!(node.current_analysis().is_none());

        // Set source at gen
        let src = Arc::new(SourceSnapshot::new_empty(Arc::from("x"), 1));
        node.source.store(Arc::new(Some(src)));

        // No analysis → still None
        assert!(node.current_analysis().is_none());

        // Set analysis at gen
        let analysis = Arc::new(AnalysisSnapshot::new_empty(1));
        node.analysis.store(Arc::new(Some(analysis)));

        // Now coherent
        assert!(node.current_analysis().is_some());

        // A later incarnation of the same canonical starts at a higher
        // generation — snapshot identity is construction-assigned, not
        // a lock-free bump.
        let advanced = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 2);
        advanced
            .source
            .store(Arc::new(Some(Arc::new(SourceSnapshot::new_empty(
                Arc::from("x"),
                1,
            )))));
        advanced
            .analysis
            .store(Arc::new(Some(Arc::new(AnalysisSnapshot::new_empty(1)))));
        assert!(advanced.current_analysis().is_none());
    }

    #[test]
    fn current_artifact_coherence() {
        let node = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 1);
        let ph: u64 = 0xABCD;

        // Set source + analysis at gen
        node.source
            .store(Arc::new(Some(Arc::new(SourceSnapshot::new_empty(
                Arc::from("x"),
                1,
            )))));
        node.analysis
            .store(Arc::new(Some(Arc::new(AnalysisSnapshot::new_empty(1)))));

        // No artifact yet
        assert!(node.current_artifact(ph).is_none());

        // Insert artifact at gen
        let art = Arc::new(ArtifactSnapshot {
            generation: 1,
            profile_hash: ph,
            data: Arc::new(EmptyData),
        });
        node.artifacts.insert(ph, Arc::clone(&art));

        // Now coherent
        assert!(node.current_artifact(ph).is_some());

        // Wrong profile hash
        assert!(node.current_artifact(0x9999).is_none());

        // A later construction-assigned generation is stale for this
        // artifact, but last-known-good still returns it.
        let advanced = FileNode::new_at("test.vue".into(), FileLanguage::vue(), 2);
        advanced.artifacts.insert(ph, art);
        assert!(advanced.current_artifact(ph).is_none());
        assert!(advanced.last_known_good_artifact(ph).is_some());
    }
}
