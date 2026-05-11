//! Stage 1 — `FileArtifactStore`: the canonical per-file artifact cache.
//!
//! `FileArtifactStore` replaces the legacy `IndexedReadyDb` as the
//! authoritative post-parse cache. It is **content-addressed**: keys carry
//! the file's canonical path AND its `content_hash` AND the
//! `parse_env_hash` (R5, R6). A change to ANY of these produces a new key;
//! the old entry stays around (memory permitting) so concurrent overlay
//! sessions reading different versions never poison each other.
//!
//! ## What lives here
//!
//! - [`FileArtifactKey`] — `(canonical, content_hash, parse_env_hash, parser_version)`.
//! - [`FileArtifacts`] — the per-file payload: `IndexedReady`, `FileFacts`,
//!   `ParsedEdges`, `parse_stable_hash`, `augmentations`.
//! - [`AugmentationTargetKey`] / [`AugmenterSet`] — the inverse-lookup index
//!   for module augmentations (R29).
//! - [`FileFacts`] — placeholder; per-file fact registry payload (populated
//!   at Stage 3).
//! - [`ParsedEdges`] — content-addressed import-edge facts published per
//!   file version (populated at Stage 9 when the workspace-edge map becomes
//!   idempotent on the env-hash quintuple).
//! - [`ModuleAugmentationFact`] — per-file syntactic augmentation fact
//!   (populated at Stage 3; Stage 1 leaves the vector empty).
//!
//! ## Invariants
//!
//! - **Content-addressed (R5, R6):** the key carries every dimension that
//!   meaningfully changes the cached value. Two project envs reading the
//!   same canonical at the same content hash but different `parse_env_hash`
//!   produce distinct entries that coexist.
//! - **Never invalidated:** entries age out via memory-bound reachability
//!   sweeps only (Stage 9 / R22). There is no `invalidate_canonical` on
//!   `FileArtifactStore`.
//! - **`parse_stable_hash`:** an alpha-normalised structural hash over the
//!   file's post-shallow-analysis decl skeleton (names, kinds, member name
//!   lists, scope structure). Invariant under cosmetic edits (whitespace,
//!   comments, JSDoc, generic param rename). Used at Stage 3 as the keying
//!   dimension for `MemberSemanticFactStore` so cosmetic edits do NOT
//!   recompute semantic facts.
//! - **`augmentation_index` populated lazily at Stage 6c:** Stage 1 owns
//!   the skeleton + accessor API; population happens when the resolver
//!   stitches augmentations into `EffectiveExportSet`.
//!
//! See `/type-cache-architecture` skill for the full rule set.

use std::sync::Arc;

use dashmap::DashMap;
use smallvec::SmallVec;
use verter_semantic::analysis::Hash16;

use crate::project_type_store::IndexedReady;

// ── Project identity wrapper ──

/// Thin newtype around the 16-byte `project_identity` value produced by
/// `IdeProjectConfig::project_identity()`.
///
/// Used as a key dimension on [`AugmentationTargetKey`] to keep
/// augmentation entries from one project from poisoning a sibling
/// project under the same syntactic specifier (Codex P0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectIdentity(pub Hash16);

// ── Interned strings ──

/// An interned module-specifier string (e.g. `"vue"`, `"./local"`,
/// `"*.css"`). Stage 1 wraps an `Arc<str>` so the type is movable into
/// data structures without back-references; later stages can swap in a
/// crate-wide interner if profiling shows hot-path duplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedSpecifier(pub Arc<str>);

