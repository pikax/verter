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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_language::FileLanguage;

use arc_swap::ArcSwap;
use dashmap::DashMap;

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
pub struct FileNode {
    /// Canonical file identifier.
    pub canonical_id: String,
    /// File language (the resolved classification row).
    pub file_language: FileLanguage,
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
}

impl FileNode {
    /// Create a new file node with generation 0.
    pub fn new(canonical_id: String, file_language: FileLanguage) -> Self {
        Self {
            canonical_id,
            file_language,
            generation: AtomicU64::new(0),
            source: ArcSwap::new(Arc::new(None)),
            analysis: ArcSwap::new(Arc::new(None)),
            artifacts: DashMap::new(),
            pending_source: ArcSwap::new(Arc::new(None)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_node_generation_monotonic() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
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
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_source_returns_some_when_generation_matches() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        let gen = node.bump_generation();

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), gen));
        node.source.store(Arc::new(Some(snap)));

        let result = node.current_source();
        assert!(result.is_some());
        assert_eq!(&*result.unwrap().source, "hello");
    }

    #[test]
    fn current_source_returns_none_when_generation_stale() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
        let gen = node.bump_generation();

        let snap = Arc::new(SourceSnapshot::new_empty(Arc::from("hello"), gen));
        node.source.store(Arc::new(Some(snap)));

        // Advance generation — snapshot is now stale
        node.bump_generation();
        assert!(node.current_source().is_none());
    }

    #[test]
    fn current_analysis_coherence() {
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
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
        let node = FileNode::new("test.vue".into(), FileLanguage::vue());
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
}
