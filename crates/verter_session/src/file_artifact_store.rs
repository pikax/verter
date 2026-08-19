//! `FileArtifactStore`: the canonical per-file artifact cache.
//!
//! `FileArtifactStore` is the **authoritative** post-parse cache. It replaces
//! the legacy `FileArtifactStore` type as the single per-file storage layer on
//! [`crate::project_type_store::ProjectTypeStore`].
//!
//! The store is **content-addressed**: keys carry the file's canonical path
//! AND its `content_hash` AND the `parse_env_hash` (R5, R6). Concurrent
//! overlay sessions reading different versions never poison each other.
//!
//! ## What lives here
//!
//! - [`FileArtifactKey`] — exact source, parse, environment, and build identity.
//! - [`FileArtifacts`] — the per-file payload: `IndexedReady`, `FileFacts`,
//!   `parse_stable_hash`, `augmentations`.
//! - [`AugmentationTargetKey`] / [`AugmenterSet`] — the inverse-lookup index
//!   for module augmentations (R29).
//! - [`FileFacts`] — placeholder; per-file fact registry payload (populated
//!   by the fact-emission walk).
//! - [`ModuleAugmentationFact`] — per-file syntactic augmentation fact
//!   (populated by the fact-emission walk; left empty until then).
//!
//! ## Two API surfaces
//!
//! The store exposes both:
//!
//! 1. **Canonical-keyed legacy surface** — matches the retired
//!    `FileArtifactStore` shape (`get(canonical, hash)`, `insert(canonical,
//!    indexed)`, etc.) returning `Arc<IndexedReady>` directly. Existing
//!    callers see no signature break across the rename.
//! 2. **`FileArtifactKey`-keyed canonical surface** — `get_artifacts(&key)`,
//!    `insert_artifacts(key, artifacts)`, etc. Returns `Arc<FileArtifacts>`,
//!    the new payload type with `IndexedReady` + facts + parsed edges +
//!    `parse_stable_hash` + augmentations. New code (later stages) uses
//!    this surface.
//!
//! Both surfaces share the same backing `DashMap<FileArtifactKey,
//! Arc<FileArtifacts>>`. The legacy surface synthesises a default
//! [`FileArtifactKey`] from `(canonical, indexed.whole_hash)`; later stages
//! plumb the full parse and environment identity through.
//!
//! ## Invariants
//!
//! - **Content-addressed (R5, R6):** the key carries every dimension that
//!   meaningfully changes the cached value.
//! - **Eviction is memory-bound (R22):** there is no `invalidate_canonical`
//!   on `FileArtifactStore`. The existing `ProjectTypeStore::evict_canonical`
//!   cascade calls `remove_canonical(canonical_id)` to drain all versions
//!   under a canonical (this path is the canonical-keyed legacy retirement target).
//! - **`parse_stable_hash`:** alpha-normalised structural hash over the
//!   file's post-shallow-analysis decl skeleton. Invariant under cosmetic
//!   edits.
//! - **`augmentation_index` populated lazily on first augmentation-sensitive query:** This module owns
//!   the skeleton + accessor API.
//!
//! See `/type-cache-architecture` skill for the full rule set.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::mapref::entry::Entry as CanonicalKeysEntry;
use dashmap::mapref::entry::Entry as AugmentationIndexEntry;
use dashmap::DashMap;
use smallvec::SmallVec;
use verter_language::FileLanguage;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::registry as fact_registry;
use verter_type_expr::TopLevelOwnerId;

use crate::project_type_store::IndexedReady;
use crate::resolver_core::bracketed_generation::BracketedGeneration;

// The fact-registry types live in `verter_semantic` so the registry can
// reference them without a back-edge on `verter_session`. We re-import
// them here so existing callers continue to see them under the
// `verter_session::file_artifact_store::*` paths they already use
// (no `pub use as` shimming — the types are identical, not renamed).
pub use verter_semantic::facts::registry::{
    InternedGlobPattern, InternedName, InternedSpecifier, SymbolSpace,
};

// ── Project identity wrapper ──

/// Thin newtype around the 16-byte `project_identity` value produced by
/// `IdeProjectConfig::project_identity()`.
///
/// Used as a key dimension on [`AugmentationTargetKey`] to keep
/// augmentation entries from one project from poisoning a sibling
/// project under the same syntactic specifier.
///
/// Byte-ordered (`PartialOrd`/`Ord` over the 16 hash bytes) so ordered
/// sets of project identities — e.g. the members of a
/// [`ReferenceComponent`](crate::external_ts::ReferenceComponent) — have
/// one canonical deterministic order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectIdentity(pub Hash16);

impl ProjectIdentity {
    /// Fold the 16-byte project-identity hash into the `u32`
    /// project-isolation dimension carried by query-identity keys that
    /// store `project_identity: u32` (the
    /// [`ResolvedDeclSlotIdentity`](crate::semantic_query::ResolvedDeclSlotIdentity)
    /// slot, `ApparentTypeContext`, `TemplateLiteralReduceContext`, …).
    ///
    /// The full 16-byte hash is the workspace + tsconfig + provider-root
    /// discriminator; this is a deterministic, order-fixed fold of all 16
    /// bytes (four little-endian `u32` lanes XOR-combined) so two distinct
    /// project identities keep distinct folds with overwhelming
    /// probability while keeping the key field a compact `u32`. The fold
    /// is the SINGLE conversion point — callers building a
    /// slot from `host_view_project_identity_for(..)` route through here
    /// rather than re-deriving a fold inline.
    #[must_use]
    pub fn fold_u32(self) -> u32 {
        let b = self.0;
        let lane = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        lane(0) ^ lane(4) ^ lane(8) ^ lane(12)
    }
}

// ── FileArtifactKey ──

/// Cache key for [`FileArtifacts`].
///
/// Keys are content-addressed (R5, R6): identity is the conjunction of
/// `canonical`, `content_hash`, `parse_env_hash`, `parse_key`, and
/// `file_language_id`. Two project envs reading the same canonical at
/// the same `content_hash` but different `parse_env_hash` coexist; the
/// cache returns the matching entry for the caller's env.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileArtifactKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    /// Exact source-bytes, language, compatibility-domain, and syntax-profile identity.
    pub parse_key: verter_language::ParseKey,
    /// Session-private derived-artifact shape identity.
    pub build_toolchain_fingerprint: crate::build_toolchain_fingerprint::BuildToolchainFingerprint,
    /// The file's [`FileLanguage`] row — the PER-FILE classification
    /// dimension of artifact identity (R21 scoping: nothing
    /// capability-shaped enters the global `parse_env_hash`).
    ///
    /// Exact readers take this row from the scheduler/runtime source
    /// authority. Exact writers take the retained row from
    /// [`IndexedReady::file_language`]. Path classification is only a
    /// pre-runtime fallback for synthetic tests and genuinely overlay-only
    /// canonicals.
    pub file_language_id: FileLanguage,
}

impl FileArtifactKey {
    /// Builds the exact key for an already-materialized artifact.
    pub(crate) fn for_indexed(
        canonical: Arc<str>,
        indexed: &IndexedReady,
        parse_env_hash: Hash16,
    ) -> Self {
        Self::for_source_identity(
            canonical,
            indexed.whole_hash,
            indexed.raw_source.as_ref(),
            indexed.file_language.clone(),
            indexed.framework_parse.as_deref(),
            parse_env_hash,
        )
        .expect("IndexedReady retains a compatible runtime language and parse artifact")
    }

    /// Builds an exact key from the source-stage identity that produced an artifact.
    pub(crate) fn for_source_identity(
        canonical: Arc<str>,
        content_hash: Hash16,
        source: &str,
        file_language_id: FileLanguage,
        framework_parse: Option<&verter_compiler::framework_common::FrameworkParseArtifact>,
        parse_env_hash: Hash16,
    ) -> Option<Self> {
        let parse_key = match framework_parse {
            Some(artifact) => {
                if artifact.adapter_id() != file_language_id.adapter_id()?
                    || Some(artifact.language_id()) != file_language_id.carrier_language_id()
                {
                    return None;
                }
                artifact.parse_key().clone()
            }
            None => {
                verter_language::default_parse_identity_for(source, &file_language_id)
                    .ok()?
                    .1
            }
        };
        Some(Self {
            canonical,
            content_hash,
            parse_env_hash,
            parse_key,
            build_toolchain_fingerprint:
                crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
            file_language_id,
        })
    }

    /// Extension-derived language for explicitly synthetic, source-less test
    /// keys. Production exact identity always comes from runtime authority.
    #[cfg(any(test, feature = "test-support"))]
    pub fn synthetic_file_language_for_test(canonical: &str) -> FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution()
    }

    /// Test-only constructor for a base-shaped synthetic key.
    ///
    /// A session-view overlay materialiser
    /// ([`crate::VerterHost::materialize_overlay_indexed_ready_with_view`])
    /// can resolve a relative import to an overlay-only helper that the
    /// base workspace cannot see — so the overlay's `IndexedReady`
    /// carries session-specific import routes. When the overlay source
    /// bytes are identical to the base file, the overlay's content hash
    /// equals the base hash, and a base-shaped key for the overlay
    /// would collide with the base artifact's key: a base read would
    /// observe the overlay's session routes, or the overlay read would
    /// silently get the base routes. Byte-identical overlays are the
    /// common case (every opened-but-unmodified file in an LSP session).
    ///
    /// Used by `tests/cases/g_misc0/eviction_policy.rs` and similar integration
    /// tests that need to construct multiple distinct
    /// `FileArtifactKey` variants for the same canonical to
    /// exercise the per-canonical retention sweep + the
    /// promotion-aware LRU floor. The production `pub(crate)`
    /// surface is unchanged; this `pub fn` exists only inside
    /// `#[cfg(any(test, feature = "test-support"))]` so production
    /// builds carry no public exposure of the base constructor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn base_for_test(canonical: Arc<str>, content_hash: Hash16) -> Self {
        let language = Self::synthetic_file_language_for_test(&canonical);
        let parse_key = verter_language::default_parse_identity_for("", &language)
            .expect("test canonical has a supported parse identity")
            .1;
        Self {
            canonical,
            content_hash,
            parse_env_hash: BASE_PARSE_ENV_HASH,
            parse_key,
            build_toolchain_fingerprint:
                crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
            file_language_id: language,
        }
    }

    /// Test-only constructor for an overlay-shaped synthetic key.
    #[cfg(any(test, feature = "test-support"))]
    pub fn overlay_scoped_for_test(
        canonical: Arc<str>,
        content_hash: Hash16,
        discriminator: Hash16,
    ) -> Self {
        let language = Self::synthetic_file_language_for_test(&canonical);
        let parse_key = verter_language::default_parse_identity_for("", &language)
            .expect("test canonical has a supported parse identity")
            .1;
        Self {
            canonical,
            content_hash,
            parse_env_hash: discriminator,
            parse_key,
            build_toolchain_fingerprint:
                crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
            file_language_id: language,
        }
    }

    /// `true` when this key has the base-artifact identity: the base
    /// parse-environment sentinel and current build-toolchain fingerprint.
    ///
    /// A non-base key carries a session-overlay **discriminator** in
    /// the `parse_env_hash` dimension — its
    /// `IndexedReady` can hold session-specific import routes resolved
    /// against an overlay-only helper the base workspace cannot see.
    ///
    /// The store's **base canonical-wide reads**
    /// ([`FileArtifactStore::get_any`], [`FileArtifactStore::get_artifacts_any`]
    /// — via their canonical→keys index candidates — and the
    /// [`FileArtifactStore::snapshot_all`] scan) filter on
    /// this predicate so a base `HostView` / `HostStoreView` reader can
    /// never observe an overlay-scoped artifact and derive base cache
    /// keys / route facts from another session's routes. A session-view
    /// reader reaches its overlay artifact through the exact-key /
    /// view-aware accessors ([`FileArtifactStore::get_overlay_scoped`],
    /// `OverlayArtifactIdentity::lookup_overlay_artifacts`) instead —
    /// they key on the full `FileArtifactKey` including the
    /// discriminator. Lifecycle / removal
    /// scans ([`FileArtifactStore::remove`],
    /// [`FileArtifactStore::remove_canonical`]) do NOT filter on this —
    /// an eviction must drain every key for a canonical, overlay-scoped
    /// keys included.
    #[must_use]
    pub(crate) fn is_base(&self) -> bool {
        self.parse_env_hash == BASE_PARSE_ENV_HASH
            && self.build_toolchain_fingerprint
                == crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
    }
}

/// `parse_env_hash` sentinel marking a BASE artifact key
/// used by the canonical-keyed base surface
/// before later stages plumb the real env hash through every call site.
/// An overlay-scoped key carries a non-zero session discriminator in
/// this dimension instead, so it can never alias a base key.
pub const BASE_PARSE_ENV_HASH: Hash16 = [0u8; 16];

// ── FileFacts ──

/// Per-file fact registry payload.
///
/// Owns the parse-domain `FactRegistry` populated by the shallow walk
/// during file-artifact construction (R10–R16, R28, R29). Consumers
/// read parse-domain facts via [`FileFacts::registry`] / `lookup`.
///
/// Parse-time emission populates ONLY header-derived facts:
/// `MemberShape`, `MemberPresence`, `SyntacticExportSet`, `ImportRef`,
/// `SyntacticReexportRef`, `ExportAlias`, `ModuleAugmentation`. The
/// body-sensitive `Export` / `LocalDecl` facts are NOT emitted here —
/// publishing lowers zero declaration bodies, so they compute lazily on
/// first observation through the artifact's declaration-body memo and
/// are served via [`FileFacts::lookup_or_compute`]. The `Member` body
/// fingerprint is likewise computed lazily on first member-access query
/// and lives in `MemberSemanticFactStore` / `MemberDisplayFactStore`,
/// NOT in this registry.
///
/// Resolve-domain facts are NOT populated here — they emit from the
/// resolver / `RouteDb` producers downstream.
#[derive(Debug, Default, Clone)]
pub struct FileFacts {
    registry: fact_registry::FactRegistry,
    /// Lazy body-sensitive fact source: `Export` / `LocalDecl` body
    /// fingerprints are NOT emitted at publish (publishing lowers zero
    /// declaration bodies) — they compute on first observation through
    /// the artifact's declaration-body memo and memoize in a shared
    /// side-store (`Arc`-shared across clones, so a `StoreView`
    /// snapshot's copy serves the same lazily computed facts).
    /// EXCLUDED from equality — the eager registry is the artifact's
    /// fact identity.
    lazy: Option<crate::fact_emission::LazyBodyFactSource>,
}

impl PartialEq for FileFacts {
    fn eq(&self, other: &Self) -> bool {
        self.registry == other.registry
    }
}

impl Eq for FileFacts {}

impl FileFacts {
    /// Construct an empty fact registry. Used by tests + legacy
    /// constructors that bypass parse-time emission.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Construct a populated `FileFacts` from a fact registry.
    #[must_use]
    pub fn from_registry(registry: fact_registry::FactRegistry) -> Self {
        Self {
            registry,
            lazy: None,
        }
    }

    /// Construct a populated `FileFacts` carrying the lazy
    /// body-sensitive fact source (the production emission path).
    #[must_use]
    pub(crate) fn from_registry_with_lazy(
        registry: fact_registry::FactRegistry,
        lazy: crate::fact_emission::LazyBodyFactSource,
    ) -> Self {
        Self {
            registry,
            lazy: Some(lazy),
        }
    }

    /// Look up a fact, computing body-sensitive `Export` / `LocalDecl`
    /// facts on demand through the lazy declaration-body path when the
    /// eager registry misses. Eager facts answer without lowering;
    /// a body-sensitive miss lowers exactly the named declaration.
    #[must_use]
    pub fn lookup_or_compute(&self, key: &fact_registry::FactKey) -> Option<fact_registry::Fact> {
        if let Some(fact) = self.lookup(key) {
            return Some(fact.clone());
        }
        self.lazy.as_ref()?.compute(key)
    }

    /// Borrow the underlying registry. O(1) per-key lookup via
    /// `registry().get(&key)`.
    #[must_use]
    pub fn registry(&self) -> &fact_registry::FactRegistry {
        &self.registry
    }

    /// Borrow the registry mutably — used by the parse-time
    /// producer during construction.
    pub fn registry_mut(&mut self) -> &mut fact_registry::FactRegistry {
        &mut self.registry
    }

    /// O(1) per-key lookup. `None` is an invalidation observation —
    /// the binding was removed or never existed.
    #[must_use]
    pub fn lookup(&self, key: &fact_registry::FactKey) -> Option<&fact_registry::Fact> {
        self.registry.get(key)
    }

    /// Number of parse-domain facts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// `true` if no facts have been emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    /// The cached `SyntacticExportSet` fact (if parse-time
    /// emission produced one).
    #[must_use]
    pub fn syntactic_export_set(&self) -> Option<&fact_registry::Fact> {
        self.registry.syntactic_export_set.as_ref()
    }
}

// ── ModuleAugmentationFact ──

/// A single `declare module "<specifier>" { ... }` block emitted by the
/// parser during shallow analysis.
///
/// The type is defined here; the shallow walk populates it.
/// Augmenting declarations are stitched into the consumer's merged
/// declaration surface for that specifier.
///
/// Fields:
///
/// - `specifier` — the syntactic specifier inside `declare module "X" {}`.
/// - `owner` — the lexical top-level owner that authored the declaration.
/// - `augmented_name` — the name of an augmented binding inside the block.
/// - `space` — which symbol space the augmented binding occupies.
/// - `augmented_member_shape_fingerprint` — alpha-normalised fingerprint
///   over the augmenting block's member set; used to detect
///   when an augmenter's contribution to the effective surface changes
///   without changing the augmenter set itself.
#[derive(Debug, Clone)]
pub struct ModuleAugmentationFact {
    pub specifier: InternedSpecifier,
    pub owner: TopLevelOwnerId,
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
/// and the global block (`declare global {}`).
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
/// keys. Project isolation prevents cross-project poisoning.
///
/// R21 scoping rule: this key carries `lib_env_hash` because module
/// augmentations live inside libs / ambient corpora — a lib update CAN
/// change which augmenters are visible.
///
/// The `population` dimension keeps a session-overlay augmenter set isolated
/// from the base set: a session that overlays a `declare module` block sees
/// the overlay's augmenters unioned with base under [`AugmentationPopulation::Session`],
/// while base reads stay on [`AugmentationPopulation::Base`] — overlay
/// augmenters never poison the base index, and project + env isolation prevents
/// cross-project poisoning.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AugmentationTargetKey {
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub population: AugmentationPopulation,
    pub target: AugmentationTargetKind,
}

/// Population identity for an [`AugmentationTargetKey`]: which artifact set the
/// CONTENT-ADDRESSED augmentation index was scanned over.
///
/// A `Base` index scans only base ([`FileArtifactKey::is_base`]) artifacts; a
/// `Session` index scans the session's overlay (non-base) artifacts UNIONED
/// with base. The `Session` discriminant carries the overlay-set CONTENT
/// fingerprint ([`crate::session_view::SessionView::fingerprint`], derived once
/// through [`crate::session_view::augmentation_population_for_view`]) — NOT a
/// raw session id. This index is a content-addressed compute cache, so the
/// fingerprint IS part of its content view identity: it keeps two sessions
/// (and the base) on distinct augmenter sets AND makes the slot self-invalidate
/// when overlay content/membership changes (a new fingerprint → a fresh scan).
/// Overlay results are NEVER written into a `Base`-keyed entry.
///
/// This is the CONTENT-ADDRESSED population: the overlay-set fingerprint IS
/// the index's content view identity, so a base entry can never satisfy a
/// session lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AugmentationPopulation {
    /// Base resolve-domain population — base artifacts only.
    Base,
    /// Session-overlay population, keyed by the overlay-set content
    /// fingerprint ([`crate::session_view::SessionView::fingerprint`]).
    Session(u64),
}