impl From<&str> for InternedSpecifier {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

/// An interned symbol name (export name, member name, etc.). Stage 1
/// stores by `Arc<str>`; Stage 3 may promote to a numeric handle.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedName(pub Arc<str>);

impl From<&str> for InternedName {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

/// An interned wildcard pattern (e.g. `"*.css"`). Distinguished from
/// [`InternedSpecifier`] so a future Stage 6c can store the pre-compiled
/// glob alongside.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedGlobPattern(pub Arc<str>);

impl From<&str> for InternedGlobPattern {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

// ── Symbol space ──

/// The TypeScript / Verter symbol space a binding occupies. A `class Foo`
/// declaration occupies BOTH `Type` and `Value` and emits two distinct
/// facts (R11) — `BothTypeValue` is forbidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolSpace {
    Type,
    Value,
    Namespace,
}

// ── FileArtifactKey ──

/// Cache key for [`FileArtifacts`].
///
/// Keys are content-addressed (R5, R6): identity is the conjunction of
/// `canonical`, `content_hash`, `parse_env_hash`, and `parser_version`.
/// Two project envs reading the same canonical at the same `content_hash`
/// but different `parse_env_hash` coexist; the cache returns the matching
/// entry for the caller's env.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileArtifactKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub parser_version: u32,
}

// ── FileFacts placeholder ──

/// Per-file fact registry payload.
///
/// Stage 1 placeholder: holds an empty registry. Stage 3 populates this
/// with the parse-domain facts (`Export`, `LocalDecl`, `MemberShape`,
/// `MemberPresence`, `SyntacticExportSet`, `MacroSurface`, `TemplateRoot`,
/// `ImportRef`, `SyntacticReexportRef`, `ModuleAugmentation`) emitted
/// during shallow analysis (R10–R16, R28, R29).
///
/// The empty struct is shaped so Stage 3 can swap in fields without an
/// API change: every consumer that wants to insert a [`FileArtifacts`]
/// passes a `FileFacts` it constructed, and the store stores it
/// verbatim.
#[derive(Debug, Default, Clone)]
pub struct FileFacts {
    // Stage 3 populates a FactKey-indexed registry here. Intentionally
    // empty in Stage 1 — Stage 3 is the producer.
    _stage1_placeholder: (),
}

impl FileFacts {
    /// Construct an empty fact registry. Stage 3 will replace with a
    /// builder + a populated registry.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

// ── ParsedEdges ──

/// Content-addressed parsed import-edge facts for a single file version.
///
/// Stage 1 placeholder: holds the per-file import-route table reference
/// that today lives on `IndexedReady.import_routes`. Stage 9 makes the
/// workspace-edge map idempotent on the env-hash quintuple, at which
/// point this type holds the authoritative content-addressed edge facts
/// directly.
///
/// For now, [`ParsedEdges::empty()`] is the canonical empty payload;
/// [`FileArtifacts::with_indexed`] uses it.
#[derive(Debug, Default, Clone)]
pub struct ParsedEdges {
    _stage1_placeholder: (),
}

impl ParsedEdges {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

// ── ModuleAugmentationFact ──

/// A single `declare module "<specifier>" { ... }` block emitted by the
/// parser during shallow analysis.
///
/// Stage 1 defines the type; Stage 3 populates it from the shallow walk.
/// Stage 6c stitches the augmenting declarations into the consumer's
/// `EffectiveExportSet` for that specifier.
///
/// Fields:
///
/// - `specifier` — the syntactic specifier inside `declare module "X" {}`.
/// - `augmented_name` — the name of an augmented binding inside the block
///   (one fact per binding). For `declare global { interface Window {} }`,
///   `augmented_name` is `"Window"` and `space = Type`.
/// - `space` — which symbol space the augmented binding occupies.
/// - `augmented_member_shape_fingerprint` — alpha-normalised fingerprint
///   over the augmenting block's member set; used by Stage 6c to detect
///   when an augmenter's contribution to the effective surface changes
///   without changing the augmenter set itself.
#[derive(Debug, Clone)]
pub struct ModuleAugmentationFact {
    pub specifier: InternedSpecifier,
    pub augmented_name: InternedName,
    pub space: SymbolSpace,
    pub augmented_member_shape_fingerprint: Hash16,
}

// ── AugmentationTargetKey / AugmentationTargetKind ──

/// Kind of augmentation target.
///
/// Distinguishes external specifiers (`declare module "vue" {}`),
/// resolved relative paths (`declare module "./local" {}` resolved
/// against the augmenter), wildcard ambients (`declare module "*.css" {}`),
/// and the global block (`declare global {}`). The four kinds use
/// disjoint resolution rules; the `target` field of
/// [`AugmentationTargetKey`] is keyed by this enum so consumers can
/// dispatch on the resolved kind. Resolved Codex-P0.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// Inverse-lookup key for the augmentation index.
///
/// Carries the resolve-domain dimensions (`project_identity`,
/// `resolve_env_hash`, `lib_env_hash`) so the same syntactic specifier
/// `"vue"` in two projects under different envs produces two distinct
/// keys. Project isolation prevents cross-project poisoning (Codex P0.1).
///
/// R21 scoping rule: this key carries `lib_env_hash` because module
/// augmentations live inside libs / ambient corpora — a lib update CAN
/// change which augmenters are visible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AugmentationTargetKey {
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub target: AugmentationTargetKind,
}

// ── AugmenterSet ──

/// The set of augmenter files that contribute to a given
/// [`AugmentationTargetKey`], sorted by `(augmenter_canonical,
/// augmenter_parse_stable_hash)`.
///
/// Each entry is one augmenter. The `fingerprint` field is the cached
/// stable hash of the sorted entry list and is the basis of the
/// `ModuleAugmentationIndexShape` fact (G1) — adding or removing an
/// augmenter changes the fingerprint and so invalidates downstream
/// `EffectiveExportSet` consumers.
#[derive(Debug, Clone)]
pub struct AugmenterSet {
    /// `(augmenter_canonical, augmenter_parse_stable_hash)` pairs, sorted.
    pub entries: SmallVec<[(Arc<str>, Hash16); 2]>,
    /// Cached `stable_hash(entries)` — the basis of
    /// `ModuleAugmentationIndexShape`.
    pub fingerprint: Hash16,
}

// ── FileArtifacts ──

/// The per-file payload stored under [`FileArtifactKey`].
///
/// Owns the canonical `IndexedReady` artifact along with the placeholder
/// fact-registry / parsed-edges / augmentation containers that Stage 3 +
/// Stage 9 will populate.
///
/// Field provenance:
///
/// - `indexed` — the existing canonical `IndexedReady` artifact. Stage 1
///   migrates every `IndexedReadyDb` consumer to read through here.
/// - `facts` — Stage 3 populates from the shallow analysis pass.
/// - `parsed_edges` — Stage 9 populates from the workspace-edge map.
/// - `parse_stable_hash` — Stage 1 computes from the post-shallow-analysis
///   decl skeleton.
/// - `augmentations` — Stage 3 populates from `declare module {}` blocks.
#[derive(Debug, Clone)]
pub struct FileArtifacts {
    pub indexed: Arc<IndexedReady>,
    pub facts: Arc<FileFacts>,
    pub parsed_edges: Arc<ParsedEdges>,
    pub parse_stable_hash: Hash16,
    pub augmentations: Arc<Vec<ModuleAugmentationFact>>,
}

impl FileArtifacts {
    /// Construct a `FileArtifacts` carrying only an `IndexedReady`. The
    /// remaining payload defaults to empty placeholders; Stage 3 + Stage 9
    /// fill them in when they emit their respective facts.
    ///
    /// `parse_stable_hash` is computed from `indexed`'s shallow-analysis
    /// decl skeleton.
    #[must_use]
    pub fn with_indexed(indexed: Arc<IndexedReady>) -> Self {
        let parse_stable_hash = crate::parse_stable_hash::compute_parse_stable_hash(&indexed);
        Self {
            indexed,
            facts: Arc::new(FileFacts::empty()),
            parsed_edges: Arc::new(ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(Vec::new()),
        }
    }
}

// ── FileArtifactStore ──

/// The per-host content-addressed file-artifact cache.
///
/// Stage 1 retires `IndexedReadyDb` and lifts the canonical artifact into
/// [`FileArtifacts`]; this store owns the inserted entries plus the
/// inverse lookup index for module augmentations.
///
/// Concurrency: backed by `DashMap`. Inserts may concurrently produce the
/// same key under different content hashes — the resulting entries
/// coexist (different keys); a re-insert under the same key replaces.
/// Per R22, eviction is memory-bound only — there is no
/// `invalidate_canonical`.
#[derive(Debug)]
pub struct FileArtifactStore {
    /// Per-(canonical, content_hash, parse_env_hash, parser_version)
    /// payloads. Keys with the same canonical but different other
    /// dimensions coexist.
    artifacts: DashMap<FileArtifactKey, Arc<FileArtifacts>>,
    /// Inverse-lookup index for module augmentations. Populated at
    /// Stage 6c — see plan §R29. Stage 1 ships the empty skeleton +
    /// accessor API.
    augmentation_index: DashMap<AugmentationTargetKey, Arc<AugmenterSet>>,
}

impl Default for FileArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FileArtifactStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            artifacts: DashMap::new(),
            augmentation_index: DashMap::new(),
        }
    }

