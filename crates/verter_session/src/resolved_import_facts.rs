//! `ResolvedImportFacts` — resolve-domain import-resolution store.
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
//! resolver_version)`.
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
//!
//! **`lib_env_hash` is intentionally absent.** R21 scoping rule: base
//! import-target resolution does not depend on TS lib data. A change in
//! `lib.dom.d.ts` MUST NOT invalidate `ResolvedImportFacts`. Lib data
//! enters at the route-surface layer (`RouteDb`, materialiser, etc.).
//!
//! # Storage shape
//!
//! The store is one [`ValidatedFactCache`] slot per key: the shared
//! bounded multi-candidate substrate, the standard per-slot
//! [`CANDIDATE_CAP`](crate::resolver_core::CANDIDATE_CAP) FIFO policy,
//! and per-reader [`ReadSetSignature`](verter_workspace::ReadSetSignature)
//! validation. Each candidate roots on the owner's own content
//! (`FileWholeHash`) — the bundle describes exactly one owner's import
//! clauses — and two concurrent resolve envs reading the same parsed
//! file stay in distinct keys.
//!
//! Nothing about resolution CURRENCY lives in the key either. The
//! producer owns an owner's slot: it runs on every fresh route batch
//! and SUPERSEDES its own retained bundle when the batch resolves
//! differently (see
//! `VerterHost::admit_resolved_import_facts_for_owner`), so a
//! re-resolution never needs a fresh key to escape its predecessor and
//! a byte-identical recomputation is skipped rather than churning the
//! slot.
//!
//! Reads go through [`ResolvedImportFactsDb::get_if_valid`] against the
//! caller's own view — there is no unvalidated read of this store.

use std::sync::Arc;

use verter_semantic::analysis::Hash16;
use verter_workspace::FactVersionRef;

use crate::resolver_core::bracketed_generation::BracketedGeneration;
use crate::resolver_core::{StoreView, ValidatedFactCache};
use verter_semantic::facts::registry::{Fact, InternedName, InternedSpecifier, SymbolSpace};

#[cfg(any(test, feature = "test-support"))]
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
///
/// Resolution CURRENCY is deliberately not a key dimension: a
/// re-resolution supersedes the owner's retained bundle in place
/// rather than needing a fresh key to escape the previous entry (see
/// the module docs).
///
/// `lib_env_hash` is NOT a key dimension by design (R21 scoping
/// rule). Tests pin this absence in
/// `crates/verter_session/tests/cases/g_resolved/resolved_import_facts_key_shape.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedImportFactsKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub resolver_version: u32,
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

/// Per-host resolved-import facts store.
///
/// One [`ValidatedFactCache`] slot per key: the shared bounded
/// multi-candidate substrate with the standard
/// [`CANDIDATE_CAP`](crate::resolver_core::CANDIDATE_CAP) FIFO policy
/// and per-reader signature validation. Concurrent resolution states
/// of the same parsed file coexist as candidates and are told apart by
/// the witness each one recorded, not by a key dimension.
///
/// The production producer
/// (`VerterHost::admit_resolved_import_facts_for_owner`) reads the
/// owner's `script_analysis.imports` + admitted route-resolutions,
/// constructs one [`ResolvedImportClauseEntry`] per `(binding, space)`
/// pair, and admits the bundle together with the owner's import-route
/// witness through [`Self::admit`].
#[derive(Debug, Default)]
pub struct ResolvedImportFactsDb {
    entries: ValidatedFactCache<ResolvedImportFactsKey, ResolvedImportFacts>,
    /// Membership generation of the `SemanticImports` compaction domain.
    ///
    /// This store is the domain's SOLE write chokepoint, so one counter
    /// covers it. It is BRACKETED rather than post-incremented: the
    /// window between a candidate entering a slot and a naive counter
    /// moving is exactly a window in which a scope can read the new
    /// membership, re-read the old generation, detect no movement, and
    /// admit a terminal aggregate asserting the domain held at a
    /// generation its facts do not come from. See
    /// [`BracketedGeneration`].
    ///
    /// [`Self::admit`] and [`Self::clear`] are the only mutators of
    /// `entries`, and both run inside the bracket — which is what makes
    /// the private field a rail rather than a convention.
    generation: BracketedGeneration,
    /// Producer-admission provenance counter — positive (resolved)
    /// entries successfully admitted.
    ///
    /// Test-only: counter exists exclusively to discriminate the
    /// producer in DISCRIMINATING tests. Bumped from
    /// `VerterHost::admit_resolved_import_facts_for_owner` after a
    /// successful admission. Snapshot via
    /// [`Self::resolved_import_facts_positive_admissions`].
    #[cfg(any(test, feature = "test-support"))]
    positive_admissions: AtomicU64,
    /// Producer-admission provenance counter — negative (unresolved)
    /// entries admitted as facts so the validator can detect when a
    /// previously unresolved binding becomes resolved.
    #[cfg(any(test, feature = "test-support"))]
    negative_admissions: AtomicU64,
    /// Producer-admission provenance counter — namespace
    /// (`import * as ns from "X"`) entries admitted. Subset of
    /// positive admissions for v8 AMENDMENT-S discrimination.
    #[cfg(any(test, feature = "test-support"))]
    namespace_admissions: AtomicU64,
}