// ── AugmenterEntry / AugmenterSet ──

/// One augmenter file's contribution identity inside an [`AugmenterSet`].
///
/// Carries the **exact** [`FileArtifactKey`] of the augmenter artifact
/// scanned at index-population time — the full content-addressed
/// identity, not just the canonical id. The augmentation-stitching
/// semantic augmentation stitcher re-fetches the augmenter's `.augmentations` through
/// [`FileArtifactStore::get_artifacts`] keyed by this exact key — never
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
    pub artifact_key: FileArtifactKey,
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

/// An owned snapshot of one base artifact's augmenter-relevant fields,
/// captured off the `self.artifacts` DashMap so the augmenter match
/// (and any resolver it invokes) runs after every shard guard is
/// released. See [`FileArtifactStore::collect_base_augmenter_candidates`].
struct AugmenterCandidate {
    /// Exact content-addressed key of the candidate augmenter artifact.
    artifact_key: FileArtifactKey,
    /// The candidate's canonical id (also reachable via `artifact_key`,
    /// kept alongside to avoid re-borrowing through the key on the hot
    /// match loop).
    canonical: Arc<str>,
    /// `parse_stable_hash` folded into the augmenter-set fingerprint.
    parse_stable_hash: Hash16,
    /// The candidate's augmentation facts. Cloned `Arc` — a cheap
    /// refcount bump, not a deep copy.
    augmentations: Arc<Vec<ModuleAugmentationFact>>,
}

/// The set of augmenter files that contribute to a given
/// [`AugmentationTargetKey`], sorted by `(augmenter_canonical,
/// augmenter_parse_stable_hash)`.
#[derive(Debug, Clone)]
pub struct AugmenterSet {
    /// Per-augmenter contribution identities, sorted by
    /// `(canonical, parse_stable_hash)`.
    pub entries: SmallVec<[AugmenterEntry; 2]>,
    /// Cached `stable_hash(entries)` — the basis of
    /// `ModuleAugmentationIndexShape`.
    pub fingerprint: Hash16,
}

// ── FileArtifacts ──

/// The per-file payload stored under [`FileArtifactKey`].
///
/// Owns the canonical `IndexedReady` artifact along with the per-file
/// fact-registry and augmentation containers the fact-emission walk
/// populates. Cross-file import edges are NOT stored here — they live in
/// the workspace [`EdgeStore`](verter_workspace) keyed by
/// [`verter_workspace::ParsedEdge`].
#[derive(Debug, Clone)]
pub struct FileArtifacts {
    pub indexed: Arc<IndexedReady>,
    pub facts: Arc<FileFacts>,
    pub parse_stable_hash: Hash16,
    pub augmentations: Arc<Vec<ModuleAugmentationFact>>,
}

impl FileArtifacts {
    /// Construct a `FileArtifacts` carrying only an `IndexedReady`.
    ///
    /// **Parse-time fact emission runs here** — the constructed
    /// `FileFacts` is populated with the HEADER-derived parse-domain
    /// `FactRegistry` (`MemberShape`, `MemberPresence`,
    /// `SyntacticExportSet`, `ImportRef`, `SyntacticReexportRef`,
    /// `ExportAlias`, `ModuleAugmentation`) by
    /// [`crate::fact_emission::emit_parse_facts`]; publishing lowers
    /// zero declaration bodies, so the body-sensitive `Export` /
    /// `LocalDecl` facts are NOT emitted here — they compute lazily on
    /// first observation (served via [`FileFacts::lookup_or_compute`]).
    /// The per-file augmentation list is populated alongside the facts.
    /// The cross-project `augmentation_index` on [`FileArtifactStore`]
    /// is NOT touched here — it is populated lazily on first
    /// augmentation-sensitive query.
    #[must_use]
    pub fn with_indexed(indexed: Arc<IndexedReady>) -> Self {
        let parse_stable_hash = crate::parse_stable_hash::compute_parse_stable_hash(&indexed);
        let emission = crate::fact_emission::emit_parse_facts(&indexed);
        Self {
            indexed,
            facts: Arc::new(emission.facts),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }
    }
}

/// `true` when `prev` and `next` are indistinguishable in every dimension a
/// base [`crate::resolver_store::HostStoreView`] point lookup can observe
/// through its captured artifact root — so replacing `prev` with `next`
/// cannot change any base view answer and the
/// folded `artifact_generation` (hence the `StoreViewValidationToken`)
/// MUST NOT advance for it.
///
/// ## Soundness (no under-bump)
///
/// This is the bump-iff-actually-changed gate for an artifact REPLACE. It
/// is deliberately CONSERVATIVE: it returns `true` only when the precise
/// root-visible dimensions are bit-identical, so any real change to a
/// base-visible value still bumps the token (the mandatory no-under-bump
/// guarantee). `HostStoreView::build` captures the root in O(1); later point
/// lookups can observe these `FileArtifacts` dimensions:
///
/// - `indexed.whole_hash` — answers the root-backed content lookup.
/// - the file's route surface (`shallow_state.has_resolvable_surface()`
///   gates whether a `Route` derived fact is emitted at all, and
///   `hash_route_surface(&shallow_state)` is the fact's hash content).
/// - `facts` — answers parse-fact lookups (`FileFacts` is `PartialEq`).
/// - `indexed.parse_env_hash` — the reuse gate
///   (`indexed_surface_is_current`) is exactly parse-env equality, so an
///   artifact built under a different parse environment is a different
///   answer to "may this be served?" even when every surface hash above
///   is identical.
/// - `indexed.built_at_content_generation` — the artifact-only serving
///   gate (`artifact_only_candidate_is_fresh`) compares it against the
///   canonical's last recorded content transition, so a fresher stamp
///   can flip a canonical from excluded to included in the base view.
///
/// There is no route- or edge-currency dimension: `IndexedReady` retains
/// no resolved target, so a base view's answer for a canonical is a
/// pure function of the by-value dimensions above.
///
/// `parse_stable_hash` and `augmentations` are NOT read by the base view's
/// per-canonical point lookups (the augmentation INDEX is a separate,
/// lazily-populated structure with its own bump sites), so they are not
/// part of this comparison.
///
/// A new `FileArtifactKey` (a FRESH insert) is never compared here — a
/// fresh insert always bumps, because the canonical's snapshot value goes
/// from absent to present.
fn base_snapshot_equivalent(prev: &FileArtifacts, next: &FileArtifacts) -> bool {
    let prev_indexed = &prev.indexed;
    let next_indexed = &next.indexed;
    prev_indexed.whole_hash == next_indexed.whole_hash
        && prev_indexed.parse_env_hash == next_indexed.parse_env_hash
        && prev_indexed.built_at_content_generation == next_indexed.built_at_content_generation
        && prev_indexed.shallow_state.has_resolvable_surface()
            == next_indexed.shallow_state.has_resolvable_surface()
        && crate::resolver_store::hash_route_surface(&prev_indexed.shallow_state)
            == crate::resolver_store::hash_route_surface(&next_indexed.shallow_state)
        && prev.facts == next.facts
}

/// `true` when replacing the augmenter artifact `prev` with `next` cannot
/// change ANY augmentation-index entry the augmenter participates in — i.e.
/// it produces the IDENTICAL `AugmenterSet` contribution for every target.
///
/// Two orthogonal inputs determine that contribution, and BOTH must be
/// unchanged for a true no-op:
///
/// 1. **Which target rows the augmenter contributes to** — governed by the
///    `ModuleAugmentationFact` multiset (via [`augmenter_fact_could_contribute`]
///    / [`augmenter_matches_target`]). A retargeted specifier, an
///    added/removed augmented binding, or a changed
///    `augmented_member_shape_fingerprint` changes the multiset.
/// 2. **The fingerprint baked into each contributed row** — the row's
///    [`AugmenterSet::fingerprint`] is [`compute_augmenter_set_fingerprint`]
///    folded over each contributing augmenter's
///    `(augmenter_canonical, parse_stable_hash)`. The fact multiset is NOT a
///    fingerprint input. So an augmenter whose facts are unchanged but whose
///    `parse_stable_hash` moved (a decl-skeleton edit reparsed under a new
///    `FileArtifactKey`) still changes every contributed row's fingerprint.
///
/// The insert paths gate
/// [`FileArtifactStore::invalidate_augmentation_index_for_augmenter`] on this
/// equivalence so a byte-identical augmenter reinsert neither invalidates nor
/// bumps the base-folded `artifact_generation` (the no-op perf win), while any
/// real contribution change still invalidates the stale rows and bumps.
///
/// ## Soundness (no under-invalidation, no drift)
///
/// Equivalence is derived from the EXACT inputs that determine the contribution,
/// not a hand-picked subset, so it cannot drift loose from the fingerprint
/// definition (the [P2] under-invalidation class). The `parse_stable_hash`
/// gate is the SAME value [`compute_augmenter_set_fingerprint`] folds; the
/// augmenter canonical — the fingerprint's only other input — is identical at
/// every call site (an augmenter artifact only ever replaces itself at its own
/// canonical), so comparing `parse_stable_hash` here is exactly comparing the
/// per-augmenter fingerprint contribution. The fact compare is order-
/// INDEPENDENT (a multiset over the five fact dimensions) and CONSERVATIVE:
/// any genuine membership change makes the multisets differ. The lockstep unit
/// invariant
/// `file_artifact_store_tests::augmentation_contribution_equivalence_tracks_fingerprint_inputs`
/// pins this predicate to [`compute_augmenter_set_fingerprint`] so the two
/// definitions cannot diverge.
fn augmentation_contribution_equivalent(prev: &FileArtifacts, next: &FileArtifacts) -> bool {
    // (2) Fingerprint contribution: `parse_stable_hash` is folded into the
    // `AugmenterSet` fingerprint (with the augmenter canonical, which is fixed
    // across a self-replace). A moved hash changes every contributed row's
    // fingerprint, so it is NOT a no-op even when the facts are identical.
    if prev.parse_stable_hash != next.parse_stable_hash {
        return false;
    }
    // (1) Target membership: the `ModuleAugmentationFact` multiset governs which
    // index rows the augmenter contributes to.
    let prev_facts = prev.augmentations.as_slice();
    let next_facts = next.augmentations.as_slice();
    if prev_facts.len() != next_facts.len() {
        return false;
    }
    // Multiset compare keyed by the five fact dimensions (all `Eq + Hash`).
    // `ModuleAugmentationFact` is not `Eq`, so fold a per-fact count map and
    // confirm `next` exactly drains it.
    type FactKey = (
        InternedSpecifier,
        TopLevelOwnerId,
        InternedName,
        SymbolSpace,
        Hash16,
    );
    let key_of = |fact: &ModuleAugmentationFact| -> FactKey {
        (
            fact.specifier.clone(),
            fact.owner,
            fact.augmented_name.clone(),
            fact.space,
            fact.augmented_member_shape_fingerprint,
        )
    };
    let mut counts: rustc_hash::FxHashMap<FactKey, i64> = rustc_hash::FxHashMap::default();
    for fact in prev_facts {
        *counts.entry(key_of(fact)).or_insert(0) += 1;
    }
    for fact in next_facts {
        let entry = counts.entry(key_of(fact)).or_insert(0);
        *entry -= 1;
    }
    counts.values().all(|&c| c == 0)
}

// ── Membership epochs, roots, and retention leases ──

/// Number of retirements a mutation batch may accumulate before the
/// store self-triggers a physical reclamation sweep.
///
/// Retirement is LOGICAL: the version leaves the current root's
/// membership but its bytes stay retained until every live root has
/// moved past it. Without an amortised sweep the retained set would
/// grow by one version per keystroke, so the store reclaims on its own
/// schedule (it owns reclamation) rather than waiting for an external
/// GC request that may never arrive on a pure edit loop.
const RECLAIM_TRIGGER_RETIREMENTS: u64 = 64;

/// Visibility window of ONE version of one membership entry, expressed
/// in [`FileArtifactStore`] membership epochs.
///
/// A version is born at the epoch its mutation published and is
/// retired at the epoch that superseded or removed it. A root at
/// `epoch` sees the version iff `birth <= epoch < retirement`.
///
/// Retirement is a LOGICAL removal: it ends the version's visibility
/// from the CURRENT root while leaving it reachable from every root
/// captured before the retirement. Physical reclamation is a separate,
/// root-gated decision ([`FileArtifactStore::reclaim_retired_versions`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VersionSpan {
    birth: u64,
    retirement: Option<u64>,
}

impl VersionSpan {
    fn born(birth: u64) -> Self {
        Self {
            birth,
            retirement: None,
        }
    }

    /// Is this version visible from a root at `epoch`?
    fn visible_at(&self, epoch: u64) -> bool {
        self.birth <= epoch
            && match self.retirement {
                Some(retirement) => epoch < retirement,
                None => true,
            }
    }

    /// Is this version part of the CURRENT root's membership?
    fn is_live(&self) -> bool {
        self.retirement.is_none()
    }
}

/// The terminal membership epoch.
///
/// The epoch counter is monotonic and never wraps: reaching this value
/// EXHAUSTS the epoch line. A wrap would invert visibility outright (a
/// version born "after" a root would compare as born before it), so the
/// store saturates here instead and every root captured from then on
/// FAILS CLOSED — it sees no artifact, no canonical→keys membership and
/// no augmenter set, so every read through it misses and the caller
/// recomputes against current state. Correctness is preserved; only warm
/// reuse is lost. At one membership mutation per nanosecond this is
/// unreachable for ~584 years, but the ordering primitive's correctness
/// is not a function of how long it takes to break.
const EXHAUSTED_EPOCH: u64 = u64::MAX;

/// The epochs a reclamation sweep must keep reachable: every live
/// captured root, plus the STABLE epoch (the epoch the next capture will
/// take).
///
/// This is the COMPLETE reachability rule, and it is per-root, not a
/// floor. A floor ("keep everything retired above the oldest live root")
/// over-retains without bound: one stale root pins every LATER version
/// too, so an edit loop under a single pinned view grows one retained
/// version per keystroke until the process dies. A root at epoch `E`
/// needs exactly the version VISIBLE at `E` for each canonical — never
/// its successors, which `E` can no longer select.
#[derive(Debug)]
struct RetentionEpochs {
    /// The epoch the next [`FileArtifactStore::capture_root`] would
    /// take. Every future root addresses this epoch or a later one, so a
    /// version still visible here (or born after it) stays retained.
    stable: u64,
    /// Every live captured root's epoch.
    roots: SmallVec<[u64; 4]>,
}

impl RetentionEpochs {
    /// Is this version invisible from EVERY root — the current/next one
    /// and every live captured one — and therefore physically
    /// reclaimable?
    fn reclaimable(&self, span: &VersionSpan) -> bool {
        match span.retirement {
            // A live version is part of the current membership.
            None => false,
            // Retired at or below the stable epoch: no FUTURE capture can
            // see it either (captures are monotonic), so the live roots
            // are the only remaining readers.
            Some(retirement) => {
                retirement <= self.stable && !self.roots.iter().any(|&epoch| span.visible_at(epoch))
            }
        }
    }
}

/// Registry of every LIVE captured [`FileArtifactRoot`] and every
/// RESERVED-but-unapplied membership epoch.
///
/// Two responsibilities, one lock, because a capture must read both in
/// the same critical section:
///
/// 1. **Reachability.** An immutable root must both NAME state and KEEP
///    it reachable, so the store cannot decide that a retired version is
///    reclaimable without consulting the roots that still address it.
/// 2. **Capture atomicity.** A membership mutation RESERVES its epoch
///    before applying it, and only completes the reservation once the
///    application has landed. A capture therefore never names an epoch
///    whose application is still in flight — it takes the newest FULLY
///    APPLIED epoch instead. Without that, a root captured between the
///    bump and the apply reads the pre-apply world and then the
///    post-apply world for the SAME epoch: a value change under a root
///    documented as immutable.
///
/// Owned by [`FileArtifactStore`] — no other layer may decide
/// reachability.
///
/// **Lock rank: LEAF.** It is acquired with no `artifacts` /
/// `canonical_keys` / `retired_*` shard guard held, and no shard guard is
/// ever taken while it is held.
#[derive(Debug, Default)]
struct LiveRootRegistry {
    state: parking_lot::Mutex<RootRegistryState>,
}

/// [`LiveRootRegistry`]'s guarded state.
#[derive(Debug, Default)]
struct RootRegistryState {
    /// `epoch -> number of live roots captured at that epoch`. A
    /// `BTreeMap` so the oldest live root is the first key (O(log n)),
    /// never a scan.
    roots: std::collections::BTreeMap<u64, usize>,
    /// `epoch -> number of reserved-but-unapplied mutations at that
    /// epoch`. Its first key bounds the newest fully-applied epoch.
    in_flight: std::collections::BTreeMap<u64, usize>,
}

impl RootRegistryState {
    /// The newest epoch whose application has fully landed.
    ///
    /// Every epoch at or below this one is applied: epochs are reserved
    /// in increasing order, so an unapplied mutation at or below the
    /// first in-flight epoch would itself be the first in-flight epoch.
    ///
    /// Monotonic over time (the counter only grows, and the first
    /// in-flight key only moves forward), so a future capture never
    /// addresses an epoch older than the one this returns now.
    fn stable_epoch(&self, current: u64) -> u64 {
        match self.in_flight.keys().next().copied() {
            Some(oldest_in_flight) => oldest_in_flight.saturating_sub(1),
            None => current,
        }
    }
}

impl LiveRootRegistry {
    fn release(&self, epoch: u64) {
        let mut state = self.state.lock();
        if let std::collections::btree_map::Entry::Occupied(mut slot) = state.roots.entry(epoch) {
            let count = slot.get_mut();
            *count = count.saturating_sub(1);
            if *count == 0 {
                slot.remove();
            }
        }
    }

    /// Release one epoch reservation. Called only from
    /// [`EpochReservation::drop`].
    fn complete(&self, epoch: u64) {
        let mut state = self.state.lock();
        if let std::collections::btree_map::Entry::Occupied(mut slot) = state.in_flight.entry(epoch)
        {
            let count = slot.get_mut();
            *count = count.saturating_sub(1);
            if *count == 0 {
                slot.remove();
            }
        }
    }
}

/// A RESERVED membership epoch, held for exactly as long as the mutation
/// that reserved it is still applying.
///
/// While one of these is alive, [`FileArtifactStore::capture_root`] will
/// not hand out its epoch — so no root can observe a half-applied
/// membership transition. Released on drop, including on unwind, so a
/// panicking mutation cannot freeze the capture epoch forever.
struct EpochReservation<'a> {
    registry: &'a LiveRootRegistry,
    epoch: u64,
}

impl EpochReservation<'_> {
    /// The reserved epoch: the birth of what this mutation publishes and
    /// the retirement of what it supersedes.
    fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for EpochReservation<'_> {
    fn drop(&mut self) {
        self.registry.complete(self.epoch);
    }
}

