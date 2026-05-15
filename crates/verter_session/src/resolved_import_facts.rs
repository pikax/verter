//! `ResolvedImportFacts` — resolve-domain import-resolution cache.
//!
//! The resolve-domain authoritative store for resolved import / re-export
//! bindings (`FactKey::ResolvedImportClause`,
//! `FactKey::ResolvedReexportBinding`) plus per-specifier import-target
//! resolutions. Populated when a file's resolve-env resolution runs;
//! consumers re-read from here instead of re-walking the AST.
//!
//! # Key composition (R5, R12, R21)
//!
//! [`ResolvedImportFactsKey`] is content-addressed and scoped to:
//! `(canonical, content_hash, parse_env_hash, resolve_env_hash,
//! resolver_version, known_miss_generation)`.
//!
//! - `content_hash` (R5): two parses of the same source coexist; an
//!   edit re-keys the entry.
//! - `parse_env_hash`: parser-flag changes (TS-syntax mode, JSX, etc.)
//!   isolate the cache slot.
//! - `resolve_env_hash`: `paths` aliases, `baseUrl`, resolution
//!   extensions — anything that changes the dependency-target map for
//!   a given specifier.
//! - `resolver_version`: substrate bump invalidates (R28). Bumped when
//!   the resolved-import producer changes shape.
//! - `known_miss_generation`: stable tag over the owner's
//!   `DerivedRawState::import_routes_known_miss_recorded_at_generation`
//!   sidecar. When `set_import_dependencies` is called again for the
//!   same owner whose source/env did not change but a previously-
//!   missing target file has now been created, the workspace
//!   `content_generation` advances and the producer admits under a
//!   new key value — the stale negative bundle is naturally
//!   superseded (rather than being pinned by first-writer-wins
//!   admission on the prior key). Empty known-miss map → `[0u8; 16]`.
//!
//! **`lib_env_hash` is intentionally absent.** R21 scoping rule: base
//! import-target resolution does not depend on TS lib data. A change in
//! `lib.dom.d.ts` MUST NOT invalidate `ResolvedImportFacts`. Lib data
//! enters at the route-surface layer (`RouteDb`, materialiser, etc.).
//!
//! # Storage shape
//!
//! Two concurrent resolve envs reading the same parsed file coexist as
//! two entries in [`ResolvedImportFactsDb`]. The store is
//! [`DashMap`]-backed; shards split on the key so per-key access is
//! wait-free under contention. Admission is first-writer-wins
//! ([`ResolvedImportFactsDb::insert_if_absent`]) so concurrent
//! recomputations on the same key collapse to one `Arc` value.

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::registry::{Fact, InternedName, InternedSpecifier, SymbolSpace};

#[cfg(any(test, debug_assertions))]
use std::sync::atomic::{AtomicU64, Ordering};

/// Substrate version for [`ResolvedImportFactsDb`] entries.
///
/// Bumped whenever the resolved-import producer changes the value
/// shape (e.g., a new field on [`ResolvedImportFacts`], a change to
/// how per-specifier resolutions are encoded, or a fix to the
/// resolver routing rules). Cache entries with a stale
/// `resolver_version` cannot be served because their value shape no
/// longer satisfies the consumer contract.
pub const RESOLVED_IMPORT_FACTS_RESOLVER_VERSION: u32 = 1;

/// Cache key for [`ResolvedImportFacts`].
///
/// Identity is the conjunction of:
///
/// - `canonical` — the file the resolved-import facts describe.
/// - `content_hash` — source-byte identity at production time.
/// - `parse_env_hash` — parser-flag identity (one of the five
///   `EnvHashes` dimensions).
/// - `resolve_env_hash` — resolution-config identity (paths,
///   baseUrl, resolution extensions, package conditions).
/// - `resolver_version` — substrate version (see
///   [`RESOLVED_IMPORT_FACTS_RESOLVER_VERSION`]).
/// - `known_miss_generation` — stable 16-byte tag derived from the
///   owner's
///   [`DerivedRawState::import_routes_known_miss_recorded_at_generation`](crate::types::DerivedRawState)
///   map via [`compute_known_miss_generation_tag`]. Empty map →
///   `[0u8; 16]`. Lets a later `set_import_dependencies` call that
///   re-resolves a previously-missing specifier (after the target
///   file is created and the workspace `content_generation` has
///   advanced) admit under a NEW key instead of being silently
///   discarded by `insert_if_absent` against a stale negative entry.
///   Both producer and validator/lookup must read the SAME sidecar
///   map and derive the same tag for cache-key determinism.
///
/// `lib_env_hash` is NOT a key dimension by design (R21 scoping
/// rule). Tests pin this absence in
/// `crates/verter_session/tests/resolved_import_facts_key_shape.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedImportFactsKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub resolver_version: u32,
    pub known_miss_generation: Hash16,
}