/// Cache-kind label carried on strict admissions so a refused
/// admission is attributable in the audit stream.
pub(crate) const RESOLVED_IMPORT_FACTS_CACHE_KIND: &str = "resolved_import_facts";

impl ResolvedImportFactsDb {
    /// Construct an empty store. Wired into
    /// [`crate::project_type_store::ProjectTypeStore`] at host
    /// construction time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The single read of this store: return the bundle for `key`
    /// whose recorded witness still validates under `view`.
    ///
    /// `None` is either a cold key or a slot whose every candidate went
    /// stale — both mean "recompute through the producer". There is no
    /// unvalidated sibling read: a caller that cannot present a view
    /// cannot serve a resolved-import bundle.
    #[must_use]
    pub fn get_if_valid<TView>(
        &self,
        key: &ResolvedImportFactsKey,
        view: &TView,
    ) -> Option<Arc<ResolvedImportFacts>>
    where
        TView: StoreView + ?Sized,
    {
        self.entries.get_if_valid(key, view)
    }

    /// `true` when ONE retained candidate carries BOTH `facts` as its
    /// witness AND a payload equal to `value`.
    ///
    /// Producer-only: a recomputation that reproduces a retained
    /// candidate WHOLE is pure churn, and re-admitting it would age a
    /// genuinely distinct concurrent candidate out of the bounded slot.
    /// The conjunction is evaluated per candidate over ONE slot load, so
    /// the decision can never be satisfied by two different candidates
    /// and can never straddle a concurrent slot mutation — either would
    /// make the producer drop a `(witness, payload)` pair that nothing
    /// retains.
    #[must_use]
    pub(crate) fn holds_candidate_matching(
        &self,
        key: &ResolvedImportFactsKey,
        facts: &[FactVersionRef],
        value: &ResolvedImportFacts,
    ) -> bool {
        self.entries
            .holds_candidate_with_signature_and_value(key, facts, value)
    }