/// An immutable, LEASED root of [`FileArtifactStore`] membership.
///
/// Holding a `FileArtifactRoot` does two inseparable things:
///
/// 1. it NAMES a membership epoch — every exact artifact key, every
///    canonical→keys index entry and every augmentation-index entry
///    that was live at that epoch resolves through it, regardless of
///    what the current root holds; and
/// 2. it KEEPS that state reachable — the store may not physically
///    reclaim any version this root can still see.
///
/// Identity without reachability is not a snapshot: a
/// `(canonical, content_hash)` pair alone can name an artifact the
/// store has already dropped. The lease closes that gap.
///
/// Not `Clone` by design — one value is one registration. Share it by
/// `Arc` (a `HostStoreView` does exactly that); the registration is
/// released once, when the last `Arc` drops.
pub struct FileArtifactRoot {
    epoch: u64,
    registry: Arc<LiveRootRegistry>,
}

impl FileArtifactRoot {
    /// The membership epoch this root addresses.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Was this root captured after the epoch line was EXHAUSTED?
    ///
    /// Such a root addresses no membership at all: every root-relative
    /// read through it misses, so consumers recompute instead of reading
    /// a world whose ordering can no longer be expressed. See
    /// [`EXHAUSTED_EPOCH`].
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.epoch == EXHAUSTED_EPOCH
    }
}

impl std::fmt::Debug for FileArtifactRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileArtifactRoot")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl Drop for FileArtifactRoot {
    fn drop(&mut self) {
        self.registry.release(self.epoch);
    }
}

/// One retired version of an exact artifact key, retained for the roots
/// that still see it.
struct RetiredArtifactVersion {
    span: VersionSpan,
    payload: Arc<FileArtifacts>,
}

/// One version of a canonical→keys index membership entry.
///
/// The index is versioned alongside the artifacts themselves: a root
/// enumerating a canonical's keys must see exactly the keys that were
/// live at its epoch, not the current live set.
#[derive(Clone)]
struct CanonicalKeyVersion {
    key: FileArtifactKey,
    span: VersionSpan,
}

/// One version of an augmentation-index entry.
#[derive(Clone)]
struct AugmenterVersion {
    span: VersionSpan,
    set: Arc<AugmenterSet>,
}

// ── StoredArtifact ──

/// The `self.artifacts` map value: the shared payload plus the
/// entry-embedded warm-read bookkeeping.
///
/// The per-entry hit counter and last-access tick live INSIDE the map
/// value so a warm hit bumps them through the already-held entry
/// reference — no side-map key hash, no `FileArtifactKey` clone, no
/// `Arc<str>` allocation per hit (side maps keyed by `FileArtifactKey`
/// / canonical cost exactly that on every warm read). The global
/// [`FileArtifactStore::access_tick`] counter remains the single
/// monotonic tick source.
///
/// Lifecycle: the counters are entry-owned, so they drop with the
/// entry — an evicted key can never leak a stale hit count into a
/// later same-key insert — and a REPLACED value starts cold again (a
/// fresh value has not yet proven warm demand; the promotion-aware LRU
/// floor treats it as new).
struct StoredArtifact {
    payload: Arc<FileArtifacts>,
    /// Membership epoch this version was published at. A root captured
    /// BEFORE the publication (`root.epoch < birth_epoch`) does not see
    /// this entry even though it occupies the live slot — the root's
    /// world predates it.
    birth_epoch: u64,
    /// Warm-hit counter; saturates at `u32::MAX` so long-lived hot
    /// entries do not overflow. Consumed by the LRU floor's promotion
    /// predicate ([`FileArtifactStore::evict_lru_promoted`]).
    hits: AtomicU32,
    /// Monotonically-maxed last-access tick (from
    /// [`FileArtifactStore::access_tick`]). Consumed by the LRU
    /// floor's recency ordering.
    last_access_tick: AtomicU64,
}

impl StoredArtifact {
    fn new(payload: Arc<FileArtifacts>, tick: u64, birth_epoch: u64) -> Self {
        Self {
            payload,
            birth_epoch,
            hits: AtomicU32::new(0),
            last_access_tick: AtomicU64::new(tick),
        }
    }

    /// Bump the warm-hit counter (saturating at `u32::MAX`).
    fn record_hit(&self) {
        let _ = self
            .hits
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |hits| {
                (hits != u32::MAX).then_some(hits + 1)
            });
    }

    /// Stamp the last-access tick (monotonic — a stale racing stamp
    /// never regresses a fresher one).
    fn record_access(&self, tick: u64) {
        self.last_access_tick.fetch_max(tick, Ordering::Relaxed);
    }

    fn hit_count(&self) -> u32 {
        self.hits.load(Ordering::Relaxed)
    }

    fn access_tick(&self) -> u64 {
        self.last_access_tick.load(Ordering::Relaxed)
    }
}

// ── FileArtifactStore ──

/// The per-host content-addressed file-artifact cache.
///
/// Replaces the retired `FileArtifactStore` type as the authoritative
/// per-file storage layer. The same struct serves both the legacy
/// canonical-keyed surface (`get(canonical, hash) -> Arc<IndexedReady>`)
/// and the new content-addressed surface (`get_artifacts(&key) ->
/// Arc<FileArtifacts>`).
pub struct FileArtifactStore {
    /// Per-(canonical, content_hash, parse_env_hash, parse_key)
    /// payloads, each carried in a [`StoredArtifact`] alongside its
    /// entry-embedded warm-read bookkeeping. Keys with the same
    /// canonical but different other dimensions coexist.
    artifacts: DashMap<FileArtifactKey, StoredArtifact>,
    /// Canonical-id → live [`FileArtifactKey`]s inverse index over
    /// `self.artifacts`.
    ///
    /// Read fast-path for the per-canonical read surfaces
    /// ([`Self::get_any`], [`Self::get_artifacts_any`],
    /// [`Self::get_artifacts_for_content`]): they resolve the canonical's
    /// candidate keys here and then read `self.artifacts` by EXACT key,
    /// instead of scanning the whole store per lookup (that scan is
    /// O(total live entries) per read and dominated warm read profiles).
    ///
    /// The value is the canonical's VERSIONED key membership — there is
    /// NO at-most-one-base-key invariant to lean on: the legacy
    /// [`Self::insert`] retires prior versions (so it leaves one LIVE
    /// base key per canonical), but the content-addressed
    /// [`Self::insert_artifacts`] deliberately lets multiple base-shape
    /// variants of one canonical coexist (the per-canonical retention
    /// sweep exists precisely to cap them), and overlay-scoped keys
    /// share the canonical besides. Each entry carries a
    /// [`VersionSpan`], so a reader at the current root filters to
    /// [`VersionSpan::is_live`] while a reader at a captured
    /// [`FileArtifactRoot`] filters to [`VersionSpan::visible_at`].
    ///
    /// Coherence discipline (mutation-site maintained, reader-tolerant):
    ///
    /// - Every `self.artifacts` insert routes through
    ///   [`Self::insert_artifact_entry`] and every removal through the
    ///   sole chokepoint ([`Self::retire_artifact_keys`]); BOTH hold the
    ///   canonical's index-slot entry guard ACROSS the paired map+index
    ///   mutation, so same-canonical mutation pairs serialize on the
    ///   slot guard and the index can never end up permanently missing
    ///   a live map entry (the harmful direction).
    /// - Lock order is always index-slot guard → `self.artifacts` shard
    ///   (map locks are only taken inside a call while the slot guard
    ///   is held, never the reverse), so the pairing cannot deadlock.
    /// - The whole-store schema reset clears this index FIRST, then the
    ///   map: an insert racing the reset leaves at worst a DANGLING
    ///   index key (listed here, absent from the map). Readers tolerate
    ///   dangling keys by construction — they try every candidate key
    ///   and skip ones whose exact read misses — so a dangling key
    ///   costs one extra exact lookup, never a wrong result.
    canonical_keys: DashMap<Arc<str>, SmallVec<[CanonicalKeyVersion; 2]>>,
    /// Retired (logically removed / superseded) artifact versions,
    /// retained for the [`FileArtifactRoot`]s that still address them.
    ///
    /// A key's live version lives in `self.artifacts`; every version
    /// that key ever displaced or that was retired out of the current
    /// root lives here, each with a closed [`VersionSpan`]. Spans for
    /// one key are disjoint, so at most one version in a chain is
    /// visible from any given epoch.
    ///
    /// The chain is drained ONLY by
    /// [`Self::reclaim_retired_versions`], and only for versions no
    /// live root can see.
    retired_artifacts: DashMap<FileArtifactKey, SmallVec<[RetiredArtifactVersion; 1]>>,
    /// Retired augmentation-index versions — same contract as
    /// `retired_artifacts`, for the augmentation-index membership
    /// domain.
    retired_augmenters: DashMap<AugmentationTargetKey, SmallVec<[AugmenterVersion; 1]>>,
    /// Monotonic MEMBERSHIP epoch — the identity of the current
    /// [`FileArtifactRoot`].
    ///
    /// Advanced by every mutation that changes membership in any of the
    /// three versioned domains (exact artifact keys, the canonical→keys
    /// index, augmentation-index keys). The mutation RESERVES the new
    /// epoch ([`FileArtifactStore::reserve_membership_epoch`]), stamps it
    /// as the birth of what it publishes and the retirement of what it
    /// supersedes, applies the change, and only then releases the
    /// reservation.
    ///
    /// Ordering contract: a capture never names a reserved-but-unapplied
    /// epoch, so a root addresses only fully-applied membership. A root
    /// captured at epoch `E` therefore sees the pre-`E+1` world for its
    /// whole life — never the pre-apply world on one read and the
    /// post-apply world on the next. Monotonic and saturating: it never
    /// wraps (see [`EXHAUSTED_EPOCH`]).
    ///
    /// Distinct from `artifact_generation`: that counts changes to the
    /// values a `HostStoreView` snapshots BY VALUE and is a cache
    /// VALIDITY dimension; this ADDRESSES a snapshot and is never a
    /// validity oracle.
    membership_epoch: AtomicU64,
    /// Every live captured root. The store is the sole authority on
    /// reachability — see [`LiveRootRegistry`].
    live_roots: Arc<LiveRootRegistry>,
    /// Retirements accumulated since the last reclamation sweep; drives
    /// the amortised self-triggered sweep at
    /// [`RECLAIM_TRIGGER_RETIREMENTS`].
    retirements_since_reclaim: AtomicU64,
    /// Global monotonic access-tick source for the per-entry
    /// [`StoredArtifact::last_access_tick`] stamps consumed by
    /// [`Self::evict_lru_promoted`] under explicit memory pressure.
    access_tick: AtomicU64,
    /// Live entry counter.
    live_counter: Arc<AtomicU64>,
    /// Stale-sweep counter.
    stale_sweeps: Arc<AtomicU64>,
    /// Monotonic artifact-publication generation.
    ///
    /// Bumped on every mutation that changes the per-canonical
    /// `IndexedReady` / `FileFacts` / derived-hash content a
    /// `HostStoreView` snapshots BY VALUE: artifact insert / replace /
    /// evict / GC and every augmentation-index populate / refresh /
    /// invalidate / clear. This is the dimension the
    /// `StoreViewValidationToken` folds so a manager-cached base view is
    /// invalidated when a lazy `ensure_indexed_ready_serve` publication lands
    /// after the snapshot was built (the lazy publish does NOT bump
    /// `store_view_epoch`). Without this the cached snapshot's
    /// `file_facts` / `derived_hashes` maps go stale and warm-hit
    /// validation false-misses — a steady-state warm-cache regression.
    /// The lazy-publication burst during a cold compute is bounded, so
    /// the cache rebuilds once and then stays warm.
    ///
    /// Distinct from `live_counter` (which counts net live entries, not
    /// mutations) and `stale_sweeps` (replacements / evictions only):
    /// neither bumps on an augmentation-index mutation, and a content
    /// REPLACE leaves `live_counter` unchanged while still changing the
    /// snapshotted value.
    artifact_generation: Arc<AtomicU64>,
    /// Semantic generation of the `RouteSurface` compaction domain.
    ///
    /// Deliberately SEPARATE from [`Self::artifact_generation`], which
    /// also advances for first-time index materialisation, for a
    /// same-fingerprint self-heal republish and for cache-only
    /// repopulation. None of those is a semantic validity flip, and all
    /// of them happen INSIDE an active fact tracer on the same thread —
    /// so a route-surface clock that inherited that shape would refuse
    /// its own consumers' cold work.
    ///
    /// It advances for changes to the augmentation WORLD: a published
    /// augmenter set whose fingerprint differs from the one it replaces,
    /// and an artifact retirement that removes index contributors.
    ///
    /// BRACKETED for the same reason the semantic-imports counter is —
    /// the index mutates while readers are mid-scope, so a post-mutation
    /// increment would let a scope pair the new index with the old
    /// generation.
    route_surface_generation: BracketedGeneration,
    /// Cache-cluster schema version this store was constructed under.
    schema_version: u32,
    /// Inverse-lookup index for module augmentations. Populated at
    /// Populated lazily by the augmentation-stitching pass on the first
    /// inverse lookup for a target.
    /// See `/type-cache-architecture` skill for the populator semantics.
    augmentation_index: DashMap<AugmentationTargetKey, AugmenterVersion>,
    /// Test-only host-level audit hook.
    #[cfg(test)]
    test_audit_hook: parking_lot::Mutex<Option<Arc<crate::host_test_audit::HostTestAuditState>>>,
}

impl std::fmt::Debug for FileArtifactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileArtifactStore")
            .field("artifacts_len", &self.artifacts.len())
            .field("augmentation_index_len", &self.augmentation_index.len())
            .field("schema_version", &self.schema_version)
            .finish_non_exhaustive()
    }
}

impl Default for FileArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FileArtifactStore {
    #[must_use]
    pub fn new() -> Self {
        Self::with_counters(Default::default(), Default::default())
    }

    pub(crate) fn with_counters(live: Arc<AtomicU64>, stale: Arc<AtomicU64>) -> Self {
        Self::with_counters_and_schema_version(
            live,
            stale,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the
    /// store. Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counters_and_schema_version(
            Default::default(),
            Default::default(),
            schema_version,
        )
    }

    fn with_counters_and_schema_version(
        live: Arc<AtomicU64>,
        stale: Arc<AtomicU64>,
        schema_version: u32,
    ) -> Self {
        Self {
            artifacts: DashMap::new(),
            canonical_keys: DashMap::new(),
            retired_artifacts: DashMap::new(),
            retired_augmenters: DashMap::new(),
            // Epoch 0 is the empty store: every publication stamps a
            // birth of at least 1, so a root at epoch 0 sees nothing —
            // which is exactly the membership an empty store has.
            membership_epoch: AtomicU64::new(0),
            live_roots: Arc::new(LiveRootRegistry::default()),
            retirements_since_reclaim: AtomicU64::new(0),
            access_tick: AtomicU64::new(0),
            live_counter: live,
            stale_sweeps: stale,
            artifact_generation: Arc::new(AtomicU64::new(0)),
            route_surface_generation: BracketedGeneration::default(),
            schema_version,
            augmentation_index: DashMap::new(),
            #[cfg(test)]
            test_audit_hook: parking_lot::Mutex::new(None),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // MVCC membership: epochs, root capture, root-relative reads, and
    // root-gated physical reclamation.
    // ──────────────────────────────────────────────────────────────────

    /// The current root's membership epoch.
    #[must_use]
    pub fn membership_epoch(&self) -> u64 {
        self.membership_epoch.load(Ordering::Acquire)
    }

    /// Reserve the next membership epoch for a mutation that is about to
    /// apply.
    ///
    /// The returned [`EpochReservation`] must stay alive until the
    /// mutation has fully applied: while it lives, no capture can name
    /// the reserved epoch, which is what makes the transition atomic with
    /// respect to [`Self::capture_root`]. See the `membership_epoch`
    /// field docs.
    ///
    /// Saturates at [`EXHAUSTED_EPOCH`] rather than wrapping.
    fn reserve_membership_epoch(&self) -> EpochReservation<'_> {
        let mut state = self.live_roots.state.lock();
        let epoch = self
            .membership_epoch
            .load(Ordering::Acquire)
            .checked_add(1)
            .unwrap_or(EXHAUSTED_EPOCH);
        self.membership_epoch.store(epoch, Ordering::Release);
        *state.in_flight.entry(epoch).or_insert(0) += 1;
        drop(state);
        EpochReservation {
            registry: &self.live_roots,
            epoch,
        }
    }

    /// Capture an immutable, LEASED root of the store's current
    /// membership.
    ///
    /// O(1): one mutex acquisition, one scalar read, one counter bump —
    /// independent of the number of artifacts, canonicals or
    /// augmentation targets. The returned root both names the epoch and
    /// keeps every version visible at that epoch physically reachable
    /// until it drops.
    ///
    /// The epoch is read UNDER the registry lock, and
    /// [`Self::reclaim_retired_versions`] computes its retention epochs
    /// under the same lock. That total order is what makes a capture
    /// racing a sweep safe: a capture that registers first is counted in
    /// the retention set; a capture that registers after the sweep
    /// necessarily reads the stable epoch the sweep used or a later one,
    /// and every version the sweep dropped was already invisible from
    /// that epoch onward.
    ///
    /// The captured epoch is the newest FULLY APPLIED one — a mutation
    /// that has reserved its epoch but not finished applying it holds
    /// the capture back to its predecessor, so no root can observe a
    /// half-applied transition (and then observe the other half through
    /// the same, supposedly immutable, root).
    #[must_use]
    pub fn capture_root(&self) -> Arc<FileArtifactRoot> {
        let epoch = {
            let mut state = self.live_roots.state.lock();
            let epoch = state.stable_epoch(self.membership_epoch.load(Ordering::Acquire));
            *state.roots.entry(epoch).or_insert(0) += 1;
            epoch
        };
        Arc::new(FileArtifactRoot {
            epoch,
            registry: Arc::clone(&self.live_roots),
        })
    }

    /// Does `root` address THIS store's membership?
    ///
    /// A root minted by a different store names an unrelated epoch
    /// line; reading through it would silently answer from the wrong
    /// world. Every root-relative accessor fails closed on a foreign
    /// root rather than guessing.
    fn owns_root(&self, root: &FileArtifactRoot) -> bool {
        Arc::ptr_eq(&root.registry, &self.live_roots)
    }

    /// THE artifact-visibility function: the version of `key` visible
    /// from a root at `epoch`, or `None` if the key had no version
    /// then.
    ///
    /// Current-epoch reads (`Self::get`, `Self::get_artifacts`, …) are
    /// the provable specialization of this function at
    /// `epoch == membership_epoch()`: a live entry's birth is always at
    /// or below the current epoch, and a retired version's retirement
    /// is too — so the retired chain can never hold a version visible
    /// at the current epoch, and probing it would be dead work. The
    /// equivalence is pinned by
    /// `file_artifact_store_tests::current_epoch_read_equals_root_relative_read_at_current_epoch`.
    fn artifact_version_at(&self, key: &FileArtifactKey, epoch: u64) -> Option<Arc<FileArtifacts>> {
        if let Some(entry) = self.artifacts.get(key) {
            let stored = entry.value();
            if stored.birth_epoch <= epoch {
                return Some(Arc::clone(&stored.payload));
            }
        }
        self.retired_artifacts.get(key).and_then(|chain| {
            chain
                .value()
                .iter()
                .find(|version| version.span.visible_at(epoch))
                .map(|version| Arc::clone(&version.payload))
        })
    }

    /// Root-relative exact artifact read. The payload stays reachable
    /// for as long as `root` lives, even after the current root has
    /// superseded or evicted it.
    #[must_use]
    pub fn artifacts_at_root(
        &self,
        root: &FileArtifactRoot,
        key: &FileArtifactKey,
    ) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        if !self.owns_root(root) || root.is_exhausted() {
            return None;
        }
        self.artifact_version_at(key, root.epoch)
    }