/// Compute a stable 16-byte tag from the owner's
/// `import_routes_known_miss_recorded_at_generation` sidecar.
///
/// Used by both the resolved-import-facts producer
/// ([`crate::VerterHost::admit_resolved_import_facts_for_owner`])
/// and the validator/lookup sites
/// ([`crate::resolver_store::HostStoreView::validates_resolve_imports_domain`],
/// [`crate::session_view::SessionView::resolved_import_facts`])
/// when composing [`ResolvedImportFactsKey`]. Determinism (and
/// therefore cache-key reachability between producer and lookup)
/// requires:
///
/// 1. Sort `(specifier, generation)` pairs lexicographically by
///    specifier so iteration order of the underlying `FxHashMap` is
///    not observable in the tag.
/// 2. Use `[u8; 16]` `xxh3_128` (via [`crate::hash::hash_16`]) so the
///    tag width matches the rest of the key's `Hash16` fields.
/// 3. Empty map → `[0u8; 16]` so the owner with no known-misses
///    composes the SAME tag value at producer time and at lookup
///    time, regardless of whether the `DerivedRawState` entry exists
///    yet.
#[must_use]
pub fn compute_known_miss_generation_tag(
    known_miss_generations: &rustc_hash::FxHashMap<String, u64>,
) -> Hash16 {
    if known_miss_generations.is_empty() {
        return [0u8; 16];
    }
    let mut pairs: Vec<(&String, &u64)> = known_miss_generations.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut buf: Vec<u8> = Vec::with_capacity(pairs.len() * 32);
    for (specifier, generation) in pairs {
        buf.push(0xFE);
        buf.extend_from_slice(specifier.as_bytes());
        buf.push(0xFD);
        buf.extend_from_slice(&generation.to_le_bytes());
    }
    crate::hash::hash_16(&buf)
}

/// One per-specifier resolution entry.
///
/// Captures the resolved canonical target for an import / re-export
/// specifier under a given symbol space (`Type` / `Value` /
/// `Namespace`). The `resolved_source_name` is the binding name as
/// seen in the resolved file's export surface — distinct from the
/// importing file's local-binding name, which lives on
/// [`ResolvedImportClauseEntry`] / [`ResolvedReexportBindingEntry`].
///
/// `Send + Sync` because every payload field is `Arc`-owned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedSpecifier {
    /// The literal specifier as written in source (`"./util"`,
    /// `"vue"`, etc.). Interned via [`InternedSpecifier`] so two
    /// importers reaching the same specifier reuse the same heap
    /// allocation.
    pub specifier: InternedSpecifier,
    /// Resolved canonical target. `None` when the specifier could
    /// not be resolved to a canonical (unresolved external,
    /// permission denied, etc.) — consumers must short-circuit on
    /// `None` rather than fabricating a target.
    pub resolved_canonical: Option<Arc<str>>,
    /// Symbol space the resolution applies to. `Type` and `Value`
    /// resolutions are distinct entries.
    pub space: SymbolSpace,
}