    /// Strict lookup by full key. Returns `Some` only if every key
    /// dimension matches.
    #[must_use]
    pub fn get(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        self.artifacts.get(key).map(|v| v.clone())
    }

    /// Look up the latest entry for `canonical` regardless of the other
    /// key dimensions.
    ///
    /// Used by callers that want "any cached artifact for this file"
    /// without re-deriving the env hashes. Returns `None` if no entry
    /// exists. If multiple entries exist (concurrent envs), one is
    /// returned non-deterministically — callers that need a specific env
    /// MUST use [`Self::get`] with a full key.
    #[must_use]
    pub fn get_any(&self, canonical: &str) -> Option<Arc<FileArtifacts>> {
        for entry in self.artifacts.iter() {
            if entry.key().canonical.as_ref() == canonical {
                return Some(entry.value().clone());
            }
        }
        None
    }

    /// Insert (or replace) the payload for `key`. Returns the previous
    /// entry if one existed under the same key (different versions under
    /// other key dimensions stay in place).
    pub fn insert(
        &self,
        key: FileArtifactKey,
        artifacts: Arc<FileArtifacts>,
    ) -> Option<Arc<FileArtifacts>> {
        self.artifacts.insert(key, artifacts)
    }

    /// Remove the entry for `key`. Returns the previous entry if one
    /// existed.
    ///
    /// Per R22, removal is memory-bound only. Stage 7 retires the
    /// public `evict_canonical` surface; for now, removal is wired into
    /// the existing `ProjectTypeStore::evict_canonical` cascade.
    pub fn remove(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        self.artifacts.remove(key).map(|(_, v)| v)
    }