    /// Root-relative `IndexedReady` read — [`Self::artifacts_at_root`]
    /// projected onto the indexed artifact.
    ///
    /// Test-only: production reads the whole [`FileArtifacts`] through
    /// [`Self::artifacts_at_root`] and projects what it needs, so a
    /// public projection with no production caller would be API surface
    /// pretending to be capability.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn indexed_at_root(
        &self,
        root: &FileArtifactRoot,
        key: &FileArtifactKey,
    ) -> Option<Arc<IndexedReady>> {
        self.artifacts_at_root(root, key)
            .map(|artifacts| Arc::clone(&artifacts.indexed))
    }

    /// Root-relative canonical→keys enumeration: exactly the keys that
    /// were live for `canonical` at `root`'s epoch, deduplicated (a key
    /// that was retired and later re-published appears once).
    #[must_use]
    pub fn artifact_keys_at_root(
        &self,
        root: &FileArtifactRoot,
        canonical: &str,
    ) -> SmallVec<[FileArtifactKey; 2]> {
        let mut keys: SmallVec<[FileArtifactKey; 2]> = SmallVec::new();
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return keys;
        }
        if !self.owns_root(root) || root.is_exhausted() {
            return keys;
        }
        if let Some(slot) = self.canonical_keys.get(canonical) {
            for version in slot
                .value()
                .iter()
                .filter(|version| version.span.visible_at(root.epoch))
            {
                if !keys.contains(&version.key) {
                    keys.push(version.key.clone());
                }
            }
        }
        keys
    }

    /// Root-relative augmentation-index read.
    #[must_use]
    pub fn augmenter_set_at_root(
        &self,
        root: &FileArtifactRoot,
        key: &AugmentationTargetKey,
    ) -> Option<Arc<AugmenterSet>> {
        if !self.owns_root(root) || root.is_exhausted() {
            return None;
        }
        let epoch = root.epoch;
        if let Some(entry) = self.augmentation_index.get(key) {
            if entry.value().span.birth <= epoch {
                return Some(Arc::clone(&entry.value().set));
            }
        }
        self.retired_augmenters.get(key).and_then(|chain| {
            chain
                .value()
                .iter()
                .find(|version| version.span.visible_at(epoch))
                .map(|version| Arc::clone(&version.set))
        })
    }

    /// The epochs a sweep must keep reachable: every live captured root,
    /// plus the epoch the next capture would take.
    ///
    /// Computed under the live-root registry lock together with the
    /// current epoch, so it is totally ordered against
    /// [`Self::capture_root`].
    fn retention_epochs(&self) -> RetentionEpochs {
        let state = self.live_roots.state.lock();
        let stable = state.stable_epoch(self.membership_epoch.load(Ordering::Acquire));
        RetentionEpochs {
            stable,
            roots: state.roots.keys().copied().collect(),
        }
    }

    /// Physically reclaim every retired version that no root can reach.
    ///
    /// **The complete reachability rule:** a version is reclaimable
    /// only when it is invisible from (a) the current artifact root AND
    /// (b) every live root captured by a `HostStoreView` / session /
    /// request. Both halves are read here, from state this store owns.
    /// No caller may substitute its own reachability judgement — a
    /// consumer such as `ProjectTypeStore` may REQUEST a sweep, and
    /// that request carries no reachability information.
    ///
    /// Reachability is decided PER ROOT, never by a floor. "Retired
    /// after the oldest live root" is not the same predicate as
    /// "visible from some live root": under a floor, one stale root pins
    /// every version born after it as well, so a pinned view turns an
    /// edit loop into unbounded growth. A root at epoch `E` selects
    /// exactly ONE version per membership entry — the one visible at `E`
    /// — and everything between that version and the current one is
    /// unreachable from every root.
    ///
    /// Returns the number of physically reclaimed versions across all
    /// three versioned membership domains.
    pub fn reclaim_retired_versions(&self) -> usize {
        let retention = self.retention_epochs();
        self.retirements_since_reclaim.store(0, Ordering::Relaxed);
        let mut reclaimed = 0usize;
        self.retired_artifacts.retain(|_key, chain| {
            let before = chain.len();
            chain.retain(|version| !retention.reclaimable(&version.span));
            reclaimed += before - chain.len();
            !chain.is_empty()
        });
        self.retired_augmenters.retain(|_key, chain| {
            let before = chain.len();
            chain.retain(|version| !retention.reclaimable(&version.span));
            reclaimed += before - chain.len();
            !chain.is_empty()
        });
        // The canonical→keys index is versioned membership too: a
        // retired index version stays enumerable from the roots that
        // still see it, and is reclaimed under the same rule. A slot that
        // still has a live key can never empty here (its live version
        // has no retirement), so an emptied slot is genuinely gone.
        self.canonical_keys.retain(|_canonical, slot| {
            let before = slot.len();
            slot.retain(|version| !retention.reclaimable(&version.span));
            reclaimed += before - slot.len();
            !slot.is_empty()
        });
        reclaimed
    }

    /// Amortised self-triggered reclamation. Retirement is logical, so
    /// without this the retained set grows by one version per publish
    /// on a pure edit loop.
    fn note_retirements(&self, count: usize) {
        if count == 0 {
            return;
        }
        let before = self
            .retirements_since_reclaim
            .fetch_add(count as u64, Ordering::Relaxed);
        if before + count as u64 >= RECLAIM_TRIGGER_RETIREMENTS {
            let _ = self.reclaim_retired_versions();
        }
    }

    /// Number of retired-but-still-retained versions across all three
    /// versioned membership domains — the measurable half of the
    /// memory bound `current retained working set + versions reachable
    /// from live view roots`.
    #[must_use]
    pub fn retained_retired_version_count(&self) -> usize {
        let artifacts: usize = self
            .retired_artifacts
            .iter()
            .map(|entry| entry.value().len())
            .sum();
        let augmenters: usize = self
            .retired_augmenters
            .iter()
            .map(|entry| entry.value().len())
            .sum();
        let index: usize = self
            .canonical_keys
            .iter()
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .filter(|version| !version.span.is_live())
                    .count()
            })
            .sum();
        artifacts + augmenters + index
    }

    /// Number of live captured roots currently leasing membership.
    #[must_use]
    pub fn live_root_count(&self) -> usize {
        self.live_roots.state.lock().roots.values().sum()
    }

    /// Seed the membership epoch. Test-only: the epoch line's terminal
    /// behaviour is otherwise unreachable in any finite test.
    #[cfg(any(test, feature = "test-support"))]
    pub fn seed_membership_epoch_for_test(&self, epoch: u64) {
        self.membership_epoch.store(epoch, Ordering::Release);
    }

    /// Current artifact-publication generation. Folded into the
    /// `StoreViewValidationToken` so a `HostStoreView` snapshot built
    /// before a lazy artifact publication is invalidated once the
    /// publication lands. See the `artifact_generation` field docs.
    #[must_use]
    pub fn artifact_generation(&self) -> u64 {
        self.artifact_generation.load(Ordering::Acquire)
    }

    /// Bump the artifact-publication generation. Called from every
    /// mutation that changes a `HostStoreView`-snapshotted value
    /// (artifact insert / replace / evict / GC, augmentation-index
    /// mutation).
    #[inline]
    fn bump_artifact_generation(&self) {
        self.artifact_generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Install the host-level test audit hook (legacy `FileArtifactStore`
    /// equivalent).
    #[cfg(test)]
    pub(crate) fn install_test_audit_hook(
        &self,
        state: Arc<crate::host_test_audit::HostTestAuditState>,
    ) {
        *self.test_audit_hook.lock() = Some(state);
    }

    // ──────────────────────────────────────────────────────────────────
    // Legacy `FileArtifactStore` API surface
    //
    // These methods preserve the retired type's signatures so existing
    // call sites compile across the rename. They map onto the
    // canonical-keyed base slot of the underlying `DashMap`.
    // ──────────────────────────────────────────────────────────────────

    /// Look up the indexed artifact for `canonical_id` if the cached
    /// entry matches `expected_whole_hash`. Stale entries are ignored.
    #[must_use]
    pub fn get(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
        expected_parse_key: &verter_language::ParseKey,
        expected_file_language: &FileLanguage,
    ) -> Option<Arc<IndexedReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = self.canonical_keys.get(canonical_id).and_then(|slot| {
            slot.value()
                .iter()
                .filter(|version| {
                    version.span.is_live()
                        && version.key.is_base()
                        && version.key.content_hash == expected_whole_hash
                        && version.key.parse_key == *expected_parse_key
                        && version.key.file_language_id == *expected_file_language
                })
                .find_map(|version| self.artifacts.get(&version.key))
                .map(|entry| {
                    let stored = entry.value();
                    stored.record_hit();
                    stored.record_access(self.next_access_tick());
                    Arc::clone(&stored.payload.indexed)
                })
        });
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .indexed
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .indexed
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Exact read for an explicitly synthetic empty-source base entry.
    /// Host-produced artifacts must use their runtime-authoritative full key.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_synthetic_empty_source_base_for_test(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
    ) -> Option<Arc<IndexedReady>> {
        let key = FileArtifactKey::base_for_test(Arc::from(canonical_id), expected_whole_hash);
        self.get(
            canonical_id,
            expected_whole_hash,
            &key.parse_key,
            &key.file_language_id,
        )
    }

    /// Overlay-scoped indexed lookup: returns the cached artifact for
    /// `canonical_id` keyed under its overlay discriminator
    /// (the overlay's content hash plus the session-overlay
    /// `discriminator`).
    ///
    /// This is the read counterpart of the overlay materialiser's
    /// publish. A session-view-routed reader resolves an overlay
    /// candidate through here so it never collides with the base
    /// artifact (always keyed under the base shape) — even
    /// when the overlay source is byte-identical to the base and the
    /// content hashes therefore coincide. A base read using
    /// [`Self::get`] never reaches an overlay-scoped entry; an
    /// overlay-scoped read never reaches the base entry. Stale
    /// candidates for an older content hash yield `None`.
    #[must_use]
    pub fn get_overlay_scoped(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
        discriminator: Hash16,
        expected_parse_key: &verter_language::ParseKey,
        expected_file_language: &FileLanguage,
    ) -> Option<Arc<IndexedReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = self
            .canonical_keys
            .get(canonical_id)
            .and_then(|slot| {
                slot.value()
                    .iter()
                    .filter(|version| {
                        version.span.is_live()
                            && version.key.parse_env_hash == discriminator
                            && version.key.content_hash == expected_whole_hash
                            && version.key.parse_key == *expected_parse_key
                            && version.key.file_language_id == *expected_file_language
                            && version.key.build_toolchain_fingerprint
                                == crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
                    })
                    .find_map(|version| self.artifacts.get(&version.key))
                    .map(|entry| {
                        let stored = entry.value();
                        stored.record_hit();
                        stored.record_access(self.next_access_tick());
                        Arc::clone(&stored.payload.indexed)
                    })
            });
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .indexed
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .indexed
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Exact read for an explicitly synthetic empty-source overlay entry.
    /// Host-produced overlays must rebuild their full key from the session view.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_synthetic_empty_source_overlay_for_test(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
        discriminator: Hash16,
    ) -> Option<Arc<IndexedReady>> {
        let key = FileArtifactKey::overlay_scoped_for_test(
            Arc::from(canonical_id),
            expected_whole_hash,
            discriminator,
        );
        self.get_overlay_scoped(
            canonical_id,
            expected_whole_hash,
            discriminator,
            &key.parse_key,
            &key.file_language_id,
        )
    }

    /// Look up the cached **base** artifact for `canonical_id` without
    /// hash check.
    ///
    /// This is the base canonical-wide read: it resolves the
    /// canonical's candidate keys through the canonical→keys index and
    /// filters to [`FileArtifactKey::is_base`] entries, so a
    /// session-overlay artifact published under an
    /// overlay-scoped key is never surfaced to a
    /// base reader. A session-view reader that wants its overlay
    /// artifact uses [`Self::get_overlay_scoped`] (exact key) instead.
    #[must_use]
    pub fn get_any(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let mut result: Option<Arc<IndexedReady>> = None;
        if let Some(slot) = self.canonical_keys.get(canonical_id) {
            for key in slot
                .value()
                .iter()
                .filter(|version| version.span.is_live() && version.key.is_base())
                .map(|version| &version.key)
            {
                // Exact read; a (benign) dangling index key just misses
                // and the next candidate is tried.
                if let Some(entry) = self.artifacts.get(key) {
                    let stored = entry.value();
                    stored.record_hit();
                    stored.record_access(self.next_access_tick());
                    result = Some(Arc::clone(&stored.payload.indexed));
                    break;
                }
            }
        }
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .indexed
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .indexed
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Content-pinned lookup: returns the cached artifact for
    /// `canonical_id` **only** when its stored content hash equals
    /// `expected_content_hash`; any stale candidate yields `None`.
    ///
    /// The correctness-sensitive read surface. Callers feeding a
    /// cache-validation oracle (parse-domain route-fact production,
    /// materialisation fence seeding, component-meta proof
    /// producers) MUST use this — or the host-level
    /// `current_content_pinned_indexed` wrapper that resolves the hash
    /// from the scheduler — instead of the permissive [`Self::get_any`].
    /// Reading a stale artifact as "current" defeats fact validation.
    /// Semantically identical to [`Self::get`]; the distinct name makes
    /// the content-pinning contract explicit at the call site.
    #[must_use]
    pub fn get_for_current_content(
        &self,
        canonical_id: &str,
        expected_content_hash: Hash16,
        expected_parse_key: &verter_language::ParseKey,
        expected_file_language: &FileLanguage,
    ) -> Option<Arc<IndexedReady>> {
        self.get(
            canonical_id,
            expected_content_hash,
            expected_parse_key,
            expected_file_language,
        )
    }

    /// Next global access tick — the single monotonic source for the
    /// per-entry [`StoredArtifact::last_access_tick`] stamps.
    #[inline]
    fn next_access_tick(&self) -> u64 {
        self.access_tick.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Test-only inspection of the per-entry hit counter.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hit_count(&self, key: &FileArtifactKey) -> u32 {
        self.artifacts
            .get(key)
            .map(|entry| entry.value().hit_count())
            .unwrap_or(0)
    }

    /// Snapshot every `(canonical_id, content_hash)` key in the cache.
    #[must_use]
    pub fn keys(&self) -> Vec<(Arc<str>, Hash16)> {
        self.artifacts
            .iter()
            .map(|entry| (entry.key().canonical.clone(), entry.key().content_hash))
            .collect()
    }

    /// THE sole `self.artifacts` insert combinator.
    ///
    /// Inserts the entry and mirrors `key` into the canonical→keys
    /// index (`canonical_keys`) under the canonical's index-slot write
    /// guard, so the pair is atomic with respect to every other
    /// same-canonical mutation (they all serialize on the same slot
    /// guard) and the index can never permanently miss a live map
    /// entry. Lock order: index-slot guard → `self.artifacts` shard —
    /// see the `canonical_keys` field docs for the full coherence
    /// discipline.
    /// Publishes `payload` at `key` under a freshly advanced membership
    /// epoch and RETIRES whatever version the key held before, so a root
    /// captured before this publication keeps reaching the superseded
    /// payload.
    ///
    /// Returns the superseded payload, if any.
    fn insert_artifact_entry(
        &self,
        key: FileArtifactKey,
        payload: Arc<FileArtifacts>,
        tick: u64,
    ) -> Option<Arc<FileArtifacts>> {
        let reservation = self.reserve_membership_epoch();
        let epoch = reservation.epoch();
        let mut slot = self
            .canonical_keys
            .entry(Arc::clone(&key.canonical))
            .or_default();
        // Retain the superseded version for the roots that still address
        // it — the legacy behaviour dropped it here, which is exactly how
        // a captured `(canonical, whole_hash)` came to name an artifact
        // the store had already freed — and retain it BEFORE the live
        // slot stops holding it (see [`Self::publish_retired_version`]).
        let displaced_payload = self.publish_retired_version(&key, epoch);
        if displaced_payload.is_some() {
            // Close the index membership version this publication
            // supersedes.
            for version in slot
                .iter_mut()
                .filter(|version| version.span.is_live() && version.key == key)
            {
                version.span.retirement = Some(epoch);
            }
        }
        self.artifacts
            .insert(key.clone(), StoredArtifact::new(payload, tick, epoch));
        slot.push(CanonicalKeyVersion {
            key,
            span: VersionSpan::born(epoch),
        });
        drop(slot);
        // The transition has landed: release the epoch so captures may
        // name it, BEFORE the (possibly reclaiming) retirement accounting.
        drop(reservation);
        if displaced_payload.is_some() {
            self.note_retirements(1);
        }
        displaced_payload
    }

    /// Copy `key`'s CURRENT live version into the retired chain, closed
    /// at `epoch`, and return its payload — WITHOUT removing the live
    /// entry.
    ///
    /// **Publish before retract.** A version MOVES between two maps, and
    /// a root-relative reader consults them in sequence without holding
    /// either. If the live entry were retracted first, a reader on a root
    /// captured before `epoch` would find the version in NEITHER map for
    /// the length of the writer's window and answer `None` — a world no
    /// epoch ever had, which the per-view memo then freezes (first-writer
    /// wins) into every request that view serves. Publishing first makes
    /// the transient window one in which the version is reachable from
    /// BOTH maps, and both copies are the same `Arc` describing the same
    /// span, so either read answers identically.
    ///
    /// The writer-side slot guard cannot substitute for this: readers do
    /// not take it, so it orders writers against each other, not the move
    /// against a read.
    ///
    /// Callers must hold the canonical's `canonical_keys` slot guard, so
    /// the read-then-write pair here is atomic against every other
    /// same-canonical mutation.
    fn publish_retired_version(
        &self,
        key: &FileArtifactKey,
        epoch: u64,
    ) -> Option<Arc<FileArtifacts>> {
        let (birth, payload) = {
            let entry = self.artifacts.get(key)?;
            (
                entry.value().birth_epoch,
                Arc::clone(&entry.value().payload),
            )
        };
        self.retired_artifacts
            .entry(key.clone())
            .or_default()
            .push(RetiredArtifactVersion {
                span: VersionSpan {
                    birth,
                    retirement: Some(epoch),
                },
                payload: Arc::clone(&payload),
            });
        Some(payload)
    }

    /// THE sole artifact-retirement chokepoint.
    ///
    /// Every path that removes entries from `self.artifacts` funnels
    /// through here: the public [`Self::remove`] / [`Self::remove_artifacts`]
    /// / [`Self::remove_canonical`] / [`Self::clear_all`], the legacy
    /// [`Self::insert`] prior-version drain, the internal memory-bound
    /// sweeps ([`Self::evict_lru_promoted`] +
    /// [`Self::enforce_per_canonical_retention`]) and the schema-mismatch
    /// whole-store reset.
    ///
    /// Removal here is LOGICAL: the version leaves the CURRENT root's
    /// membership under a freshly advanced epoch, and its payload moves
    /// into the retired chain where every root captured before this
    /// epoch keeps reaching it. Physical reclamation is a separate,
    /// root-gated decision ([`Self::reclaim_retired_versions`]) — no
    /// removal path may free bytes a live root can still see. That is
    /// what makes an immutable root a RETENTION LEASE rather than a
    /// bare name.
    ///
    /// Retiring the entries and invalidating the `augmentation_index`
    /// entries they contributed to is ONE inseparable operation, so it is
    /// **structurally impossible** to retire an artifact while leaving a
    /// stale [`AugmenterSet`] behind — closing the augmentation-index
    /// under-invalidation class by construction rather than by enumerating
    /// callers. Both halves share the SAME epoch, so no root can observe
    /// the artifacts retired but the index not. The static guard
    /// `artifact_removal_routes_through_single_chokepoint` pins that this
    /// method (and [`Self::drop_artifact_entry`]) is the only `self.artifacts`
    /// removal site.
    ///
    /// Counter / audit-event bookkeeping is **caller policy** — an LRU
    /// drop, a replacement drain, and a hard delete attribute
    /// `live_counter` / `stale_sweeps` / structured events differently —
    /// so this chokepoint touches none of them; it returns the retired
    /// `(key, payload)` pairs and lets the caller account for them.
    /// Per-entry warm-read bookkeeping (hit counter, access tick) is
    /// current-root state and is NOT carried into the retired chain —
    /// a re-published key starts cold, as before.
    ///
    /// Invalidation runs ONCE over the union of every retired entry's
    /// augmentation facts, after every `self.artifacts` shard guard is
    /// released (all removes complete before the index scan), preserving
    /// the no-shard-guard-across-reentrancy discipline of
    /// [`Self::invalidate_augmentation_index_for_augmenter`].
    fn retire_artifact_keys(
        &self,
        keys: &[FileArtifactKey],
    ) -> Vec<(FileArtifactKey, Arc<FileArtifacts>)> {
        if keys.is_empty() {
            return Vec::new();
        }
        let reservation = self.reserve_membership_epoch();
        let epoch = reservation.epoch();
        let mut removed: Vec<(FileArtifactKey, Arc<FileArtifacts>)> =
            Vec::with_capacity(keys.len());
        let mut removed_augmentations: Vec<ModuleAugmentationFact> = Vec::new();
        for key in keys {
            // Paired map+index retirement under the canonical's
            // index-slot write guard (same serialization discipline as
            // `insert_artifact_entry`; see the `canonical_keys` field
            // docs). The key's LIVE index version is closed
            // unconditionally — under the slot guard, a map miss means
            // the key is genuinely absent, so a (benign) dangling index
            // key is self-healed here. A canonical with no slot still
            // attempts the map remove so the map stays authoritative.
            //
            // The slot guard is held across BOTH the map removal and the
            // retired-chain push, exactly as `insert_artifact_entry`
            // holds it across the insert and the push. Releasing it in
            // between publishes a torn world: a reader on a root captured
            // before this epoch would enumerate the key (still visible in
            // the index) while both the live entry and the retired
            // version are missing, and — because the per-view memo is
            // first-writer-wins and shared — would freeze that
            // never-existed world into every request the view serves.
            // PUBLISH BEFORE RETRACT: the version reaches the retired
            // chain first and only then leaves the live map, so a
            // concurrent root-relative read finds it in BOTH rather than
            // in NEITHER. See [`Self::publish_retired_version`].
            let mut retire_live = |key: &FileArtifactKey| {
                let Some(payload) = self.publish_retired_version(key, epoch) else {
                    return;
                };
                self.artifacts.remove(key);
                removed_augmentations.extend(payload.augmentations.iter().cloned());
                removed.push((key.clone(), payload));
            };
            match self.canonical_keys.entry(Arc::clone(&key.canonical)) {
                CanonicalKeysEntry::Occupied(mut slot) => {
                    retire_live(key);
                    for version in slot
                        .get_mut()
                        .iter_mut()
                        .filter(|version| version.span.is_live() && &version.key == key)
                    {
                        version.span.retirement = Some(epoch);
                    }
                    // A slot only empties once reclamation has dropped
                    // every version it held; a retirement leaves the
                    // closed version behind for the roots that see it.
                    if slot.get().is_empty() {
                        slot.remove();
                    }
                }
                CanonicalKeysEntry::Vacant(vacant) => {
                    // No index slot for this canonical — but the vacant
                    // entry still holds the shard guard, so the removal
                    // and the retained push stay serialized against every
                    // other same-canonical mutation just as the occupied
                    // arm is. Nothing is inserted: an untracked canonical
                    // must not gain an empty slot.
                    retire_live(key);
                    drop(vacant);
                }
            }
        }
        // Demand-driven coherence: every retired augmenter retires every index
        // entry it contributed to so the next cold-rescan rebuilds without it.
        // Same epoch as the artifact retirements above — one membership
        // transition, not two.
        self.invalidate_augmentation_index_at_epoch(&removed_augmentations, epoch);
        drop(reservation);
        self.note_retirements(removed.len());
        removed
    }

    /// Single-key convenience over the [`Self::retire_artifact_keys`]
    /// chokepoint. Returns the retired payload, or `None` if `key` was
    /// absent. The augmentation-index invalidation is performed by the
    /// chokepoint — callers cannot bypass it.
    fn drop_artifact_entry(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        self.retire_artifact_keys(std::slice::from_ref(key))
            .into_iter()
            .next()
            .map(|(_, payload)| payload)
    }

    /// LRU floor — drop entries down to `min_floor` by oldest-access
    /// order.
    ///
    /// Delegates to [`Self::evict_lru_promoted`] with `promote_threshold
    /// = 0` (no promotion — pure recency LRU). Promotion-aware callers
    /// thread the configured `promote_threshold` directly through
    /// `evict_lru_promoted` to preserve hot entries.
    pub fn evict_lru(&self, min_floor: usize) {
        self.evict_lru_promoted(min_floor, 0);
    }

    /// Promotion-aware LRU floor. Entries whose per-entry hit counter
    /// is **strictly below** `promote_threshold` are considered
    /// "cold" and age out first regardless of access-tick
    /// recency; the floor's recency comparison only applies among
    /// the surviving cold pool. Hot entries (counter >=
    /// `promote_threshold`) survive unless every entry is hot, in
    /// which case the floor falls back to pure recency.
    ///
    /// R22 — memory-bound eviction. The hot/cold split
    /// is the only behavioural difference from the pure-recency
    /// [`Self::evict_lru`]; correctness still flows from
    /// fact-validation (R19).
    pub fn evict_lru_promoted(&self, min_floor: usize, promote_threshold: u32) {
        let len = self.artifacts.len();
        if len <= min_floor {
            return;
        }
        let drop_count = len - min_floor;
        // Collect (key, hit_count, tick) for every entry — both read
        // straight off the entry-embedded atomics during this single
        // iteration (no side-map lookups). The tick is per ENTRY: a
        // stale variant of a recently-read canonical ages out before
        // the variant that actually served the reads.
        let mut entries: Vec<(FileArtifactKey, u32, u64)> = self
            .artifacts
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    entry.value().hit_count(),
                    entry.value().access_tick(),
                )
            })
            .collect();
        // Partition: cold (hits < promote_threshold) first, then hot.
        // Within each partition, oldest tick first.
        entries.sort_by(|a, b| {
            let a_cold = a.1 < promote_threshold;
            let b_cold = b.1 < promote_threshold;
            match (a_cold, b_cold) {
                (true, false) => std::cmp::Ordering::Less, // cold first
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.2.cmp(&b.2), // within partition, oldest first
            }
        });
        let drop_keys: Vec<FileArtifactKey> = entries
            .into_iter()
            .take(drop_count)
            .map(|(key, _hits, _tick)| key)
            .collect();
        // Route through the single removal chokepoint: it drops the
        // entries (their embedded hit counters and access ticks go with
        // them) and invalidates the augmentation index the evicted
        // augmenters contributed to.
        let removed = self.retire_artifact_keys(&drop_keys);
        let evicted_any = !removed.is_empty();
        for _ in &removed {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        if evicted_any {
            self.bump_artifact_generation();
        }
    }

    /// Enforce per-canonical content-hash retention. Keeps at most
    /// `retention` distinct LIVE `FileArtifactKey` variants per
    /// canonical id; surplus variants are RETIRED in deterministic
    /// `content_hash` order (lowest first — see below).
    ///
    /// Setting `retention == usize::MAX` is a no-op. Setting
    /// `retention == 0` retires every variant beyond the most
    /// recently inserted (the live counter's "current generation").
    ///
    /// **The cap bounds the CURRENT root's membership, never
    /// reachability.** A fixed cap that physically discarded versions
    /// would happily free one a live [`FileArtifactRoot`] still
    /// addresses, breaking the lease the root's holder was promised.
    /// Retirement here is logical, so a pinned version survives the cap
    /// and is freed only once [`Self::reclaim_retired_versions`] finds
    /// no root can see it. The cap therefore stays a bound on the live
    /// working set, and the retained set is bounded by
    /// `live working set + versions reachable from live view roots`.
    ///
    /// Binds R22: the cap is a memory bound; correctness is still
    /// owned by fact-validation. Variant ordering uses the entry's
    /// own `content_hash` as a stable tiebreaker so test runs are
    /// deterministic across DashMap iteration order.
    pub fn enforce_per_canonical_retention(&self, retention: usize) {
        if retention == usize::MAX {
            return;
        }
        // Group keys by canonical.
        let mut by_canonical: rustc_hash::FxHashMap<Arc<str>, Vec<FileArtifactKey>> =
            rustc_hash::FxHashMap::default();
        for entry in self.artifacts.iter() {
            by_canonical
                .entry(entry.key().canonical.clone())
                .or_default()
                .push(entry.key().clone());
        }
        let mut drop_keys: Vec<FileArtifactKey> = Vec::new();
        for (_canonical, mut keys) in by_canonical {
            if keys.len() <= retention {
                continue;
            }
            // Sort by content_hash for deterministic order; we drop
            // from the front (older / lower-numbered variants).
            keys.sort_by_key(|key| key.content_hash);
            let drop_count = keys.len() - retention;
            drop_keys.extend(keys.into_iter().take(drop_count));
        }
        // Route through the single removal chokepoint: it drops the
        // entries (embedded warm-read bookkeeping goes with them) and
        // invalidates the augmentation index the evicted augmenters
        // contributed to.
        let removed = self.retire_artifact_keys(&drop_keys);
        let evicted_any = !removed.is_empty();
        for _ in &removed {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        if evicted_any {
            // Per-canonical retention dropped at least one artifact
            // version a `HostStoreView` could have snapshotted; bump the
            // generation so a pre-retention view is token-invalidated.
            // This sweep runs on every `evict_unreachable_artifacts_with_policy`
            // call, so it MUST advance the token like every other keyed
            // removal.
            self.bump_artifact_generation();
        }
    }

    /// Snapshot every live **base** entry in `(canonical, indexed)`
    /// shape (matches the legacy `FileArtifactStore` API).
    ///
    /// This is a base canonical-wide scan: it yields only
    /// [`FileArtifactKey::is_base`] entries. The `(canonical,
    /// indexed)` shape discards the key, so a consumer cannot tell a
    /// base artifact from a session-overlay one — filtering to base
    /// keys keeps legacy diagnostics off session-specific overlay routes.
    /// `HostStoreView::build` does not use this scan; it captures immutable
    /// roots in O(1). Diagnostics that need every
    /// keyed entry use [`Self::snapshot_artifacts`] (which returns the
    /// full [`FileArtifactKey`]) instead.
    #[must_use]
    pub fn snapshot_all(&self) -> Vec<(Arc<str>, Arc<IndexedReady>)> {
        self.artifacts
            .iter()
            .filter(|entry| entry.key().is_base())
            .map(|entry| {
                (
                    entry.key().canonical.clone(),
                    Arc::clone(&entry.value().payload.indexed),
                )
            })
            .collect()
    }

    /// Insert or replace the entry for `canonical_id`. Older versions for
    /// the same canonical leave the CURRENT root's membership — the
    /// legacy `FileArtifactStore` guaranteed exactly one LIVE entry per
    /// canonical regardless of content_hash, so this method preserves
    /// that semantics by retiring every other version of the same
    /// canonical before inserting the new one.
    ///
    /// Retirement here is logical. The prior version's payload stays
    /// reachable from every [`FileArtifactRoot`] captured before this
    /// publication: physically dropping it — which the legacy drain did
    /// — is precisely how a `HostStoreView` that had captured
    /// `(canonical, whole_hash)` ended up naming an artifact the store
    /// had already freed. See [`Self::retire_artifact_keys`].
    ///
    /// The new content-addressed `insert_artifacts` surface DOES allow
    /// multiple versions to coexist; callers that want that behaviour
    /// MUST route through `insert_artifacts` with a full
    /// `FileArtifactKey`.
    pub fn insert(&self, canonical_id: Arc<str>, indexed: Arc<IndexedReady>) {
        let whole_hash = indexed.whole_hash;
        let canonical_for_event = Arc::clone(&canonical_id);
        let tick = self.next_access_tick();

        // The base-visible identity for this insert is the base key at
        // the NEW content hash — the exact key a base `HostStoreView`
        // snapshots for this canonical's live content (`snapshot_all` /
        // `snapshot_file_facts_into` gate on `content_hash == live
        // whole_hash`). Compute it first so a base-equivalent replace can
        // be detected BEFORE any removal and expose NO absent window for it.
        let current_key =
            FileArtifactKey::for_indexed(Arc::clone(&canonical_id), &indexed, BASE_PARSE_ENV_HASH);
        let payload = Arc::new(FileArtifacts::with_indexed(indexed));

        // Is the new payload base-snapshot-equivalent to what already lives
        // at the current key? Compared BEFORE draining: a byte-identical
        // re-insert is a true no-op for the current key and must remain a
        // LITERAL no-op — never a remove-then-insert that exposes an absent
        // window. A base snapshot interleaving a remove-then-reinsert of the
        // current key would observe the canonical's live-content artifact as
        // momentarily ABSENT (a missing `file_facts` / `Route` fact) while
        // `artifact_generation` is unchanged (no-op → no bump), caching an
        // incomplete snapshot under the unchanged token. Leaving the
        // base-equivalent current-key entry untouched closes that gap.
        // Presence of ANY entry at the current content key, captured with
        // the same read as the equivalence probe. Discriminates a genuine
        // BUILD of this content version (no entry at the current key — a
        // fresh insert or a content-changed rebuild) from a REFRESH
        // re-insert of an already-stored version (edge-refresh materialise
        // reusing the content-addressed payload, base-equivalent no-op):
        // the audit `IndexedReadyBuilt` event below fires only for the
        // former.
        let current_key_prior_entry_present;
        let current_key_is_base_equivalent = match self.artifacts.get(&current_key) {
            Some(entry) => {
                current_key_prior_entry_present = true;
                let equivalent = base_snapshot_equivalent(&entry.value().payload, &payload);
                if equivalent {
                    // The no-op path leaves the entry untouched below;
                    // refresh its access tick here so a no-op reinsert
                    // still registers as recency for the LRU floor (the
                    // legacy insert always counts as an access).
                    entry.value().record_access(tick);
                }
                equivalent
            }
            None => {
                current_key_prior_entry_present = false;
                false
            }
        };

        // Legacy semantics: exactly one entry per canonical regardless of
        // content_hash. Drain every prior version EXCEPT the current key
        // when it is base-equivalent (left in place above). Overlay-scoped
        // prior versions are base-invisible (`snapshot_all` filters to
        // base keys), so draining them alone does NOT force a base-token
        // bump. The prior BASE (base-key) payload — captured before
        // draining — is what the bump-iff-actually-changed gate compares
        // against when the current key is NOT already present (a content
        // change replacing a different-hash base entry). The drain itself
        // routes through the single removal chokepoint, which invalidates
        // the prior versions' augmentation-index entries — a content edit
        // that RETARGETS or DROPS an augmentation must clean the PRIOR
        // target's index entry, which the new facts alone would not cover.
        //
        // The canonical's prior keys are resolved through the
        // canonical→keys index (`canonical_keys`), NOT by scanning
        // `self.artifacts`: a whole-store scan is O(total live entries)
        // per insert, so every publish into a warm host re-walked the
        // entire store. The index is maintained by the paired
        // insert/removal chokepoints and its only failure direction is a
        // DANGLING key (listed here, absent from the map), so each
        // candidate is confirmed against `self.artifacts` before it
        // counts as a prior version — that preserves the scan's exact
        // result set, which `had_prior` and `prior_base_payload` below
        // both read as "a LIVE prior entry". Lock order is the
        // documented one (index-slot guard → `self.artifacts` shard),
        // matching every other index-backed reader.
        let prior_keys: Vec<FileArtifactKey> = self
            .canonical_keys
            .get(canonical_id.as_ref())
            .map(|slot| {
                slot.value()
                    .iter()
                    // Only the CURRENT root's membership drains; already
                    // retired versions are retained for the roots that
                    // still address them and must not be re-retired.
                    .filter(|version| version.span.is_live())
                    .map(|version| &version.key)
                    // When the current key is a base-equivalent no-op we
                    // leave it in place; do NOT drain it (that would open
                    // the absent window this fix exists to close). Every
                    // OTHER prior key (stale content hashes,
                    // overlay-scoped variants) still drains.
                    .filter(|key| !(current_key_is_base_equivalent && *key == &current_key))
                    .filter(|key| self.artifacts.contains_key(*key))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let had_prior = !prior_keys.is_empty() || current_key_is_base_equivalent;
        // Capture the prior BASE (base-key) payload BEFORE draining so the
        // bump-iff-actually-changed gate can compare it against the new
        // payload (a content change replacing a different-hash base entry).
        let prior_base_payload: Option<Arc<FileArtifacts>> =
            prior_keys.iter().find(|k| k.is_base()).and_then(|k| {
                self.artifacts
                    .get(k)
                    .map(|e| Arc::clone(&e.value().payload))
            });
        // Capture the NEW artifact's augmentations before the conditional
        // insert moves the payload, so the publish-side invalidation can
        // fold them (the drain below already cleaned the prior target).
        let new_augmentations: Vec<ModuleAugmentationFact> =
            payload.augmentations.iter().cloned().collect();
        // Cheap `Arc` clone of the new payload so the no-op gate can compare the
        // full augmentation contribution (fact set AND `parse_stable_hash`, the
        // `AugmenterSet` fingerprint input) without re-fetching after the
        // conditional insert below may move `payload`.
        let payload_for_compare: Arc<FileArtifacts> = Arc::clone(&payload);
        // Capture the base-equivalent current-key entry's payload BEFORE any
        // mutation so the no-op gate can compare the augmentation contribution.
        // Only meaningful when the current key is left in place (a true no-op
        // reinsert); otherwise the current key is drained / replaced and the
        // publish-side invalidation always runs.
        let current_key_prior_payload: Option<Arc<FileArtifacts>> =
            if current_key_is_base_equivalent {
                self.artifacts
                    .get(&current_key)
                    .map(|e| Arc::clone(&e.value().payload))
            } else {
                None
            };
        // Drain every (non-base-equivalent) prior version through the single
        // removal chokepoint: it drops the entries (embedded warm-read
        // bookkeeping goes with them) and invalidates the prior versions'
        // augmentation-index entries. The chokepoint deliberately does NOT touch
        // `artifact_generation` (counter bookkeeping is caller policy) — the
        // base-folded bump for this insert is owned by the
        // `snapshot_changed` gate below so an overlay-only / stale-hash
        // drain does not churn the token.
        let _drained = self.retire_artifact_keys(&prior_keys);

        // Bump the base-folded `artifact_generation` ONLY when this insert
        // changes the canonical's base snapshot value. A base-equivalent
        // re-insert at the current key is a literal no-op (the entry was
        // left untouched) and never bumps. Otherwise: with no prior base
        // (base-key) entry the canonical's base snapshot goes absent →
        // present, which always bumps; a replace of a different-hash base
        // entry bumps unless the new payload is base-snapshot-equivalent to
        // it (R4 parity). The comparison is CONSERVATIVE: any real change to
        // a base-visible dimension still bumps (mandatory no-under-bump).
        let snapshot_changed = if current_key_is_base_equivalent {
            false
        } else {
            match prior_base_payload.as_ref() {
                Some(prev_base) => !base_snapshot_equivalent(prev_base, &payload),
                None => true,
            }
        };
        // Skip the re-insert entirely when the current key already holds a
        // base-equivalent payload: it stays continuously present, so no base
        // reader can ever observe it absent. Otherwise insert (fresh content
        // or a base-visible change at the current key).
        if !current_key_is_base_equivalent {
            self.insert_artifact_entry(current_key, payload, tick);
        }
        if snapshot_changed {
            self.bump_artifact_generation();
        }

        // Demand-driven coherence — gated on augmentation-contribution
        // equivalence. A base-equivalent reinsert whose augmentation facts AND
        // `parse_stable_hash` (the `AugmenterSet` fingerprint input) are
        // unchanged is a true no-op for the index: invalidating would remove a
        // contributing row and bump `artifact_generation` on a no-op, churning
        // the store-view validation token and forcing warm-cache misses /
        // base-view rebuilds. A content change (current key NOT base-equivalent)
        // always invalidates the NEW target — its contribution may differ — and
        // the drain above already cleaned the prior versions' rows. A genuine
        // contribution change at a base-equivalent key (rare: same base
        // snapshot, different augmentation facts or moved `parse_stable_hash`)
        // likewise invalidates.
        let publish_invalidation_needed = match current_key_prior_payload.as_ref() {
            Some(prior_payload) => {
                !augmentation_contribution_equivalent(prior_payload, &payload_for_compare)
            }
            None => true,
        };
        if publish_invalidation_needed {
            self.invalidate_augmentation_index_for_augmenter(&new_augmentations);
        }

        if had_prior {
            // Replacement: live count unchanged, bump stale sweep.
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
        // Audit event fires whenever a genuinely NEW content version was
        // built and inserted — a fresh insert OR a content-changed
        // replacement (the edit-cycle rebuild of an already-tracked
        // canonical): the current content key held NO entry before this
        // insert. Re-inserts of an already-stored version record NOTHING —
        // a base-equivalent reinsert is a literal no-op serve, and an
        // edge-refresh re-materialise reuses the content-addressed payload
        // (no shallow re-processing happened). `indexed_ready_builds`
        // keeps the read-once meaning "this request BUILT the artifact for
        // this (canonical, whole_hash)", never "this request re-served or
        // edge-refreshed an already-built version".
        if !current_key_prior_entry_present {
            crate::component_meta_audit::record_indexed_ready_built(
                Arc::clone(&canonical_for_event),
                whole_hash,
            );
            #[cfg(test)]
            if let Some(state) = self.test_audit_hook.lock().as_ref() {
                state.record_shallow_process(canonical_for_event.as_ref());
            }
        }
    }

    /// Remove every entry for `canonical_id` regardless of content hash
    /// (legacy `FileArtifactStore::remove` semantics).
    pub fn remove(&self, canonical_id: &str) {
        let to_remove: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .filter(|entry| entry.key().canonical.as_ref() == canonical_id)
            .map(|entry| entry.key().clone())
            .collect();
        // Route through the single removal chokepoint: it drops the
        // entries (embedded warm-read bookkeeping goes with them) and
        // invalidates the augmentation index the removed augmenters
        // contributed to.
        let removed = self.retire_artifact_keys(&to_remove);
        let removed_any = !removed.is_empty();
        for _ in &removed {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        if removed_any {
            self.bump_artifact_generation();
        }
    }

    /// Number of live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let canonical: Arc<str> = Arc::from(marker);
        let indexed = Arc::new(IndexedReady::new_for_test([0u8; 16]));
        let payload = Arc::new(FileArtifacts::with_indexed(indexed));
        let key = FileArtifactKey::base_for_test(canonical, [0u8; 16]);
        // Tick 0: the synthetic inserter never counted as an access.
        let prev = self.insert_artifact_entry(key, payload, 0);
        if prev.is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // New `FileArtifactKey`-keyed canonical API surface.
    //
    // Later layers (upsert no-op, fact emission, multi-version
    // 6c augmentation stitching, etc.) write through these methods.
    // ──────────────────────────────────────────────────────────────────

    /// Strict lookup by full content-addressed key.
    #[must_use]
    pub fn get_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        self.artifacts.get(key).map(|entry| {
            entry.value().record_hit();
            Arc::clone(&entry.value().payload)
        })
    }

    /// Fetch an augmenter's `FileArtifacts` by its captured exact
    /// `FileArtifactKey`, self-healing a STALE captured key after a
    /// same-canonical re-key whose decl skeleton (hence `parse_stable_hash`,
    /// hence the augmenter-set fingerprint) is unchanged.
    ///
    /// The augmentation index captures each augmenter's exact
    /// content-addressed `FileArtifactKey` at index-population time, and
    /// the augmenter-set fingerprint folds over `parse_stable_hash`, NOT
    /// `content_hash`. So a member-body / cosmetic edit to an augmenter —
    /// content hash advances, decl skeleton unchanged — does NOT
    /// invalidate the cached `AugmenterSet`: its
    /// [`AugmenterEntry::artifact_key`] keeps pointing at the PRE-edit
    /// content hash, which a same-canonical re-key has drained from the
    /// store. A bare [`Self::get_artifacts`] then misses, and a caller
    /// that silently skips the augmenter would shrink the stitched
    /// surface while keeping the (now wrong) fingerprint / count.
    ///
    /// On a miss this re-derives the augmenter's CURRENT exact key by
    /// advancing ONLY the `content_hash` dimension to
    /// `current_content_hash` (the scheduler-authoritative current content
    /// hash — for the names stitch the `contributor_whole_hash` oracle, for
    /// the body stitch `IndexedReady::whole_hash`) while preserving the
    /// captured key's `parse_env_hash` / `parse_key` shape (so base
    /// and overlay augmenters both heal correctly), and reads pinned to
    /// it — never a content-agnostic `get_artifacts_any` scan.
    ///
    /// Returns `(artifacts, refreshed_key)`. `refreshed_key` is `Some`
    /// ONLY when the heal fired (the captured key missed but the
    /// re-derived current key hit) so the caller can write the refreshed
    /// key back into the cached `AugmenterSet`. A genuine miss after the
    /// current-key re-fetch (the augmenter's artifact is not materialised
    /// under its current content hash) returns `None` — a principled
    /// skip, never a content-agnostic fallback.
    #[must_use]
    pub fn augmenter_artifacts_self_healing(
        &self,
        captured_key: &FileArtifactKey,
        current_content_hash: Hash16,
    ) -> Option<(Arc<FileArtifacts>, Option<FileArtifactKey>)> {
        if let Some(art) = self.get_artifacts(captured_key) {
            return Some((art, None));
        }
        // The captured key already carries the current content hash, so
        // the miss is a genuine absence, not a stale-key miss — no heal.
        if current_content_hash == captured_key.content_hash {
            return None;
        }
        let current_key = self
            .canonical_keys
            .get(captured_key.canonical.as_ref())
            .and_then(|slot| {
                slot.value()
                    .iter()
                    .filter(|version| {
                        version.span.is_live()
                            && version.key.content_hash == current_content_hash
                            && version.key.parse_env_hash == captured_key.parse_env_hash
                            && version.key.parse_key == captured_key.parse_key
                            && version.key.file_language_id == captured_key.file_language_id
                            && version.key.build_toolchain_fingerprint
                                == captured_key.build_toolchain_fingerprint
                    })
                    .map(|version| version.key.clone())
                    .next()
            })?;
        self.get_artifacts(&current_key)
            .map(|art| (art, Some(current_key)))
    }

    /// Look up a `FileArtifacts` payload for `canonical` whose key's
    /// `content_hash` equals `content_hash`, **regardless of the
    /// `parse_env_hash` / `parse_key` dimensions**.
    ///
    /// This is content-addressed by the `(canonical, content_hash)`
    /// pair — strictly narrower than the permissive
    /// [`Self::get_artifacts_any`] (which ignores `content_hash` too).
    /// It is the read for consumers that need the **parse-domain
    /// `FileFacts` registry** for a specific observed content version:
    /// a base artifact and a session-overlay artifact
    /// for the SAME content version carry an identical parse-fact
    /// registry, so the `parse_env_hash` discriminator is irrelevant to
    /// a parse-fact lookup. Returns the first matching candidate; for
    /// `.facts` recovery any candidate at the content hash is
    /// equivalent. A reader that needs the full base- or overlay-specific
    /// `IndexedReady` must use [`Self::get`] or
    /// [`Self::get_overlay_scoped`] with the right key.
    #[must_use]
    pub fn get_artifacts_for_content(
        &self,
        canonical: &str,
        content_hash: Hash16,
        parse_key: &verter_language::ParseKey,
        file_language_id: &FileLanguage,
    ) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let mut matched: Option<Arc<FileArtifacts>> = None;
        if let Some(slot) = self.canonical_keys.get(canonical) {
            for key in slot
                .value()
                .iter()
                .filter(|version| {
                    version.span.is_live() && version.key.content_hash == content_hash
                        && version.key.parse_key == *parse_key
                        && version.key.file_language_id == *file_language_id
                        && version.key.build_toolchain_fingerprint
                            == crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
                })
                .map(|version| &version.key)
            {
                // Exact read; a (benign) dangling index key just misses
                // and the next candidate is tried.
                if let Some(entry) = self.artifacts.get(key) {
                    entry.value().record_hit();
                    matched = Some(Arc::clone(&entry.value().payload));
                    break;
                }
            }
        }
        matched
    }

    /// Test-only content scan with deliberately incomplete identity.
    ///
    /// This exists for store retention and bookkeeping tests that intentionally
    /// ask whether any candidate at a content hash remains. It is not an exact
    /// artifact lookup and production code cannot call it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn scan_artifacts_by_content_for_test(
        &self,
        canonical: &str,
        content_hash: Hash16,
    ) -> Option<Arc<FileArtifacts>> {
        let slot = self.canonical_keys.get(canonical)?;
        slot.value()
            .iter()
            .filter(|version| {
                version.span.is_live()
                    && version.key.content_hash == content_hash
                    && version.key.build_toolchain_fingerprint
                        == crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
            })
            .find_map(|version| self.artifacts.get(&version.key))
            .map(|entry| {
                entry.value().record_hit();
                Arc::clone(&entry.value().payload)
            })
    }

    /// Look up the current-build base artifact for an exact content hash.
    #[must_use]
    pub(crate) fn get_base_artifacts_for_content(
        &self,
        canonical: &str,
        content_hash: Hash16,
        parse_key: &verter_language::ParseKey,
        file_language_id: &FileLanguage,
    ) -> Option<Arc<FileArtifacts>> {
        let slot = self.canonical_keys.get(canonical)?;
        slot.value()
            .iter()
            .filter(|version| {
                version.span.is_live()
                    && version.key.content_hash == content_hash
                    && version.key.parse_key == *parse_key
                    && version.key.file_language_id == *file_language_id
                    && version.key.is_base()
            })
            .find_map(|version| self.artifacts.get(&version.key))
            .map(|entry| Arc::clone(&entry.value().payload))
    }

    /// Look up the current-build overlay artifact for an exact view discriminator.
    #[must_use]
    pub(crate) fn get_overlay_artifacts_scoped(
        &self,
        canonical: &str,
        content_hash: Hash16,
        discriminator: Hash16,
        parse_key: &verter_language::ParseKey,
        file_language_id: &FileLanguage,
    ) -> Option<Arc<FileArtifacts>> {
        let slot = self.canonical_keys.get(canonical)?;
        slot.value()
            .iter()
            .filter(|version| {
                version.span.is_live()
                    && version.key.content_hash == content_hash
                    && version.key.parse_env_hash == discriminator
                    && version.key.parse_key == *parse_key
                    && version.key.file_language_id == *file_language_id
                    && version.key.build_toolchain_fingerprint
                        == crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint()
            })
            .find_map(|version| self.artifacts.get(&version.key))
            .map(|entry| Arc::clone(&entry.value().payload))
    }

    /// Look up the latest **base** `FileArtifacts` payload for
    /// `canonical`.
    ///
    /// This is the base canonical-wide read: it resolves the
    /// canonical's candidate keys through the canonical→keys index and
    /// filters to [`FileArtifactKey::is_base`] entries, so a
    /// session-overlay artifact published under an
    /// overlay-scoped key is never surfaced to a
    /// base reader (which would otherwise read the overlay's
    /// session-specific `IndexedReady` import routes). A session-view
    /// reader uses
    /// [`OverlayArtifactIdentity::lookup_overlay_artifacts`](crate::host_manage::overlay_materialize::OverlayArtifactIdentity::lookup_overlay_artifacts)
    /// (exact key) instead.
    #[must_use]
    pub fn get_artifacts_any(&self, canonical: &str) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let mut matched: Option<Arc<FileArtifacts>> = None;
        if let Some(slot) = self.canonical_keys.get(canonical) {
            for key in slot
                .value()
                .iter()
                .filter(|version| version.span.is_live() && version.key.is_base())
                .map(|version| &version.key)
            {
                // Exact read; a (benign) dangling index key just misses
                // and the next candidate is tried.
                if let Some(entry) = self.artifacts.get(key) {
                    entry.value().record_hit();
                    matched = Some(Arc::clone(&entry.value().payload));
                    break;
                }
            }
        }
        matched
    }

    /// Insert (or replace) the payload for `key`.
    pub fn insert_artifacts(
        &self,
        key: FileArtifactKey,
        artifacts: Arc<FileArtifacts>,
    ) -> Option<Arc<FileArtifacts>> {
        let canonical = Arc::clone(&key.canonical);
        let content_hash = key.content_hash;
        let parse_env_hash = key.parse_env_hash;
        let tick = self.next_access_tick();
        // Capture the registry handle + the full payload handle BEFORE
        // moving `artifacts` into the DashMap. Cold-path: cheap Arc clones.
        // `artifacts_for_compare` lets the bump-iff-actually-changed gate
        // compare the replaced value against the incoming one without
        // re-fetching from the map.
        let facts_for_emit: Arc<FileFacts> = Arc::clone(&artifacts.facts);
        // Capture the full payload handle too so the bump-iff-actually-changed
        // gate can compare the replaced value against the incoming one without
        // re-fetching from the map.
        let artifacts_for_compare: Arc<FileArtifacts> = Arc::clone(&artifacts);
        let prev = self.insert_artifact_entry(key, artifacts, tick);
        // Demand-driven coherence — gated on augmentation-contribution
        // equivalence. A byte-identical reinsert of a module-augmentation file
        // leaves both the augmenter's `ModuleAugmentationFact` set AND its
        // `parse_stable_hash` (the `AugmenterSet` fingerprint input) unchanged,
        // so no index entry's fold can change: invalidating would bump the base-
        // folded `artifact_generation` on a no-op (the invalidation removes
        // contributing rows and bumps on removal), churning the store-view
        // validation token and forcing warm-cache misses / base-view rebuilds.
        // Only a CHANGED contribution (fresh augmenter, retarget, a changed
        // augmented-member fingerprint, or a moved `parse_stable_hash`)
        // invalidates — with the union of the new ∪ replaced facts, so both the
        // prior and new targets are cleaned and the next cold rescan folds the
        // change in.
        let augmentation_contribution_changed = match prev.as_ref() {
            Some(prev_value) => {
                !augmentation_contribution_equivalent(prev_value, &artifacts_for_compare)
            }
            // A fresh insert that declares augmentations is an absent → present
            // contribution: any index row scanned before this augmenter existed
            // is stale and must be invalidated.
            None => !artifacts_for_compare.augmentations.is_empty(),
        };
        if augmentation_contribution_changed {
            let mut changed_augmentations: Vec<ModuleAugmentationFact> = artifacts_for_compare
                .augmentations
                .iter()
                .cloned()
                .collect();
            if let Some(prev) = prev.as_ref() {
                changed_augmentations.extend(prev.augmentations.iter().cloned());
            }
            self.invalidate_augmentation_index_for_augmenter(&changed_augmentations);
        }
        let is_fresh = prev.is_none();
        if is_fresh {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        // Bump the base-folded `artifact_generation` ONLY when this insert
        // actually changes a base-visible snapshot value (R4 parity with
        // the bump-iff-transition treatment elsewhere). A FRESH insert
        // always bumps (the canonical's snapshot value goes absent →
        // present). A REPLACE bumps unless the new payload is
        // indistinguishable from the old one in EVERY dimension a base
        // `HostStoreView` snapshots BY VALUE (`base_snapshot_equivalent`) —
        // a true no-op (re-insert of byte-identical content) and an
        // overlay-scoped re-insert that does not alter any base snapshot
        // must NOT churn the token, which would otherwise spuriously
        // invalidate the manager-cached base view and split singleflight
        // lanes. The comparison is CONSERVATIVE: any real change to a
        // base-visible value still bumps (mandatory no-under-bump).
        let snapshot_changed = match prev.as_ref() {
            Some(prev_value) => !base_snapshot_equivalent(prev_value, &artifacts_for_compare),
            None => true,
        };
        if snapshot_changed {
            self.bump_artifact_generation();
        }
        // R23 typed event: a `FileArtifactStore` entry was admitted.
        // Best-effort emission — silent no-op when no observer
        // accumulator is installed on the current thread.
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                canonical_id: Arc::clone(&canonical),
                action: verter_audit::FileArtifactCacheAction::Admit,
                content_hash,
                parse_env_hash,
                entry_count_after: self.artifacts.len() as u32,
            },
        );
        // R23 typed event: emit one `FactRegistryWrite` per parse-
        // domain fact admitted to the registry. Cold-path / fresh-
        // insert only — a replacement insert (`prev.is_some()`) does
        // NOT re-emit (the registry was already attributed on its
        // original parse-time admit). The emission attributes each
        // fact to the canonical id whose registry it lives in, so
        // downstream telemetry can fold registry writes per file.
        if is_fresh {
            for (_key, fact) in facts_for_emit.registry().iter() {
                emit_fact_registry_writes(&canonical, fact);
            }
        }
        prev
    }

    /// Remove the artifact under `key`.
    pub fn remove_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        let canonical = Arc::clone(&key.canonical);
        let content_hash = key.content_hash;
        let parse_env_hash = key.parse_env_hash;
        // Route through the single removal chokepoint: it clears the hit
        // counter and invalidates the augmentation index the removed
        // augmenter contributed to.
        let removed = self.drop_artifact_entry(key);
        if removed.is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            // The chokepoint (`drop_artifact_entry` → `retire_artifact_keys`)
            // already cleared the per-key hit counter and invalidated the
            // augmentation index. A keyed removal drops a canonical's
            // `IndexedReady` / `FileFacts` / derived hashes that a
            // `HostStoreView` snapshots BY VALUE; bump the generation so a
            // view built before this removal (e.g. a manager-cached base
            // view surviving a reachability GC) is token-invalidated and
            // rebuilt. This is the GC path
            // (`evict_unreachable_artifacts_with_policy` routes every
            // unreachable version through here).
            self.bump_artifact_generation();
            // R23 typed event: a `FileArtifactStore` entry was
            // evicted. Best-effort emission.
            crate::host_manage::push_structured_event(
                crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                    canonical_id: canonical,
                    action: verter_audit::FileArtifactCacheAction::Evict,
                    content_hash,
                    parse_env_hash,
                    entry_count_after: self.artifacts.len() as u32,
                },
            );
        }
        removed
    }

    /// Drain every entry whose canonical matches `canonical_id`.
    pub fn remove_canonical(&self, canonical_id: &str) -> usize {
        let to_remove: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .filter(|entry| entry.key().canonical.as_ref() == canonical_id)
            .map(|entry| entry.key().clone())
            .collect();
        // Route through the single removal chokepoint: it drops the
        // entries (embedded warm-read bookkeeping goes with them) and
        // invalidates the augmentation index the removed augmenters
        // contributed to.
        let removed_pairs = self.retire_artifact_keys(&to_remove);
        let removed = removed_pairs.len();
        if removed > 0 {
            self.live_counter
                .fetch_sub(removed as u64, Ordering::Relaxed);
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
            // Draining every version of a canonical drops by-value
            // snapshot dimensions; bump the generation so a pre-removal
            // `HostStoreView` is token-invalidated.
            self.bump_artifact_generation();
            // R23 typed event: each eviction emits one event so
            // downstream telemetry can attribute drain footprint
            // per `FileArtifactKey` dimension.
            for (key, _payload) in &removed_pairs {
                crate::host_manage::push_structured_event(
                    crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                        canonical_id: Arc::clone(&key.canonical),
                        action: verter_audit::FileArtifactCacheAction::Evict,
                        content_hash: key.content_hash,
                        parse_env_hash: key.parse_env_hash,
                        entry_count_after: self.artifacts.len() as u32,
                    },
                );
            }
        }
        removed
    }

    /// Drop EVERY artifact in the store — the `set_workspace` cascade
    /// step. A workspace-authority swap orphans every artifact-only
    /// canonical's payload (its content authority is gone) and makes a
    /// full rebuild of scheduler-tracked artifacts the only provably
    /// correct posture. Routed through the same removal chokepoint as
    /// [`Self::remove_canonical`] so the canonical→keys index and the
    /// augmentation index stay coherent, and bumps the artifact
    /// generation so a pre-clear `HostStoreView` is token-invalidated.
    pub fn clear_all(&self) {
        let to_remove: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let removed_pairs = self.retire_artifact_keys(&to_remove);
        let removed = removed_pairs.len();
        if removed > 0 {
            self.live_counter
                .fetch_sub(removed as u64, Ordering::Relaxed);
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
            self.bump_artifact_generation();
            for (key, _payload) in &removed_pairs {
                crate::host_manage::push_structured_event(
                    crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                        canonical_id: Arc::clone(&key.canonical),
                        action: verter_audit::FileArtifactCacheAction::Evict,
                        content_hash: key.content_hash,
                        parse_env_hash: key.parse_env_hash,
                        entry_count_after: self.artifacts.len() as u32,
                    },
                );
            }
        }
    }

    /// Snapshot every key, for diagnostics / reachability sweeps.
    #[must_use]
    pub fn artifact_keys(&self) -> Vec<FileArtifactKey> {
        self.artifacts
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Snapshot every `(key, payload)` pair.
    #[must_use]
    pub fn snapshot_artifacts(&self) -> Vec<(FileArtifactKey, Arc<FileArtifacts>)> {
        self.artifacts
            .iter()
            .map(|entry| (entry.key().clone(), Arc::clone(&entry.value().payload)))
            .collect()
    }

    /// Visit every live `(key, payload)` variant of `canonical` whose
    /// `content_hash` matches — the targeted read backing the
    /// store-view `file_facts` / `source_envs` snapshot. Resolves the
    /// canonical's candidate keys through the canonical→keys index and
    /// reads `self.artifacts` by EXACT key (no whole-store scan, no
    /// whole-store `Vec`); a (benign) dangling index key just misses.
    /// Base and overlay-scoped variants at the hash are both visited.
    pub fn for_each_artifact_for_canonical_content(
        &self,
        canonical: &str,
        content_hash: Hash16,
        mut visit: impl FnMut(&FileArtifactKey, &Arc<FileArtifacts>),
    ) {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return;
        }
        if let Some(slot) = self.canonical_keys.get(canonical) {
            for key in slot
                .value()
                .iter()
                .filter(|version| {
                    version.span.is_live() && version.key.content_hash == content_hash
                })
                .map(|version| &version.key)
            {
                if let Some(entry) = self.artifacts.get(key) {
                    visit(key, &entry.value().payload);
                }
            }
        }
    }

    /// Aggregate `(entry_count, canonical + source byte sum)` over the
    /// FULL keyed artifact population (base + overlay-scoped) — the
    /// audit-snapshot read. Count and bytes are drawn from one
    /// iteration of the same population `snapshot_artifacts()`
    /// enumerates, without materializing the whole-store `Vec`.
    #[must_use]
    pub fn artifact_count_and_source_bytes(&self) -> (u32, u64) {
        let mut count: u32 = 0;
        let mut bytes: u64 = 0;
        for entry in self.artifacts.iter() {
            count = count.saturating_add(1);
            let indexed = &entry.value().payload.indexed;
            bytes += entry.key().canonical.len() as u64
                + indexed.raw_source.len() as u64
                + indexed.eval_source.len() as u64;
        }
        (count, bytes)
    }

    // ──────────────────────────────────────────────────────────────────
    // Augmentation index API (populated lazily by the
    // augmentation-stitching pass — see `/type-cache-architecture` skill)
    // ──────────────────────────────────────────────────────────────────

    /// Look up the [`AugmenterSet`] for an [`AugmentationTargetKey`].
    #[must_use]
    pub fn get_augmenter_set(&self, key: &AugmentationTargetKey) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index
            .get(key)
            .map(|entry| Arc::clone(&entry.value().set))
    }

    /// THE sole augmentation-index publication combinator.
    ///
    /// Publishes `set` at `key` under a freshly advanced membership
    /// epoch and RETIRES the version it supersedes into the retired
    /// chain, so a root captured before this publication keeps
    /// resolving the augmenter set its world had.
    fn install_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        let reservation = self.reserve_membership_epoch();
        let epoch = reservation.epoch();
        let version = AugmenterVersion {
            span: VersionSpan::born(epoch),
            set,
        };
        // The whole read-retire-publish sequence runs under ONE entry
        // guard. Two reasons, both load-bearing:
        //
        // * ATOMICITY — the displaced value is the caller's "was this
        //   absent?" signal, and `populate_augmenter_set` bumps the
        //   artifact generation only on a genuine absent → present
        //   transition. A separate read-then-insert lets N concurrent
        //   duplicate populates each observe "absent" and each bump,
        //   churning the base store-view reuse token.
        // * ORDERING — publish before retract, as on the artifact path
        //   (see [`Self::publish_retired_version`]). The superseded
        //   version reaches the retired chain before the live slot stops
        //   holding it, so a root-relative reader can never find it in
        //   neither map.
        let retired = match self.augmentation_index.entry(key.clone()) {
            AugmentationIndexEntry::Occupied(mut slot) => {
                let previous = slot.get();
                self.retired_augmenters
                    .entry(key)
                    .or_default()
                    .push(AugmenterVersion {
                        span: VersionSpan {
                            birth: previous.span.birth,
                            retirement: Some(epoch),
                        },
                        set: Arc::clone(&previous.set),
                    });
                Some(slot.insert(version).set)
            }
            AugmentationIndexEntry::Vacant(slot) => {
                slot.insert(version);
                None
            }
        };
        drop(reservation);
        if retired.is_some() {
            self.note_retirements(1);
        }
        retired
    }

    /// THE sole augmentation-index retirement combinator. Logical, like
    /// every other membership removal: the entries leave the current
    /// root's membership at `epoch` and stay reachable from every root
    /// captured before it.
    fn retire_augmenter_keys(&self, keys: &[AugmentationTargetKey], epoch: u64) -> usize {
        let mut retired = 0usize;
        for key in keys {
            // One entry guard across the retire and the removal — same
            // atomicity + publish-before-retract contract as
            // [`Self::install_augmenter_set`].
            if let AugmentationIndexEntry::Occupied(slot) =
                self.augmentation_index.entry(key.clone())
            {
                let previous = slot.get();
                self.retired_augmenters
                    .entry(key.clone())
                    .or_default()
                    .push(AugmenterVersion {
                        span: VersionSpan {
                            birth: previous.span.birth,
                            retirement: Some(epoch),
                        },
                        set: Arc::clone(&previous.set),
                    });
                slot.remove();
                retired += 1;
            }
        }
        self.note_retirements(retired);
        retired
    }

    /// Install (or replace) the augmenter set under `key`. Used by
    /// the index-population path.
    /// The domain's current stable semantic generation, or `None` while
    /// an augmentation-world mutation is in flight.
    #[must_use]
    pub(crate) fn stable_route_surface_generation(&self) -> Option<u64> {
        self.route_surface_generation.stable()
    }

    /// Publish an augmenter set INSIDE the route-surface generation
    /// bracket.
    ///
    /// The single publication seam for the domain's clock, so both
    /// publishers apply the same rule rather than each deciding for
    /// itself. The generation advances only when a set that was ALREADY
    /// published is replaced by one with a DIFFERENT fingerprint:
    ///
    /// * first-time materialisation (`prev == None`) is the index
    ///   learning a row the artifact corpus already implied — a cache
    ///   population, not a change to the augmentation world;
    /// * a same-fingerprint republish (the stale-key self-heal) leaves
    ///   every recorded shape fact valid by construction, and an older
    ///   captured root still resolves the retired version through the
    ///   version chain, so birth-epoch movement alone is not a validity
    ///   flip.
    fn publish_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        let new_fingerprint = set.fingerprint;
        self.route_surface_generation.mutate(|| {
            let prev = self.install_augmenter_set(key, set);
            let changed = prev
                .as_ref()
                .is_some_and(|previous| previous.fingerprint != new_fingerprint);
            (prev, changed)
        })
    }

    pub fn populate_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        let new_fingerprint = set.fingerprint;
        let prev = self.publish_augmenter_set(key, set);
        // `route_surface_index_fingerprints` is snapshotted BY VALUE on a
        // `HostStoreView`. Bump the base-folded `artifact_generation` ONLY
        // when this populate actually changes the snapshotted fingerprint
        // (R4 parity with the bump-iff-actually-changed gate on the artifact
        // insert paths): a re-populate of an identical augmenter set is a
        // no-op for the base snapshot and must not churn the token. Any real
        // fingerprint change (including absent → present) still bumps (no
        // under-bump).
        let snapshot_changed = match prev.as_ref() {
            Some(prev_set) => prev_set.fingerprint != new_fingerprint,
            None => true,
        };
        if snapshot_changed {
            self.bump_artifact_generation();
        }
        prev
    }

    /// Snapshot the base-artifact augmenter rows into an owned `Vec`,
    /// then drop the `self.artifacts` shard guards.
    ///
    /// The match step (`augmenter_matches_target`) may invoke a
    /// caller-supplied resolver that re-enters `FileArtifactStore` and
    /// inserts into `self.artifacts` (a relative `declare module "./x"`
    /// target resolves its specifier through `ensure_indexed_ready_serve`,
    /// which materialises and inserts the dependency). The DashMap
    /// shards are `std::sync::RwLock`, which is non-reentrant: a write
    /// to a shard the current thread already read-locks via an active
    /// `iter()` guard would block on itself. Collecting the candidate
    /// rows first — and only matching/resolving after every shard guard
    /// is released — keeps the resolver off the guard. Same discipline
    /// as the snapshot in
    /// [`Self::ensure_augmentation_index_populated`].
    ///
    /// Candidate selection is population-aware (overlay isolation): a base
    /// scan (`overlay_discriminator: None`) collects base
    /// ([`FileArtifactKey::is_base`]) artifacts only; a session scan
    /// (`Some(discriminator)`) collects base artifacts UNIONED with this
    /// session's own overlay artifacts (the non-base key whose
    /// `parse_env_hash` discriminator matches). A different session's overlay
    /// artifact carries a different discriminator and is excluded — overlay
    /// augmenters never cross sessions or poison the base index. Only
    /// artifacts carrying at least one augmentation fact are collected.
    fn collect_augmenter_candidates(
        &self,
        overlay_discriminator: Option<Hash16>,
    ) -> Vec<AugmenterCandidate> {
        self.artifacts
            .iter()
            .filter(|entry| {
                let key = entry.key();
                // Base population: base-key artifacts only. Session
                // population: base artifacts UNIONED with the session's own
                // overlay artifacts (the non-base key whose `parse_env_hash`
                // discriminator matches this session). A DIFFERENT session's
                // overlay artifact carries a different discriminator and is
                // excluded — overlay augmenters never cross sessions or poison
                // the base index.
                key.is_base()
                    || overlay_discriminator
                        .is_some_and(|d| !key.is_base() && key.parse_env_hash == d)
            })
            .filter(|entry| !entry.value().payload.augmentations.is_empty())
            .map(|entry| AugmenterCandidate {
                artifact_key: entry.key().clone(),
                canonical: Arc::clone(&entry.key().canonical),
                parse_stable_hash: entry.value().payload.parse_stable_hash,
                augmentations: Arc::clone(&entry.value().payload.augmentations),
            })
            .collect()
    }

    /// Lazily populate the augmentation-index entry for `key`, then
    /// return the `AugmenterSet`.
    ///
    /// On a warm hit, returns the cached `Arc<AugmenterSet>` directly
    /// without re-scanning. On a cold miss, scans `self.artifacts` for
    /// files whose `FileArtifacts.augmentations` match the queried
    /// target under the current `(project_identity, resolve_env_hash,
    /// lib_env_hash)`, sorts the matched augmenters by
    /// `(augmenter_canonical, parse_stable_hash)`, computes a stable
    /// fingerprint, inserts, and emits a `ModuleAugmentationIndexShape`
    /// audit event recording the install (or refresh on
    /// re-population). Subsequent reads under the same key are
    /// warm-hit fast paths.
    ///
    /// Index population is incremental per R29 — only files that have
    /// entered `FileArtifactStore` contribute. There is no
    /// workspace-wide eager scan; out-of-program files contribute
    /// nothing (matches TypeScript's own augmentation-visibility
    /// rule).
    ///
    /// `resolve_relative_canonical` is a caller-supplied hook that
    /// resolves a `(augmenter_canonical, relative_specifier)` pair to
    /// a canonical when the queried target is `ResolvedRelativeCanonical`.
    /// `None` means the augmenter's specifier did not resolve to the
    /// queried canonical and the augmenter is skipped.
    pub fn ensure_augmentation_index_populated<R>(
        &self,
        key: &AugmentationTargetKey,
        resolve_relative_canonical: R,
        overlay_discriminator: Option<Hash16>,
    ) -> Arc<AugmenterSet>
    where
        R: Fn(&str, &str) -> Option<Arc<str>>,
    {
        if let Some(existing) = self.augmentation_index.get(key) {
            return Arc::clone(&existing.value().set);
        }

        // Cold scan — collect (canonical, parse_stable_hash) for
        // every artifact whose augmentations include at least one
        // matching `ModuleAugmentationFact` for the queried target.
        // Dedup by canonical so a file with multiple matching facts
        // contributes only once.
        //
        // The scan filters to base ([`FileArtifactKey::is_base`])
        // artifacts: the augmentation index is keyed by a base
        // resolve-domain identity (`project_identity`,
        // `resolve_env_hash`, `lib_env_hash`). A session-overlay artifact
        // An overlay-scoped key carries session-divergent
        // augmentations and must not poison that base index.
        // Snapshot first, then match off the guard: the resolver invoked
        // by `augmenter_matches_target` for a relative target re-enters
        // the store and inserts into `self.artifacts`, which cannot run
        // while a `self.artifacts.iter()` shard guard is held (see
        // `collect_augmenter_candidates`).
        let candidates = self.collect_augmenter_candidates(overlay_discriminator);
        let mut matched: Vec<AugmenterEntry> = Vec::new();
        let mut seen_canonicals: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        for candidate in &candidates {
            for fact in candidate.augmentations.iter() {
                if augmenter_matches_target(
                    fact,
                    key,
                    candidate.canonical.as_ref(),
                    &resolve_relative_canonical,
                ) {
                    if seen_canonicals.insert(Arc::clone(&candidate.canonical)) {
                        // Capture the EXACT artifact key — the stitch
                        // consumer re-fetches `.augmentations` via
                        // `get_artifacts(&key)` so it reads precisely
                        // the version fingerprinted here.
                        matched.push(AugmenterEntry {
                            artifact_key: candidate.artifact_key.clone(),
                            parse_stable_hash: candidate.parse_stable_hash,
                        });
                    }
                    break;
                }
            }
        }

        // Sort by (canonical, parse_stable_hash) for determinism.
        matched.sort_by(|a, b| {
            a.canonical()
                .as_ref()
                .cmp(b.canonical().as_ref())
                .then_with(|| a.parse_stable_hash.cmp(&b.parse_stable_hash))
        });

        let augmenter_count = matched.len() as u32;
        let fingerprint = compute_augmenter_set_fingerprint(&matched);
        let entries: SmallVec<[AugmenterEntry; 2]> = matched.into_iter().collect();
        let set = Arc::new(AugmenterSet {
            entries,
            fingerprint,
        });

        // Insert. Capture prev fingerprint for audit event.
        let prev = self.publish_augmenter_set(key.clone(), Arc::clone(&set));
        let prev_fingerprint = prev.as_ref().map(|p| p.fingerprint);
        // `route_surface_index_fingerprints` is snapshotted BY VALUE on a
        // `HostStoreView`, and `artifact_generation` is folded into the
        // store-view reuse oracle. Bump it ONLY when this cold populate
        // actually changes the snapshotted fingerprint (R4 parity with
        // `populate_augmenter_set`): when two threads cold-scan the same
        // target concurrently, the
        // second `insert` replaces the first with an IDENTICAL fingerprint,
        // a no-op for the base snapshot that must not churn the token (which
        // would spuriously invalidate the manager-cached base view and split
        // singleflight lanes under batch load). Any real fingerprint change
        // (including absent → present) still bumps (no under-bump).
        if prev_fingerprint != Some(fingerprint) {
            self.bump_artifact_generation();
        }

        // Emit `ModuleAugmentationIndexShape` typed audit event.
        emit_module_augmentation_index_shape_event(
            key,
            prev_fingerprint,
            fingerprint,
            augmenter_count,
        );

        set
    }

    /// Invalidate every `augmentation_index` entry that the augmenter
    /// facts on `augmenter_facts` could contribute to.
    ///
    /// This is the SOLE augmentation-index invalidation primitive. It is the
    /// demand-driven coherence rail for the index: whenever the augmenter
    /// SET changes (a file is published as / edited into / removed as an
    /// augmenter), every entry the changed facts could touch is RETIRED, and
    /// the next [`Self::ensure_augmentation_index_populated`] cold-rescan
    /// rebuilds it from the now-current `artifacts` corpus. There is no eager
    /// in-place rebuild — invalidate-then-lazy-rebuild keeps the index
    /// query-scoped (Build Philosophy) and uniform across populations.
    ///
    /// Retirement is logical: a root captured before the invalidation
    /// keeps resolving the augmenter set its world had, because the
    /// augmentation-index is versioned membership like the artifacts
    /// themselves.
    ///
    /// Wired into every artifact-set mutation that changes a file's
    /// augmentation contribution: [`Self::insert`] / [`Self::insert_artifacts`]
    /// (publish — using the prior ∪ new facts so a retarget cleans BOTH the
    /// old and the new target) and [`Self::remove`] / [`Self::remove_artifacts`]
    /// / [`Self::remove_canonical`] (evict — using the removed facts). It is
    /// also called from the side-effect-import probe walk
    /// (`VerterHost::owner_has_module_augmentation_dependency`) after newly
    /// materialising an augmenter, so a probe of a target whose cold scan ran
    /// BEFORE the augmenter entered the store does not warm-hit a stale empty
    /// set.
    ///
    /// Matching is the resolver-free, population-agnostic conservative test
    /// [`augmenter_fact_could_contribute`] — it removes BOTH `Base` and
    /// `Session` entries the augmenter could touch (a base augmenter
    /// participates in every `Session` set, which is base ∪ overlay), and
    /// over-matches relative targets safely (drop-then-rebuild can never add a
    /// wrong augmenter, so overlay isolation and the no-base-poison rule hold).
    /// Entries the facts do NOT touch are left untouched — out-of-program
    /// files contribute nothing.
    ///
    /// Snapshot-first / no-shard-guard discipline: existing keys are collected
    /// off the `augmentation_index` shard guard before any removal, so the
    /// removal loop never holds a guard across a re-entrant store access.
    ///
    /// Returns the count of entries actually retired.
    pub fn invalidate_augmentation_index_for_augmenter(
        &self,
        augmenter_facts: &[ModuleAugmentationFact],
    ) -> usize {
        // A standalone invalidation is one membership transition of its
        // own. The artifact-retirement chokepoint instead threads ITS
        // epoch through [`Self::invalidate_augmentation_index_at_epoch`]
        // so artifacts and index retire together.
        let reservation = self.reserve_membership_epoch();
        let epoch = reservation.epoch();
        let retired = self.invalidate_augmentation_index_at_epoch(augmenter_facts, epoch);
        drop(reservation);
        retired
    }

    /// [`Self::invalidate_augmentation_index_for_augmenter`] under a
    /// caller-supplied membership epoch — the form the artifact
    /// retirement chokepoint uses so both halves of one removal share a
    /// single epoch and no root can observe them split.
    fn invalidate_augmentation_index_at_epoch(
        &self,
        augmenter_facts: &[ModuleAugmentationFact],
        epoch: u64,
    ) -> usize {
        if augmenter_facts.is_empty() {
            return 0;
        }
        if self.augmentation_index.is_empty() {
            return 0;
        }
        // Snapshot existing keys off the shard guard before retiring entries.
        let retire_keys: Vec<AugmentationTargetKey> = self
            .augmentation_index
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|key| {
                augmenter_facts
                    .iter()
                    .any(|fact| augmenter_fact_could_contribute(fact, key))
            })
            .collect();
        // Retirement removes index contributors, which IS a change to the
        // augmentation world — so it advances the route-surface clock,
        // inside the same bracket, unlike the publication cases above.
        let removed = self.route_surface_generation.mutate(|| {
            let removed = self.retire_augmenter_keys(&retire_keys, epoch);
            (removed, removed > 0)
        });
        if removed > 0 {
            self.bump_artifact_generation();
        }
        removed
    }

    /// Retire every entry from the augmentation index.
    pub fn clear_augmentation_index(&self) {
        let reservation = self.reserve_membership_epoch();
        let all_keys: Vec<AugmentationTargetKey> = self
            .augmentation_index
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        self.retire_augmenter_keys(&all_keys, reservation.epoch());
        drop(reservation);
        self.bump_artifact_generation();
    }

    /// Number of entries in the augmentation index.
    #[must_use]
    pub fn augmentation_index_len(&self) -> usize {
        self.augmentation_index.len()
    }

    /// Snapshot every `(AugmentationTargetKey, fingerprint)` pair in
    /// the augmentation index. Used by `HostStoreView::build` to copy
    /// the route-surface-domain fingerprints into the view for
    /// per-candidate validation (R29 + G1 + R26).
    #[must_use]
    pub fn snapshot_augmentation_index_fingerprints(&self) -> Vec<(AugmentationTargetKey, Hash16)> {
        self.augmentation_index
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().set.fingerprint))
            .collect()
    }

    /// Collect the distinct wildcard-ambient patterns
    /// (`declare module "*.css"`) declared by any base artifact currently
    /// in the store.
    ///
    /// A wildcard ambient applies to an importer through a matching
    /// import, so its glob pattern cannot be derived from the importer's
    /// own specifiers — a compile-eligibility probe must enumerate the
    /// declared patterns and query each as a
    /// [`AugmentationTargetKind::WildcardAmbient`] target. External
    /// (`declare module "vue"`) and relative (`declare module "./x"`)
    /// augmenters always require a matching importer specifier (the
    /// importer derives those targets from its own import list), and a
    /// global block is probed directly as
    /// [`AugmentationTargetKind::GlobalAugmentation`]; this accessor
    /// covers only the wildcard remainder.
    ///
    /// The scan reads declared [`ModuleAugmentationFact`] specifiers (the
    /// authoritative augmentation source) and filters to base
    /// ([`FileArtifactKey::is_base`]) artifacts — session-overlay
    /// artifacts carry session-divergent augmentations and must not leak
    /// into a base-domain probe set. The returned patterns are
    /// deduplicated.
    #[must_use]
    pub fn declared_wildcard_ambient_patterns(&self) -> Vec<InternedGlobPattern> {
        let mut wildcard_patterns: Vec<InternedGlobPattern> = Vec::new();
        let mut seen_patterns: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        for artifact_entry in self.artifacts.iter() {
            if !artifact_entry.key().is_base() {
                continue;
            }
            for fact in artifact_entry.value().payload.augmentations.iter() {
                let specifier: &str = fact.specifier.as_ref();
                if specifier.contains('*') && seen_patterns.insert(Arc::from(specifier)) {
                    wildcard_patterns.push(InternedGlobPattern::from(specifier));
                }
            }
        }
        wildcard_patterns
    }
}