/// One resolved import-clause binding.
///
/// Mirrors the payload of [`verter_semantic::facts::FactKey::ResolvedImportClause`]:
/// `(specifier, binding, space, resolved_canonical,
/// resolved_source_name)` — the resolver's claim that a particular
/// `import { binding } from "spec"` (or default / namespace import)
/// resolves to a specific source-name in a specific canonical file.
///
/// Owned `Send + Sync` (every field is `Arc`-owned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImportClauseEntry {
    /// Imported specifier (`"vue"`, `"./util"`, etc.).
    pub specifier: InternedSpecifier,
    /// Local binding name as written in the importing file
    /// (`import { X as Y }` → binding `Y`).
    pub binding: InternedName,
    /// Symbol space the binding occupies.
    pub space: SymbolSpace,
    /// Resolved canonical target — `None` for unresolved externals.
    pub resolved_canonical: Option<Arc<str>>,
    /// Resolved source-name at the target file (`import { X as Y }`
    /// → source-name `X`).
    pub resolved_source_name: InternedName,
    /// Cached fact pair (`semantic_hash`, `display_hash`) so the
    /// route-surface layer can record a dep-signature without
    /// re-walking the resolver.
    pub fact: Arc<Fact>,
}

/// One resolved re-export-binding entry.
///
/// Mirrors the payload of [`verter_semantic::facts::FactKey::ResolvedReexportBinding`]:
/// `(specifier, source_name, target_name, space, resolved_canonical,
/// resolved_source_name)` — the resolver's claim that
/// `export { source_name as target_name } from "specifier"` reaches a
/// specific source-name in a specific canonical file.
///
/// Owned `Send + Sync` (every field is `Arc`-owned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReexportBindingEntry {
    /// Re-exported specifier.
    pub specifier: InternedSpecifier,
    /// Source-name in the re-exporting file
    /// (`export { source_name as target_name }`).
    pub source_name: InternedName,
    /// Locally-published target-name in the re-exporting file
    /// (`export { source_name as target_name }`).
    pub target_name: InternedName,
    /// Symbol space the binding occupies.
    pub space: SymbolSpace,
    /// Resolved canonical target — `None` for unresolved externals.
    pub resolved_canonical: Option<Arc<str>>,
    /// Resolved source-name at the target file (the binding the
    /// re-export ultimately reaches).
    pub resolved_source_name: InternedName,
    /// Cached fact pair.
    pub fact: Arc<Fact>,
}

/// Per-file resolved-import payload.
///
/// Owns the resolved import-clause and re-export-binding lists plus
/// the per-specifier resolution map for the file. Consumers
/// (`RouteDb`, materialiser, etc.) read directly from these vectors
/// instead of re-walking the AST.
///
/// Every field is owned (`Arc<...>` or `Vec<owned>`). No borrows, no
/// parser arenas, no back-references — safe for long-lived
/// host-owned caches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedImportFacts {
    /// Resolved import-clause entries for this file.
    pub import_clauses: Vec<ResolvedImportClauseEntry>,
    /// Resolved re-export-binding entries for this file.
    pub reexport_bindings: Vec<ResolvedReexportBindingEntry>,
    /// Per-specifier resolutions. Keyed by `(specifier, space)` so
    /// `Type` and `Value` resolutions for the same specifier coexist.
    pub specifier_resolutions: Vec<ResolvedSpecifier>,
}