    /// Drain every entry whose canonical matches `canonical_id`,
    /// regardless of other key dimensions.
    ///
    /// Used by the legacy `ProjectTypeStore::evict_canonical` cascade
    /// until Stage 7 retires that surface (R3).
    pub fn remove_canonical(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        self.artifacts.retain(|key, _| {
            if key.canonical.as_ref() == canonical_id {
                removed += 1;
                false
            } else {
                true
            }
        });
        removed
    }

    /// Number of live entries across all keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    /// `true` iff the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    /// Snapshot every key, for diagnostics / reachability sweeps.
    #[must_use]
    pub fn keys(&self) -> Vec<FileArtifactKey> {
        self.artifacts
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Snapshot every `(key, payload)` pair.
    #[must_use]
    pub fn snapshot_all(&self) -> Vec<(FileArtifactKey, Arc<FileArtifacts>)> {
        self.artifacts
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    // ── Augmentation index API (populated at Stage 6c — see plan §R29) ──

    /// Look up the [`AugmenterSet`] for an [`AugmentationTargetKey`].
    ///
    /// Stage 1 ships the empty skeleton. Stage 6c populates the index on
    /// first miss by scanning [`FileArtifacts::augmentations`] across
    /// loaded files.
    #[must_use]
    pub fn get_augmenter_set(&self, key: &AugmentationTargetKey) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index.get(key).map(|v| v.clone())
    }

    /// Install (or replace) the augmenter set under `key`. Used by
    /// Stage 6c during index population.
    pub fn populate_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index.insert(key, set)
    }

    /// Drop every entry from the augmentation index. Used by
    /// reachability sweeps + tests.
    pub fn clear_augmentation_index(&self) {
        self.augmentation_index.clear();
    }

    /// Number of entries in the augmentation index. Used by counter
    /// telemetry (`module_augmentation_index_size`).
    #[must_use]
    pub fn augmentation_index_len(&self) -> usize {
        self.augmentation_index.len()
    }
}

#[cfg(test)]
#[path = "file_artifact_store_tests.rs"]
mod file_artifact_store_tests;