    /// The payload of the LAST candidate retained for `key`, whatever its
    /// witness. Test-only: fixtures use it to arrange and inspect slot
    /// state. It performs no validation, correlates nothing, and must
    /// never inform a production decision — that is precisely the read
    /// whose uncorrelated use this module's dedupe guard replaced.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn retained_bundle_for_tests(
        &self,
        key: &ResolvedImportFactsKey,
    ) -> Option<Arc<ResolvedImportFacts>> {
        self.entries.lookup_any_candidate(key)
    }

    /// Every retained candidate's witness, in slot order. Test-only slot
    /// inspection.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn candidate_signatures_for_tests(
        &self,
        key: &ResolvedImportFactsKey,
    ) -> Vec<Arc<[FactVersionRef]>> {
        self.entries.candidate_signatures_for_key(key)
    }

    /// Test-support seeding: admit `value` under `facts` unless a
    /// candidate with that exact witness is already retained.
    ///
    /// Fixtures compose `facts` through
    /// [`VerterHost::resolved_import_facts_witness_for`](crate::VerterHost::resolved_import_facts_witness_for)
    /// so a seeded bundle validates under exactly the same rail as a
    /// produced one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_if_absent(
        &self,
        key: ResolvedImportFactsKey,
        value: Arc<ResolvedImportFacts>,
        facts: Vec<FactVersionRef>,
    ) -> bool {
        if self.entries.holds_candidate_with_signature(&key, &facts) {
            return false;
        }
        self.admit(key, value, facts)
    }

    /// Admit a freshly-resolved payload under the witness the producer
    /// observed. Returns `true` when the candidate entered the slot.
    ///
    /// Strict admission: an empty or over-cap witness is refused
    /// (`ReturnOnly`) rather than admitted unrooted.
    ///
    /// The whole insertion runs inside the domain's generation bracket,
    /// and the generation advances for exactly the outcomes that change
    /// what a recorded fact can depend on:
    ///
    /// * a candidate entering the slot ADVANCES — including when it ages
    ///   the oldest candidate out, because eviction happens inside this
    ///   same insertion and so is never an admit-free validity flip;
    /// * a REFUSED admission (empty or over-cap witness) does not — no
    ///   membership moved;
    /// * an identical-candidate SKIP does not reach here at all: the
    ///   producer's dedupe returns before calling this, so a
    ///   recomputation that reproduces a retained candidate whole costs
    ///   no reader their compaction.
    pub(crate) fn admit(
        &self,
        key: ResolvedImportFactsKey,
        value: Arc<ResolvedImportFacts>,
        facts: Vec<FactVersionRef>,
    ) -> bool {
        self.generation.mutate(|| {
            let admitted = self
                .entries
                .insert_arc_with_kind(key, value, facts, RESOLVED_IMPORT_FACTS_CACHE_KIND)
                .is_some();
            (admitted, admitted)
        })
    }

    /// The domain's current stable membership generation, or `None`
    /// while an admission is in flight.
    ///
    /// `None` disarms compaction for the reading scope rather than
    /// guessing — the same fail-safe direction as a domain with no
    /// producer at all.
    #[must_use]
    pub(crate) fn stable_generation(&self) -> Option<u64> {
        self.generation.stable()
    }

    /// Number of occupied slots. Used by tests + diagnostics.
    ///
    /// Mirrors the
    /// [`MaterializeStructureDb::entry_count`](crate::component_meta_caches::MaterializeStructureDb::entry_count)
    /// accessor for symmetric observability.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no slot is occupied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every cached candidate. Used by GC sweeps and test setup.
    ///
    /// Advances the domain generation unconditionally, inside the same
    /// bracket as [`Self::admit`]. A clear removes membership every bit
    /// as much as an admit adds it, so a compacted witness must not
    /// survive one; and unlike admission this is not a hot path, so
    /// there is nothing to gain from distinguishing a clear of an
    /// already-empty store.
    pub fn clear(&self) {
        self.generation.mutate(|| {
            self.entries.clear();
            ((), true)
        });
    }

    /// Bump the positive-admission provenance counter. Called by the
    /// production producer
    /// (`VerterHost::admit_resolved_import_facts_for_owner`) after
    /// each per-binding positive entry it constructs and admits.
    ///
    /// Test-only — the snapshot accessor
    /// [`Self::resolved_import_facts_positive_admissions`] reads
    /// this counter from discriminating tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn record_positive_admission(&self) {
        self.positive_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the negative-admission provenance counter. Called by the
    /// production producer after each per-binding negative
    /// (unresolved) entry it admits.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn record_negative_admission(&self) {
        self.negative_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the namespace-admission provenance counter. Called by
    /// the production producer when the admitted entry's `space`
    /// is [`SymbolSpace::Namespace`].
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn record_namespace_admission(&self) {
        self.namespace_admissions.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot the positive-admission provenance counter (relaxed
    /// load — counter is a discriminator, not a synchronisation
    /// primitive). Used by `discriminating tests to verify the
    /// production producer ran (delta > 0 against pre-state).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn resolved_import_facts_positive_admissions(&self) -> u64 {
        self.positive_admissions.load(Ordering::Relaxed)
    }

    /// Snapshot the negative-admission provenance counter.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn resolved_import_facts_negative_admissions(&self) -> u64 {
        self.negative_admissions.load(Ordering::Relaxed)
    }

    /// Snapshot the namespace-admission provenance counter (subset
    /// of positive admissions where `space == SymbolSpace::Namespace`).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn resolved_import_facts_namespace_admissions(&self) -> u64 {
        self.namespace_admissions.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
#[path = "resolved_import_facts_generation_tests.rs"]
mod resolved_import_facts_generation_tests;