impl ResolvedImportFacts {
    /// Construct an empty `ResolvedImportFacts` payload. The
    /// resolver populates the vectors before admission.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Per-host resolved-import facts cache.
///
/// Sharded by key via [`DashMap`]; concurrent readers for different
/// keys are wait-free. Admission is via
/// [`Self::insert_if_absent`] (first-writer-wins). The store hands
/// out `Arc<ResolvedImportFacts>` so consumers cheaply clone
/// references without re-validating.
///
/// The production producer
/// (`VerterHost::admit_resolved_import_facts_for_owner`) reads the
/// owner's `script_analysis.imports` + admitted route-resolutions
/// and constructs one [`ResolvedImportClauseEntry`] per
/// `(binding, space)` pair, then admits the bundle through
/// [`Self::insert_if_absent`].
#[derive(Debug, Default)]
pub struct ResolvedImportFactsDb {
    entries: DashMap<ResolvedImportFactsKey, Arc<ResolvedImportFacts>>,
    /// Producer-admission provenance counter — positive (resolved)
    /// entries successfully admitted (first-writer-wins).
    ///
    /// Test-only: counter exists exclusively to discriminate the
    /// producer in DISCRIMINATING tests. Bumped from
    /// `VerterHost::admit_resolved_import_facts_for_owner` after a
    /// successful `insert_if_absent`. Snapshot via
    /// [`Self::positive_admissions`].
    #[cfg(any(test, debug_assertions))]
    positive_admissions: AtomicU64,
    /// Producer-admission provenance counter — negative (unresolved)
    /// entries admitted as facts so the validator can detect when a
    /// previously unresolved binding becomes resolved on workspace
    /// bump.
    #[cfg(any(test, debug_assertions))]
    negative_admissions: AtomicU64,
    /// Producer-admission provenance counter — namespace
    /// (`import * as ns from "X"`) entries admitted. Subset of
    /// positive admissions for v8 AMENDMENT-S discrimination.
    #[cfg(any(test, debug_assertions))]
    namespace_admissions: AtomicU64,
}

impl ResolvedImportFactsDb {
    /// Construct an empty cache. Wired into
    /// [`crate::project_type_store::ProjectTypeStore`] at host
    /// construction time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup a resolved-import-facts entry by full key. `None` is
    /// a cold miss — the caller computes the resolved facts and
    /// admits the entry via [`Self::insert_if_absent`].
    #[must_use]
    pub fn get(&self, key: &ResolvedImportFactsKey) -> Option<Arc<ResolvedImportFacts>> {
        self.entries.get(key).map(|v| Arc::clone(&*v))
    }

    /// Admit a freshly-resolved payload. Returns `true` when the
    /// caller's entry won the admission race, `false` when an
    /// existing entry was already present.
    ///
    /// First-writer-wins semantics: an identical key MUST be a
    /// deterministic recomputation, so the existing entry is
    /// preserved to keep `Arc` identity stable for consumers
    /// already holding a clone.
    pub fn insert_if_absent(
        &self,
        key: ResolvedImportFactsKey,
        value: Arc<ResolvedImportFacts>,
    ) -> bool {
        let mut admitted = false;
        self.entries.entry(key).or_insert_with(|| {
            admitted = true;
            value
        });
        admitted
    }

    /// Number of cached entries. Used by tests + diagnostics.
    ///
    /// Mirrors the
    /// [`MaterializeStructureDb::entry_count`](crate::component_meta_caches::MaterializeStructureDb::entry_count)
    /// accessor for symmetric observability.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every cached entry. Used by GC sweeps and test setup.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Bump the positive-admission provenance counter. Called by the
    /// production producer
    /// (`VerterHost::admit_resolved_import_facts_for_owner`) after
    /// each per-binding positive entry it constructs and admits.
    ///
    /// Test-only — the snapshot accessor
    /// [`Self::resolved_import_facts_positive_admissions`] reads
    /// this counter from discriminating tests.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn record_positive_admission(&self) {
        self.positive_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the negative-admission provenance counter. Called by the
    /// production producer after each per-binding negative
    /// (unresolved) entry it admits.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn record_negative_admission(&self) {
        self.negative_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the namespace-admission provenance counter. Called by
    /// the production producer when the admitted entry's `space`
    /// is [`SymbolSpace::Namespace`].
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn record_namespace_admission(&self) {
        self.namespace_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot the positive-admission provenance counter (relaxed
    /// load — counter is a discriminator, not a synchronisation
    /// primitive). Used by `discriminating tests to verify the
    /// production producer ran (delta > 0 against pre-state).
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn resolved_import_facts_positive_admissions(&self) -> u64 {
        self.positive_admissions.load(Ordering::Relaxed)
    }

    /// Snapshot the negative-admission provenance counter.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn resolved_import_facts_negative_admissions(&self) -> u64 {
        self.negative_admissions.load(Ordering::Relaxed)
    }

    /// Snapshot the namespace-admission provenance counter (subset
    /// of positive admissions where `space == SymbolSpace::Namespace`).
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn resolved_import_facts_namespace_admissions(&self) -> u64 {
        self.namespace_admissions.load(Ordering::Relaxed)
    }
}