impl crate::cache_schema::CacheSchemaVersioned for FileArtifactStore {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        // Whole-store reset. It routes through the SAME retirement
        // chokepoint as every partial removal rather than clearing the
        // maps: a schema mismatch changes what the CURRENT root serves,
        // it does not revoke the lease a live `FileArtifactRoot` already
        // holds, and physically clearing here would free versions those
        // roots still address. The chokepoint retires the artifacts and
        // the augmentation-index rows they contributed to under one
        // epoch, so no stale `AugmenterSet` survives into the current
        // root either. `clear_augmentation_index` retires whatever rows
        // the artifact facts did not cover, making the reset total.
        let all_keys: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let count = all_keys.len();
        let _retired = self.retire_artifact_keys(&all_keys);
        self.clear_augmentation_index();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        }
        self.bump_artifact_generation();
        count
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for FileArtifactStore {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent]
    }
    fn invalidate(&self, _domain: crate::invalidation_domain::InvalidationDomain) {
        // FileArtifacts survives project-generation bumps (content_hash is
        // sufficient identity); per-canonical eviction is the only
        // invalidation mode.
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for FileArtifactStore {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.len();
        self.remove(canonical_id);
        let after = self.len();
        before.saturating_sub(after)
    }
}

/// Special marker the parse-domain emission uses for `declare global
/// { ... }` blocks (see `fact_emission::GLOBAL_AUGMENTATION_TAG`).
/// Duplicated here to keep the matcher free-standing of fact_emission.
const GLOBAL_AUGMENTATION_TAG: &str = "$global";

