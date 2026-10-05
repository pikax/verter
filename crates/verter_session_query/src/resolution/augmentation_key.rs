//! Dependency-neutral module-augmentation inverse-index key vocabulary.
//! `ProjectIdentity` wraps a
//! plain `Hash16`, `AugmentationPopulation` is `Base | Session(u64)`, and
//! `AugmentationTargetKind`'s variants (`InternedSpecifier`, `Arc<str>`,
//! `InternedGlobPattern`) are already `verter_semantic`/`verter_workspace`
//! fact-registry vocabulary, not session/host handles. None of the four
//! types has an `impl` block reaching session/host state.
//! `verter_session::file_artifact_store` re-exports all four for its store API.
//!
//! [`AugmenterSet`] / [`AugmenterEntry`] are the owned per-target answer the
//! session index stores: the sorted augmenter identities and their cached
//! fingerprint. They carry exact artifact keys, never artifacts.
//!
//! `FileArtifactStore`'s own `augmentation_index` `DashMap`, its
//! membership-epoch/retirement bookkeeping, and
//! `ensure_augmentation_index_populated`'s cold-scan/publish machinery
//! are session-owned. Only the key vocabulary crosses the observation
//! boundary, so
//! `verter_resolution::ResolverObservation::module_augmentation_index`
//! can accept the SAME key type the session-owned index is stored under
//! rather than a parallel shape.
//!
//! All four types implement `Ord`/`PartialOrd` because
//! [`crate::resolution::InputKey`] requires `Ord` for `LoadSet`'s
//! normalize-sort-dedup contract.

use std::sync::Arc;

use crate::analysis::types::Hash16;
use crate::facts::registry::{InternedGlobPattern, InternedSpecifier};

// ── Project identity wrapper ──

/// Thin newtype around the 16-byte `project_identity` value produced by
/// `IdeProjectConfig::project_identity()`.
///
/// Used as a key dimension on [`AugmentationTargetKey`] to keep
/// augmentation entries from one project from poisoning a sibling project
/// under the same syntactic specifier.
///
/// Byte-ordered (`PartialOrd`/`Ord` over the 16 hash bytes) so ordered sets
/// of project identities have one canonical deterministic order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectIdentity(pub Hash16);

impl ProjectIdentity {
    /// Fold the 16-byte project-identity hash into the `u32`
    /// project-isolation dimension carried by query-identity keys that
    /// store `project_identity: u32`.
    ///
    /// The full 16-byte hash is the workspace + tsconfig + provider-root
    /// discriminator; this is a deterministic, order-fixed fold of all 16
    /// bytes (four little-endian `u32` lanes XOR-combined) so two distinct
    /// project identities keep distinct folds with overwhelming
    /// probability while keeping the key field a compact `u32`.
    #[must_use]
    pub fn fold_u32(self) -> u32 {
        let b = self.0;
        let lane = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        lane(0) ^ lane(4) ^ lane(8) ^ lane(12)
    }
}

// ── AugmentationTargetKey / AugmentationTargetKind / AugmentationPopulation ──

/// Kind of augmentation target.
///
/// Distinguishes external specifiers (`declare module "vue" {}`),
/// resolved relative paths (`declare module "./local" {}` resolved
/// against the augmenter), wildcard ambients (`declare module "*.css" {}`),
/// and the global block (`declare global {}`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AugmentationTargetKind {
    /// `declare module "vue" {}` — bare specifier resolved through the
    /// project's module resolver under the resolve env.
    ExternalSpecifier(InternedSpecifier),
    /// `declare module "./local" {}` — relative path resolved against
    /// the augmenter's own canonical.
    ResolvedRelativeCanonical(Arc<str>),
    /// `declare module "*.css" {}` — wildcard ambient module pattern.
    WildcardAmbient(InternedGlobPattern),
    /// `declare global { ... }` — augments the global scope.
    GlobalAugmentation,
}

/// Population identity for an [`AugmentationTargetKey`]: which artifact set
/// the content-addressed augmentation index was scanned over.
///
/// A `Base` index scans only base artifacts; a `Session` index scans the
/// session's overlay (non-base) artifacts unioned with base. The `Session`
/// discriminant carries the overlay-set CONTENT fingerprint (the session
/// view's `fingerprint()`) — not a raw session id — so this stays a
/// content-addressed identity dimension, never a live session handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AugmentationPopulation {
    /// Base resolve-domain population — base artifacts only.
    Base,
    /// Session-overlay population, keyed by the overlay-set content
    /// fingerprint.
    Session(u64),
}

/// Inverse-lookup key for the augmentation index.
///
/// Carries the resolve-domain dimensions (`project_identity`,
/// `resolve_env_hash`, `lib_env_hash`) so the same syntactic specifier
/// `"vue"` in two projects under different envs produces two distinct
/// keys. Project isolation prevents cross-project poisoning.
///
/// R21 scoping rule: this key carries `lib_env_hash` because module
/// augmentations live inside libs / ambient corpora — a lib update CAN
/// change which augmenters are visible.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AugmentationTargetKey {
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub population: AugmentationPopulation,
    pub target: AugmentationTargetKind,
}

// ── AugmenterEntry / AugmenterSet ──

/// One augmenter file's contribution identity inside an [`AugmenterSet`].
///
/// Carries the **exact** [`FileArtifactKey`](crate::source::artifact_key::FileArtifactKey) of the augmenter artifact
/// scanned at index-population time — the full content-addressed
/// identity, not just the canonical id. The augmentation-stitching
/// semantic augmentation stitcher re-fetches the augmenter's `.augmentations` through
/// the artifact store's exact-key read keyed by this exact key — never
/// a content-agnostic canonical-only scan, which (with lazy cache
/// invalidation) could surface a different content version of the
/// augmenter than the one the fingerprint was computed over.
///
/// The captured key can itself go stale: the augmenter-set fingerprint
/// folds over `parse_stable_hash` (the decl skeleton), so a member-body
/// edit that leaves the skeleton intact reparses the augmenter under a
/// new `FileArtifactKey` (new `content_hash`) WITHOUT moving the
/// fingerprint — the cached `AugmenterSet` is not invalidated and this
/// `artifact_key` keeps pointing at the drained pre-edit version. The
/// stitch consumer self-heals that exact-key miss by re-deriving the
/// augmenter's current key from the scheduler-authoritative content
/// hash and writing the refreshed key back here.
#[derive(Debug, Clone)]
pub struct AugmenterEntry {
    /// Exact content-addressed key of the augmenter artifact.
    pub artifact_key: crate::source::artifact_key::FileArtifactKey,
    /// `parse_stable_hash` of the augmenter artifact — the structural
    /// hash that the augmenter-set fingerprint folds in (R29).
    pub parse_stable_hash: Hash16,
}

impl AugmenterEntry {
    /// The augmenter file's canonical id.
    #[must_use]
    pub fn canonical(&self) -> &Arc<str> {
        &self.artifact_key.canonical
    }
}

/// The set of augmenter files that contribute to a given
/// [`AugmentationTargetKey`], sorted by `(augmenter_canonical,
/// augmenter_parse_stable_hash)`.
#[derive(Debug, Clone)]
pub struct AugmenterSet {
    /// Per-augmenter contribution identities, sorted by
    /// `(canonical, parse_stable_hash)`.
    pub entries: smallvec::SmallVec<[AugmenterEntry; 2]>,
    /// Cached `stable_hash(entries)` — the basis of
    /// `ModuleAugmentationIndexShape`.
    pub fingerprint: Hash16,
}
