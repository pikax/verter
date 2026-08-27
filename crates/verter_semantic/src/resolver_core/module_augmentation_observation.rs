//! Observation DTOs for
//! [`crate::resolver_core::ResolverObservation::module_augmentation_index`].
//!
//! `verter_session::file_artifact_store::FileArtifactStore`'s
//! `augmentation_index` remains the session-owned authoritative store —
//! population, publication, exact-key self-healing, membership epochs, and
//! generation bumps stay session-only. These two types are the
//! narrow, immutable READ VIEW the session's `get_augmenter_set` peek
//! projects into: they omit `FileArtifactKey`'s exact-key self-healing
//! identity (`content_hash`, `parse_env_hash`, `parse_key`,
//! `file_language_id`, and the session-private
//! `build_toolchain_fingerprint`) entirely, carrying only what a kernel
//! consumer needs to (a) know which files contribute and in what order,
//! and (b) re-demand each contributor's declaration bodies through
//! [`crate::resolver_core::ResolverObservation::type_decl`]/
//! [`crate::resolver_core::ResolverObservation::value_decl`] — the session
//! side re-derives the exact artifact key from the live content hash when
//! it actually needs to re-fetch a contributor's raw facts: the
//! session side heals/materializes before constructing the immutable
//! observation").

use std::sync::Arc;

use crate::analysis::Hash16;

/// One augmenter file's identity contributing to a
/// [`ModuleAugmentationIndexObservation`].
///
/// Dependency-neutral mirror of `verter_session::file_artifact_store::
/// AugmenterEntry`, narrowed to the two fields a kernel consumer needs:
/// `canonical` (to re-demand the contributor's declaration bodies via
/// `type_decl`/`value_decl`) and `parse_stable_hash` (the structural hash
/// the augmenter-set fingerprint folds over, and the tiebreaker in the
/// session's stable `(canonical, parse_stable_hash)` contributor order).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AugmentationContributorObservation {
    pub canonical: Arc<str>,
    pub parse_stable_hash: Hash16,
}

/// Immutable snapshot of every augmenter contributing to one
/// `AugmentationTargetKey`, as observed by a kernel attempt.
///
/// `Complete(ModuleAugmentationIndexObservation { contributors: [], .. })`
/// is the stable "no augmenter contributes" fact — never itself a
/// reason to request more inputs. `contributors` is pre-sorted by
/// `(canonical, parse_stable_hash)`, matching the session's
/// `AugmenterSet.entries` order (deterministic, discovery-order-
/// independent), so a kernel consumer never needs to re-sort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleAugmentationIndexObservation {
    /// `stable_hash(contributors)` — the basis of `ModuleAugmentationIndexShape`,
    /// the sole cache-validity rail for a merged augmentation value (a
    /// contributor add/remove/reorder moves this fingerprint).
    pub fingerprint: Hash16,
    pub contributors: Arc<[AugmentationContributorObservation]>,
}