/// Does `fact` (emitted by `augmenter_canonical`) contribute to the
/// queried `target_key`?
///
/// Classification semantics by target-kind archetype:
///
/// - `ExternalSpecifier(s)` → match `fact.specifier == s` AND the
///   specifier is NOT relative, NOT a wildcard, NOT the global tag.
/// - `ResolvedRelativeCanonical(canon)` → match relative specifiers
///   (the full TS `pathIsRelative` class via
///   [`verter_workspace::resolver::is_relative_specifier`]) whose
///   `resolve_relative_canonical` resolves equal to `canon`.
/// - `WildcardAmbient(pattern)` → match `fact.specifier == pattern`
///   AND the specifier contains a wildcard `*`.
/// - `GlobalAugmentation` → match `fact.specifier == "$global"`.
///
/// Relative classification MUST be the shared resolver predicate, not
/// a `./`/`../` prefix check: a `declare module '..'` fact is the
/// parent-directory index module, and treating it as a bare external
/// named `..` would match location-independently against any `'..'`
/// import target regardless of the directories involved.
pub(crate) fn augmenter_matches_target<R>(
    fact: &ModuleAugmentationFact,
    target_key: &AugmentationTargetKey,
    augmenter_canonical: &str,
    resolve_relative_canonical: R,
) -> bool
where
    R: Fn(&str, &str) -> Option<Arc<str>>,
{
    use verter_workspace::resolver::is_relative_specifier;
    let specifier: &str = fact.specifier.as_ref();
    match &target_key.target {
        AugmentationTargetKind::ExternalSpecifier(target_spec) => {
            // Bare external: not relative, not wildcard, not global.
            let is_relative = is_relative_specifier(specifier);
            let is_wildcard = specifier.contains('*');
            let is_global = specifier == GLOBAL_AUGMENTATION_TAG;
            !is_relative && !is_wildcard && !is_global && specifier == target_spec.as_ref()
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(target_canon) => {
            if !is_relative_specifier(specifier) {
                return false;
            }
            match resolve_relative_canonical(augmenter_canonical, specifier) {
                Some(resolved) => resolved.as_ref() == target_canon.as_ref(),
                None => false,
            }
        }
        AugmentationTargetKind::WildcardAmbient(target_pattern) => {
            specifier.contains('*') && specifier == target_pattern.as_ref()
        }
        AugmentationTargetKind::GlobalAugmentation => specifier == GLOBAL_AUGMENTATION_TAG,
    }
}

/// Resolver-free, conservative variant of [`augmenter_matches_target`] used
/// for cache INVALIDATION on the artifact lifecycle.
///
/// Exact arms (`ExternalSpecifier` / `WildcardAmbient` / `GlobalAugmentation`)
/// are decided from the fact's specifier alone — no resolver needed. The
/// `ResolvedRelativeCanonical` arm cannot resolve the augmenter's relative
/// specifier without a module resolver (the store layer has none), so it
/// answers CONSERVATIVELY: any relative `declare module "./x"` fact is treated
/// as a potential contributor to EVERY relative-target entry.
///
/// Over-matching here is correctness-safe: an invalidation only DROPS an index
/// entry, and the next [`FileArtifactStore::ensure_augmentation_index_populated`]
/// cold-rescan rebuilds it identically (re-applying the exact, resolver-backed
/// [`augmenter_matches_target`] and the population filter). The only cost of a
/// conservative match is recomputing a relative-target entry that did not
/// strictly need it — and relative `declare module` augmentation is rare. It
/// can NEVER add a wrong augmenter to a set, so overlay isolation and the
/// no-base-poison rule are preserved.
fn augmenter_fact_could_contribute(
    fact: &ModuleAugmentationFact,
    target_key: &AugmentationTargetKey,
) -> bool {
    let specifier: &str = fact.specifier.as_ref();
    // Same relative class as `augmenter_matches_target` — the exact
    // matcher and this conservative invalidation variant must agree on
    // which facts are relative, or a `declare module '..'` fact would
    // be exact-matched as relative but never invalidate relative-target
    // entries.
    let is_relative = verter_workspace::resolver::is_relative_specifier(specifier);
    match &target_key.target {
        AugmentationTargetKind::ExternalSpecifier(target_spec) => {
            let is_wildcard = specifier.contains('*');
            let is_global = specifier == GLOBAL_AUGMENTATION_TAG;
            !is_relative && !is_wildcard && !is_global && specifier == target_spec.as_ref()
        }
        // Conservative: cannot resolve the relative specifier without a
        // module resolver, so any relative fact could target this entry.
        AugmentationTargetKind::ResolvedRelativeCanonical(_) => is_relative,
        AugmentationTargetKind::WildcardAmbient(target_pattern) => {
            specifier.contains('*') && specifier == target_pattern.as_ref()
        }
        AugmentationTargetKind::GlobalAugmentation => specifier == GLOBAL_AUGMENTATION_TAG,
    }
}

/// Compute the stable `AugmenterSet.fingerprint` over the sorted
/// `[(augmenter_canonical, parse_stable_hash)]` list.
///
/// Two FxHasher passes seeded with distinct salts produce a 16-byte
/// hash with low collision risk. Matches the cheap-hash pattern used
/// by `resolver_core::compute_signature_fingerprint`.
pub(crate) fn compute_augmenter_set_fingerprint(entries: &[AugmenterEntry]) -> Hash16 {
    use std::hash::{BuildHasher, Hasher};
    let salt_lo = rustc_hash::FxBuildHasher;
    let salt_hi = rustc_hash::FxBuildHasher;
    let mut h_lo = salt_lo.build_hasher();
    let mut h_hi = salt_hi.build_hasher();
    h_lo.write_u64(0xC4A1_C4A1_4A1C_4A1C);
    h_hi.write_u64(0x9E37_79B9_7F4A_7C15);
    // R29: the fingerprint folds in `(augmenter_canonical,
    // augmenter_parse_stable_hash)` — the `FileArtifactKey`'s other
    // dimensions are deliberately NOT mixed in, so a cosmetic edit
    // (content_hash changes, parse_stable_hash invariant) does not
    // perturb the augmenter-set fingerprint.
    for entry in entries {
        let canon = entry.canonical();
        let hash = &entry.parse_stable_hash;
        h_lo.write(canon.as_bytes());
        h_lo.write(hash);
        h_hi.write(canon.as_bytes());
        h_hi.write(hash);
    }
    let lo = h_lo.finish();
    let hi = h_hi.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

/// Emit a typed `StructuredAuditEvent::ModuleAugmentationIndexShape`
/// event recording an install / refresh of the augmentation-index
/// entry. Silent no-op when no audit accumulator is installed.
pub(crate) fn emit_module_augmentation_index_shape_event(
    key: &AugmentationTargetKey,
    prev_fingerprint: Option<Hash16>,
    new_fingerprint: Hash16,
    augmenter_count: u32,
) {
    use verter_audit::AugmentationTargetKindTag;
    let (tag, external_specifier, resolved_relative_canonical, wildcard_pattern) = match &key.target
    {
        AugmentationTargetKind::ExternalSpecifier(spec) => (
            AugmentationTargetKindTag::ExternalSpecifier,
            Some(Arc::<str>::from(spec.as_ref())),
            None,
            None,
        ),
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => (
            AugmentationTargetKindTag::ResolvedRelativeCanonical,
            None,
            Some(Arc::clone(canon)),
            None,
        ),
        AugmentationTargetKind::WildcardAmbient(pat) => (
            AugmentationTargetKindTag::WildcardAmbient,
            None,
            None,
            Some(Arc::<str>::from(pat.as_ref())),
        ),
        AugmentationTargetKind::GlobalAugmentation => (
            AugmentationTargetKindTag::GlobalAugmentation,
            None,
            None,
            None,
        ),
    };
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::ModuleAugmentationIndexShape {
            target_kind_tag: tag,
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
            prev_fingerprint,
            new_fingerprint,
            augmenter_count,
        },
    );
}

/// Translate a parse-domain [`fact_registry::FactKey`] to its
/// audit-side discriminator [`verter_audit::FactKeyKindTag`].
///
/// Pure translation — does not mint state. Only the parse-domain
/// `FactKey` variants are mirrored on the audit side; the
/// resolve-imports and route-surface domain keys flow through the
/// parallel `ResolvedImportFacts` / `RouteDb` admission paths and
/// emit their own typed events.
pub(crate) fn fact_key_kind_tag_for(key: &fact_registry::FactKey) -> verter_audit::FactKeyKindTag {
    use fact_registry::FactKey;
    use verter_audit::FactKeyKindTag;
    match key {
        FactKey::Export { .. } => FactKeyKindTag::Export,
        FactKey::ExportAlias { .. } => FactKeyKindTag::ExportAlias,
        FactKey::SyntacticExportSet => FactKeyKindTag::SyntacticExportSet,
        FactKey::LocalDecl { .. } => FactKeyKindTag::LocalDecl,
        FactKey::Member { .. } => FactKeyKindTag::Member,
        FactKey::MemberPresence { .. } => FactKeyKindTag::MemberPresence,
        FactKey::MemberShape { .. } => FactKeyKindTag::MemberShape,
        FactKey::MacroSurface { .. } => FactKeyKindTag::MacroSurface,
        FactKey::TemplateRoot => FactKeyKindTag::TemplateRoot,
        FactKey::ImportRef { .. } => FactKeyKindTag::ImportRef,
        FactKey::SyntacticReexportRef { .. } => FactKeyKindTag::SyntacticReexportRef,
        FactKey::ModuleAugmentation { .. } => FactKeyKindTag::ModuleAugmentation,
        FactKey::AugmentationContributionSet { .. } => FactKeyKindTag::AugmentationContributionSet,
        FactKey::AugmentationContributionOrder { .. } => {
            FactKeyKindTag::AugmentationContributionOrder
        }
        FactKey::DeclContributionOrder { .. } => FactKeyKindTag::DeclContributionOrder,
        FactKey::AugmentationTargetSet => FactKeyKindTag::AugmentationTargetSet,
        FactKey::NamespaceScopeSet => FactKeyKindTag::NamespaceScopeSet,
        // Resolve-imports + route-surface domain keys live on the
        // parallel `ResolvedImportFacts` / `RouteDb` admission paths
        // and emit their own typed events. They are not admitted to
        // the `FileFacts.registry` parse-domain inventory, so the
        // unreachable arm flags a producer error if we ever do.
        FactKey::ResolvedImportClause { .. }
        | FactKey::ResolvedReexportBinding { .. }
        | FactKey::ModuleAugmentationIndexShape { .. } => FactKeyKindTag::SyntacticExportSet,
    }
}

/// Emit one typed `StructuredAuditEvent::FactRegistryWrite` per fact
/// admitted to the per-file `FactRegistry` (R10, R11). Cold-path /
/// parse-time only — fires once per fact at the point the host-side
/// `FileArtifactStore` admits a fresh `FileArtifacts` payload.
///
/// The `lane` field is set to `Semantic` (the dominant caching
/// dimension); the parallel `semantic_hash` and `display_hash`
/// fields carry both lane hashes simultaneously.
fn emit_fact_registry_writes(canonical_id: &Arc<str>, fact: &fact_registry::Fact) {
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::FactRegistryWrite {
            canonical_id: Arc::clone(canonical_id),
            fact_key_kind: fact_key_kind_tag_for(&fact.key),
            lane: verter_audit::FactLaneTag::Semantic,
            semantic_hash: fact.semantic_hash,
            display_hash: fact.display_hash,
        },
    );
}

#[cfg(test)]
#[path = "file_artifact_store_tests.rs"]
mod file_artifact_store_tests;

#[cfg(test)]
#[path = "route_surface_generation_tests.rs"]
mod route_surface_generation_tests;
