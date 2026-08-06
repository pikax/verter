//! Push-style fact-read tracer used by cold-compute paths.
//!
//! ## What this module owns
//!
//! - [`FactReadSet`] — `!Send + !Sync` accumulator that collects
//!   [`FactVersionRef`] observations made during one cold compute and
//!   produces an immutable `Arc<[FactVersionRef]>` ready to install as
//!   a `Candidate::fact_dep_signature`.
//! - [`FactReadSetCell`] — interior-mutability wrapper so trait
//!   methods can record observations through `&self`.
//! - [`FactReadSetFinalise`] — the result of finalising a tracer:
//!   either the immutable signature (`Ok`) or a bounded-overflow
//!   sentinel (`Overflow`) when the observation count exceeds
//!   [`FACT_SIGNATURE_CAP`].
//!
//! ## Push vs pull
//!
//! Today the host already exposes a pull-style helper:
//! [`ResolverContext::current_dependency_fact_versions`] re-walks the
//! observed dependency set after a compute completes. That helper is
//! suitable for tests and offline observability, but it forces the
//! caller to know the dep set up-front and re-reads each fact a
//! second time.
//!
//! The push-style tracer here records facts as they are read on the
//! cold path. The caller installs a tracer scope, every reader
//! observes through [`ResolverContext::observe`] /
//! [`ResolverContext::observe_borrowed_signature`] as it walks the
//! graph, and the tracer accumulates an exact, ordered, deduplicated
//! list of every fact the compute actually consumed.
//!
//! ## Bounded by `FACT_SIGNATURE_CAP`
//!
//! Finalisation enforces the same cap the warm-validation path
//! enforces: at most [`FACT_SIGNATURE_CAP`] (= 1024) facts per
//! candidate. Tracers that exceed the cap return
//! [`FactReadSetFinalise::Overflow`]; callers admit the result as
//! non-cacheable and emit a structured audit event via the
//! per-domain admission path (the event surface itself is wired by a
//! separate fact-signature-overflow audit hookup; this module only
//! reports overflow back to the caller).
//!
//! ## R24 zero-allocation guarantee on the warm-hit path
//!
//! The default-impl [`ResolverContext::observe`] (in
//! `resolver_context.rs`) reads `current_fact_tracer()`. On the
//! warm-hit path no tracer is installed; `current_fact_tracer()`
//! returns `None`; the convenience method falls through with no
//! heap activity. The tracer itself only allocates after the first
//! observation — a `SmallVec` inline capacity of 16 covers the
//! overwhelming majority of real-world cold computes (typical
//! `fact_dep_signature` lengths land between 4 and 12).
//!
//! ## Single-thread, single-compute lifetime
//!
//! A [`FactReadSetCell`] is `!Send + !Sync` by construction: it
//! cannot leak across a task boundary even by accident. The
//! installer lives on the calling thread for the duration of one
//! cold compute. This makes the tracer a true per-compute substrate,
//! not a shared accumulator that readers race on.
//!
//! ## Nesting is supported
//!
//! Installations NEST: the installer pushes onto a per-thread tracer
//! STACK, and every observation fans out to ALL active cells (see
//! `resolver_context`'s fan-out chokepoints). An inner cold compute
//! therefore records its facts into its own cell AND into every
//! enclosing one, so an outer compute's observation set stays complete
//! while the inner one can make its OWN admission decision (a nested
//! non-cacheable read or a nested signature overflow refuses the inner
//! entry without silently laundering into the outer signature). The
//! per-cell `!Send + !Sync` lifetime above is unchanged — the stack is
//! thread-local, and each cell still belongs to exactly one compute.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::fact_cache::{
    compaction_domain, AggregateGenerations, AggregatePopulation, CompactionDomain,
    DomainGenerationFact, FactVersionRef, ViewPopulation,
};
use crate::resolution_currency::ResolutionPopulation;

pub const FACT_SIGNATURE_CAP: usize = 1_024;

/// Per-domain precision threshold: a compaction domain stays PRECISE while
/// its deduplicated bucket holds at most this many facts, and lifts to its
/// terminal aggregate at the first fact beyond it.
///
/// Deliberately the same number as [`FACT_SIGNATURE_CAP`], and read with
/// the same `>` comparison, so "the size at which a single-domain
/// observation set used to be refused" and "the size at which that domain
/// now compacts" are the same boundary rather than two that can drift.
pub const FACT_DOMAIN_PRECISE_MAX: usize = FACT_SIGNATURE_CAP;

/// The population `fact`'s bucket is keyed by, and that an aggregate
/// minted for that bucket speaks for. `None` when nothing in scope can
/// name one — such a bucket never mints.
///
/// Exhaustive over [`CompactionDomain`] on purpose: this is the whole
/// translation, and a new domain must state where its population comes
/// from before it compiles. The three answers are not interchangeable.
///
/// * **[`CompactionDomain::Resolution`] answers from the BUCKET.** Its
///   precise facts carry a population in their own keys, so its buckets
///   partition themselves and a base and a session bucket can coexist in
///   one signature.
/// * **[`CompactionDomain::WorkspaceShape`] is GLOBAL.**
///   `ProjectGeneration` moves only on a project-shape change, which no
///   per-canonical overlay shadows, so its aggregate is base-scoped even
///   inside an overlay-bearing scope — which is what lets a session scope
///   and a base scope share one workspace-shape witness rather than each
///   minting a private copy.
/// * **The remaining four answer from the VIEW.** Their facts carry no
///   population, so the only honest source is the view that validated
///   them. A session overlay re-roots whole hashes and parse facts while
///   leaving the workspace content generation untouched, so an
///   overlay-derived aggregate labelled `Base` would satisfy a base read
///   and stale-serve overlay content. Absent a supplied view population
///   the answer is `None` and the bucket stays precise — never a
///   fallback, because a fallback is exactly the stale-serve.
fn aggregate_population(
    fact: &FactVersionRef,
    basis: &AggregateGenerations,
) -> Option<AggregatePopulation> {
    // An aggregate already names its own population, and must bucket with
    // exactly the precise facts it replaced.
    if let FactVersionRef::DomainGeneration(aggregate) = fact {
        return Some(aggregate.population);
    }
    match compaction_domain(fact) {
        CompactionDomain::Resolution => {
            resolution_population_of(fact).map(AggregatePopulation::Resolution)
        }
        CompactionDomain::WorkspaceShape => Some(AggregatePopulation::View(ViewPopulation::Base)),
        CompactionDomain::Content
        | CompactionDomain::SourceEnv
        | CompactionDomain::SemanticImports
        | CompactionDomain::RouteSurface => basis.view_population.map(AggregatePopulation::View),
    }
}

/// The population carried by a precise RESOLUTION-domain fact's own key.
///
/// `None` is unreachable for a fact [`compaction_domain`] classified as
/// [`CompactionDomain::Resolution`] — that classification is exactly the
/// `ResolveImports(Resolution(_))` arm. It is expressed as an `Option`
/// rather than a panic so a future reshaping of the variant degrades to
/// "this bucket does not mint" instead of aborting a compute.
fn resolution_population_of(fact: &FactVersionRef) -> Option<ResolutionPopulation> {
    match fact {
        FactVersionRef::ResolveImports(inner) => {
            inner.resolution_fact().map(|fact| fact.key.population())
        }
        _ => None,
    }
}

/// Lift every over-threshold domain in a CANONICAL (sorted, deduplicated)
/// observation set to that domain's terminal aggregate, leaving every
/// other domain precise.
///
/// Three properties this must preserve, and the reasons:
///
/// * **Domain-wise, never whole-signature.** One domain outgrowing its
///   bucket must not cost the precision of the others, or a single wide
///   resolution surface would coarsen an entry's content dependency and
///   destroy warm reuse across unrelated edits.
/// * **One aggregate per represented population.** A bucket that mixes
///   populations lifts to one aggregate per population, never to a single
///   aggregate that silently speaks for both.
/// * **No producer, no compaction.** A domain whose generation is absent
///   from `basis` stays precise. Minting an aggregate with no live
///   producer would create a witness nothing can ever invalidate.
/// * **No population, no compaction.** A bucket whose population nothing
///   in scope can name stays precise too, for the mirror-image reason: an
///   aggregate that cannot say *for whom* the domain held is a witness
///   the wrong view can satisfy. See [`aggregate_population`].
///
/// A domain lifts for one of two reasons, and BOTH must hold the "no
/// regrow" property:
///
/// * its precise bucket outgrew [`FACT_DOMAIN_PRECISE_MAX`] and the basis
///   can name a generation for it, or
/// * an aggregate for it is ALREADY present — absorbed from a reused warm
///   candidate that lifted it earlier. Its precise facts collapse into
///   that aggregate no matter how few they are: the aggregate makes the
///   strictly stronger claim ("the whole domain held as of this
///   generation"), so keeping precise siblings beside it would let the
///   bucket regrow one reuse at a time and re-approach the bound the
///   lifting existed to remove.
///
/// Returns `true` when at least one domain was lifted.
fn compact_domains(facts: &mut Vec<FactVersionRef>, basis: &AggregateGenerations) -> bool {
    /// A bucket whose population is `None` is a real bucket — its facts
    /// still group and still survive together — it simply can never mint.
    type BucketKey = (CompactionDomain, Option<AggregatePopulation>);
    let mut precise: rustc_hash::FxHashMap<BucketKey, usize> = rustc_hash::FxHashMap::default();
    let mut already_lifted: rustc_hash::FxHashSet<BucketKey> = rustc_hash::FxHashSet::default();
    for fact in facts.iter() {
        if matches!(fact, FactVersionRef::StrictSelfRootWorld(_)) {
            continue;
        }
        let key = (compaction_domain(fact), aggregate_population(fact, basis));
        if matches!(fact, FactVersionRef::DomainGeneration(_)) {
            already_lifted.insert(key);
        } else {
            *precise.entry(key).or_insert(0) += 1;
        }
    }
    // Mint a fresh aggregate only where the threshold was crossed AND the
    // basis can name a generation AND the bucket has a population. No
    // producer, no aggregate; no population, no aggregate.
    let mint: rustc_hash::FxHashSet<(CompactionDomain, AggregatePopulation)> = precise
        .iter()
        .filter(|(_, count)| **count > FACT_DOMAIN_PRECISE_MAX)
        .filter(|(key, _)| !already_lifted.contains(*key))
        .filter_map(|((domain, population), _)| Some((*domain, (*population)?)))
        .filter(|(domain, _)| basis.stamp_for(*domain).is_some())
        .collect();
    if mint.is_empty() && already_lifted.is_empty() {
        return false;
    }
    let lifted: rustc_hash::FxHashSet<BucketKey> = mint
        .iter()
        .map(|(domain, population)| (*domain, Some(*population)))
        .chain(already_lifted)
        .collect();
    let mut kept: Vec<FactVersionRef> = Vec::with_capacity(facts.len());
    for fact in facts.drain(..) {
        if matches!(fact, FactVersionRef::StrictSelfRootWorld(_)) {
            kept.push(fact);
            continue;
        }
        let key = (compaction_domain(&fact), aggregate_population(&fact, basis));
        // Existing aggregates survive; precise facts in a lifted bucket do
        // not.
        if matches!(fact, FactVersionRef::DomainGeneration(_)) || !lifted.contains(&key) {
            kept.push(fact);
        }
    }
    for (domain, population) in mint {
        let stamp = basis
            .stamp_for(domain)
            .expect("filtered above: only domains with a live stamp mint an aggregate");
        kept.push(FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain,
            population,
            stamp,
        }));
    }
    kept.sort_unstable_by(compare_fact_refs);
    kept.dedup();
    *facts = kept;
    true
}

/// Inline capacity for the observation accumulator. Empirically most
/// computes observe between 4 and 12 facts. The inline capacity is
/// sized to cover those without allocating; longer computes spill to
/// the heap exactly once.
const INLINE_CAPACITY: usize = 16;

/// How a cache-refusal signal propagates through nested cold-compute scopes.
///
/// `LocalOnly` refuses retention for the scope that owns the admission
/// decision while leaving an enclosing compute free to root its own result.
/// `Transitive` identifies an unsafe derivation basis: every enclosing scope
/// that consumes the value must refuse admission too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonCacheablePropagation {
    LocalOnly,
    Transitive,
}

/// Push-style accumulator for cold-compute fact reads.
///
/// Construct via [`FactReadSet::new`], record observations via
/// [`FactReadSet::observe`] / [`FactReadSet::observe_borrowed_signature`],
/// and seal via [`FactReadSet::finalise`].
///
/// Bounded by [`FACT_SIGNATURE_CAP`]: finalisation returns
/// [`FactReadSetFinalise::Overflow`] when the deduplicated signature
/// exceeds the cap.
///
/// `!Send + !Sync` by design: the tracer is per-compute, per-thread
/// state and must never cross a task boundary. The `PhantomData<*const ()>`
/// marker enforces this at compile time.
pub struct FactReadSet {
    observations: SmallVec<[FactVersionRef; INLINE_CAPACITY]>,
    /// Already-canonical signatures absorbed wholesale from a completed
    /// compute (see [`FactReadSet::absorb_canonical_signature`]). Kept as
    /// separate strictly-increasing runs instead of being splatted into
    /// `observations`, so finalisation MERGES them in `O(n)` instead of
    /// re-sorting a set that was already sorted once. Every run in here has
    /// been verified strictly increasing at insertion.
    canonical_runs: Vec<Arc<[FactVersionRef]>>,
    /// TRUE when a NON-CACHEABLE read was consumed inside this tracer's
    /// scope. The class is: a FENCED (ReturnOnly, `store_published ==
    /// false`) `IndexedReady` serve; a broken decl-body lease
    /// (`DemandOutcome` / `PreparedDeclOutcome` / `LocatorBodyDerefError`
    /// `LeaseMiss`); an unrootable / unadmitted import route; an
    /// unobservable contributor source-env identity. Set through the
    /// fan-out marking chokepoint (`note_non_cacheable_read_fan_out`);
    /// consumers refuse shared-cache admission for a result whose compute
    /// consumed such a read — the result's fact stamps are read from the
    /// LIVE post-mutation state while its payload was computed from a
    /// superseded / unrootable / transient basis the read-side fact rail
    /// cannot reject. Orthogonal to completeness: such a result stays
    /// `Complete` and flows to the caller; ONLY memo/cache admission is
    /// refused.
    /// Strongest refusal propagation observed by this scope. `Option` keeps
    /// the state allocation-free while preserving the distinction that a
    /// boolean erased. `Transitive` monotonically dominates `LocalOnly`.
    non_cacheable_propagation: Option<NonCacheablePropagation>,
    /// Live generation of each compaction domain, supplied by whoever
    /// installed this tracer. A domain absent from the basis never
    /// compacts — see [`compact_domains`].
    aggregate_basis: AggregateGenerations,
    /// TRUE once a domain THIS scope compacts against was observed to
    /// have advanced since its basis was installed. Sticky — a scope
    /// cannot become stable again, and cannot exempt itself from a
    /// generation it moved.
    mutation_unstable: bool,
    _not_send_sync: PhantomData<*const ()>,
}

impl Default for FactReadSet {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FactReadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactReadSet")
            .field("len", &self.len())
            .field("non_cacheable_propagation", &self.non_cacheable_propagation)
            .finish()
    }
}

impl FactReadSet {
    /// Construct an empty tracer.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            observations: SmallVec::new(),
            canonical_runs: Vec::new(),
            non_cacheable_propagation: None,
            aggregate_basis: AggregateGenerations::default(),
            mutation_unstable: false,
            _not_send_sync: PhantomData,
        }
    }

    /// Supply the live per-domain generations — and the view population —
    /// this scope compacts against.
    ///
    /// Monotonic in coverage: a later call may only ADD domains, never
    /// retract one, so a nested installer cannot silently disable an
    /// enclosing scope's compaction. Re-supplying a domain overwrites its
    /// generation, which is what a re-read of a live counter must do.
    ///
    /// `view_population` merges by the same rule, so a scope that was
    /// installed without one can still be given one. Supplying a
    /// DIFFERENT one to a scope that already has facts is not meaningful —
    /// a scope validates against one view — and preventing it belongs to
    /// basis installation rather than here; nothing in the tree does it
    /// today (the sole production supplier calls this once on a fresh
    /// tracer).
    pub fn set_aggregate_basis(&mut self, basis: AggregateGenerations) {
        let current = &mut self.aggregate_basis;
        current.content = basis.content.or(current.content);
        current.source_env = basis.source_env.or(current.source_env);
        current.semantic_imports = basis.semantic_imports.or(current.semantic_imports);
        current.resolution = basis.resolution.or(current.resolution);
        current.route_surface = basis.route_surface.or(current.route_surface);
        current.workspace_shape = basis.workspace_shape.or(current.workspace_shape);
        current.view_population = basis.view_population.or(current.view_population);
    }

    /// The basis this scope compacts against, for a caller that needs to
    /// know whether re-reading the live generations is worth anything.
    #[must_use]
    pub fn aggregate_basis(&self) -> &AggregateGenerations {
        &self.aggregate_basis
    }

    /// Re-read the live per-domain generations and record MUTATION
    /// INSTABILITY if any domain this scope compacts against has moved
    /// since its basis was installed.
    ///
    /// Called at every ADMISSION BOUNDARY, not only at finalisation. A
    /// cacheability scope can authorise writes from inside its own
    /// closure, so an exit-only check runs after the write it was meant
    /// to gate.
    ///
    /// There is no "this trace caused the bump, so ignore it" exception,
    /// and one must not be added. A trace cannot exempt itself from a
    /// generation it moved: its own observations were made on both sides
    /// of the mutation, and nothing in the finalised set records which.
    ///
    /// Sticky, and terminal: an unstable scope never becomes stable
    /// again and is never retried automatically.
    pub fn note_basis_recheck(&mut self, live: &AggregateGenerations) {
        if self.mutation_unstable {
            return;
        }
        self.mutation_unstable = self.aggregate_basis.any_named_domain_moved(live);
    }

    /// TRUE when a domain this scope compacts against was observed to
    /// have advanced since its basis was installed.
    #[inline]
    #[must_use]
    pub fn mutation_unstable(&self) -> bool {
        self.mutation_unstable
    }

    /// Record that a NON-CACHEABLE read (fenced serve, broken decl-body
    /// lease, unrootable route, unobservable source-env) was consumed
    /// inside this tracer's scope. Monotonic — never cleared.
    #[inline]
    pub fn note_non_cacheable_read(&mut self, propagation: NonCacheablePropagation) {
        if self.non_cacheable_propagation != Some(NonCacheablePropagation::Transitive) {
            self.non_cacheable_propagation = Some(propagation);
        }
    }

    /// TRUE when any non-cacheable read was consumed inside this tracer's
    /// scope.
    #[inline]
    #[must_use]
    pub fn non_cacheable_read_observed(&self) -> bool {
        self.non_cacheable_propagation.is_some()
    }

    /// Record one observed fact.
    ///
    /// Same-fact adjacent duplicates are short-circuited inline; the
    /// finalisation pass performs the full sort + dedup across all
    /// observations.
    #[inline]
    pub fn observe(&mut self, fact: FactVersionRef) {
        if self.observations.last() == Some(&fact) {
            return;
        }
        self.observations.push(fact);
    }

    /// Bulk-record a previously finalised signature.
    ///
    /// Used when a higher-tier cold compute consumes a lower-tier
    /// cached result: the caller "inherits" the callee's observed
    /// facts by appending the callee's signature into the current
    /// tracer. The finalisation pass dedups across the merged set.
    #[inline]
    pub fn observe_borrowed_signature(&mut self, sig: &[FactVersionRef]) {
        self.observations.reserve(sig.len());
        for fact in sig {
            self.observations.push(fact.clone());
        }
    }

    /// Absorb an ALREADY-CANONICAL signature — the output of a previous
    /// [`Self::finalise`], retained on a warm cache candidate — without
    /// re-sorting it.
    ///
    /// The warm-reuse path of a cache slot appends the reused candidate's
    /// whole witness and then adds only a handful of attempt-local
    /// observations of its own. Splatting the witness into `observations`
    /// made every reuse pay an `O(n log n)` re-sort of a run that was
    /// sorted when it was minted; retaining it as a run makes finalisation
    /// merge instead, `O(n + m)` with no comparison wasted re-discovering
    /// an order the run already has.
    ///
    /// The finalised set is EXACTLY the union of the absorbed runs and the
    /// locally-observed facts — absorbing never drops an attempt-local
    /// observation, and merging never drops a fact present in only one
    /// side.
    ///
    /// An `Arc<[FactVersionRef]>` carries no proof that it is canonical, so
    /// the fast lane VERIFIES the precondition with a linear
    /// [`is_canonical_run`] check rather than trusting the caller; a
    /// non-canonical input is recorded as ordinary observations and sorted
    /// by the same finalisation pass. Either way the result is the same
    /// canonical set — there is exactly one canonicaliser.
    #[inline]
    pub fn absorb_canonical_signature(&mut self, signature: &Arc<[FactVersionRef]>) {
        if signature.is_empty() {
            return;
        }
        if is_canonical_run(signature) {
            self.canonical_runs.push(Arc::clone(signature));
        } else {
            self.observe_borrowed_signature(signature);
        }
    }

    /// Number of observations recorded so far (pre-dedup), counting facts
    /// held in absorbed canonical runs.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
            + self
                .canonical_runs
                .iter()
                .map(|run| run.len())
                .sum::<usize>()
    }

    /// Whether no observations have been recorded.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty() && self.canonical_runs.is_empty()
    }

    /// Collapse the tracer into ONE strictly-increasing observation vector:
    /// sort + dedup the local observations, then merge every absorbed
    /// canonical run into them.
    ///
    /// Idempotent, and leaves the tracer in an equivalent state (all facts
    /// in `observations`, no runs outstanding), so a mid-scope
    /// [`Self::would_overflow`] peek can call it without disturbing a later
    /// [`Self::finalise`].
    fn canonicalise(&mut self) {
        self.observations.sort_unstable_by(compare_fact_refs);
        self.observations.dedup();
        if !self.canonical_runs.is_empty() {
            let mut merged: Vec<FactVersionRef> = self.observations.as_slice().to_vec();
            for run in std::mem::take(&mut self.canonical_runs) {
                merged = merge_canonical_runs(&merged, &run);
            }
            self.observations = SmallVec::from_vec(merged);
        }
        // Domain-wise lifting runs LAST, on the deduplicated set: the
        // threshold is a property of the DISTINCT facts a domain
        // contributed, not of how many times they were observed. Running
        // it here — inside the one canonicaliser — is what makes every
        // finalised signature, from every entry point, compact
        // identically.
        let mut canonical = std::mem::take(&mut self.observations).into_vec();
        compact_domains(&mut canonical, &self.aggregate_basis);
        self.observations = SmallVec::from_vec(canonical);
    }

    /// Whether sealing this tracer WOULD report
    /// [`FactReadSetFinalise::Overflow`] — WITHOUT sealing it.
    ///
    /// The overflow-only peek for a consumer that reads the tracer's
    /// CACHEABILITY verdict but builds its cache entry's signature from
    /// another source (a carrier's `dep_signature`, a keyed canonical's
    /// observed hash). Such a consumer never needs the finalised set, so
    /// it must not pay [`Self::finalise`]'s `Arc<[FactVersionRef]>`
    /// allocation — nor emit the overflow audit event, which stays owned
    /// by the ONE signature-consuming [`Self::finalise`] boundary per
    /// compute (a nested peek that also emitted would multiply one
    /// overflowing compute's event + counter across every enclosing
    /// tracer level).
    ///
    /// Cheap by construction: dedup can only SHRINK the observation set,
    /// so a raw count at-or-under [`FACT_SIGNATURE_CAP`] — local
    /// observations PLUS every absorbed run, since the cap is a property
    /// of the finalised set — cannot overflow and short-circuits before
    /// any sort. The over-cap branch collapses the tracer to canonical
    /// form in place; the collapse is equivalence-preserving and
    /// idempotent, so a later `finalise` still sees the same set.
    #[must_use]
    pub fn would_overflow(&mut self) -> bool {
        if self.len() <= FACT_SIGNATURE_CAP {
            return false;
        }
        self.canonicalise();
        self.observations.len() > FACT_SIGNATURE_CAP
    }

    /// Seal the tracer into either an immutable signature or an
    /// overflow sentinel.
    ///
    /// Sort + dedup the observed facts in canonical order; if the
    /// deduplicated set exceeds [`FACT_SIGNATURE_CAP`], return
    /// [`FactReadSetFinalise::Overflow`]. Overflow is not a panic;
    /// the caller is responsible for refusing admission and emitting
    /// the appropriate audit event.
    #[must_use]
    pub fn finalise(mut self) -> FactReadSetFinalise {
        // STABILITY is settled before CARDINALITY, and stays a separate
        // outcome. An unstable attempt must never be reported as a size
        // failure: it would be refused under a rail that is about the
        // number of facts, and the caller could not tell a genuinely
        // wide compute from a racing one.
        if self.mutation_unstable {
            return FactReadSetFinalise::MutationUnstable;
        }
        // Canonicalise so two tracers that observed the same set of facts
        // in different orders produce byte-identical signatures: sort +
        // dedup the local observations under the derived total order, then
        // merge every absorbed canonical run in.
        {
            crate::probe_scope!(FINISH_SORT);
            crate::probe_tally!(
                ABSORBED_RUN_FACTS,
                self.canonical_runs
                    .iter()
                    .map(|run| run.len())
                    .sum::<usize>()
            );
            self.canonicalise();
        }
        crate::probe_tally!(OBS_POST_DEDUP, self.observations.len());
        if self.observations.len() > FACT_SIGNATURE_CAP {
            return FactReadSetFinalise::Overflow;
        }
        let arc: Arc<[FactVersionRef]> = {
            crate::probe_scope!(FINISH_ARC);
            Arc::from(self.observations.into_vec())
        };
        if self.non_cacheable_propagation.is_some() {
            FactReadSetFinalise::NonCacheable(arc)
        } else {
            FactReadSetFinalise::Ok(arc)
        }
    }
}

/// Outcome of [`FactReadSet::finalise`].
#[derive(Debug, Clone)]
pub enum FactReadSetFinalise {
    /// Successfully sealed: an immutable, sorted, deduplicated
    /// signature ready to install as a `Candidate::fact_dep_signature`.
    Ok(Arc<[FactVersionRef]>),
    /// The observation set is complete and remains available for bubbling into
    /// an enclosing tracer, but this compute consumed a read whose validating
    /// basis cannot be represented by those facts. The value may be returned;
    /// these facts must never authorize shared-cache admission.
    NonCacheable(Arc<[FactVersionRef]>),
    /// Signature exceeded [`FACT_SIGNATURE_CAP`]; the caller must
    /// refuse admission. No partial signature is returned — the
    /// tracer is consumed regardless of outcome.
    Overflow,
    /// A compaction domain this scope was COMPACTING advanced between
    /// its basis being installed and this finalisation, so the terminal
    /// aggregate would claim the domain held as of a generation these
    /// observations do not come from.
    ///
    /// Terminal on the first unstable attempt — no automatic retry — and
    /// deliberately NOT foldable into [`Self::Overflow`]. Degrading a
    /// stability failure into a cardinality one refuses the attempt for
    /// the wrong reason, under exactly the size rail this substrate
    /// exists to remove.
    MutationUnstable,
}

/// Interior-mutability wrapper allowing `&self` callers to record
/// observations.
///
/// The wrapper is `!Send + !Sync` (it owns a [`FactReadSet`] which
/// is `!Send + !Sync` by construction). A trait method that takes
/// `&self` and forwards into this wrapper does not need to bubble
/// `&mut self` through every callsite.
pub struct FactReadSetCell(RefCell<FactReadSet>);

impl Default for FactReadSetCell {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FactReadSetCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(b) => f
                .debug_struct("FactReadSetCell")
                .field("inner", &*b)
                .finish(),
            Err(_) => f
                .debug_struct("FactReadSetCell")
                .field("inner", &"<borrowed>")
                .finish(),
        }
    }
}

impl FactReadSetCell {
    /// Construct an empty cell.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self(RefCell::new(FactReadSet::new()))
    }

    /// Record one observation through `&self`.
    #[inline]
    pub fn observe(&self, fact: FactVersionRef) {
        self.0.borrow_mut().observe(fact);
    }

    /// Bulk-record a previously finalised signature through `&self`.
    #[inline]
    pub fn observe_borrowed_signature(&self, sig: &[FactVersionRef]) {
        self.0.borrow_mut().observe_borrowed_signature(sig);
    }

    /// Absorb an already-canonical signature through `&self`. See
    /// [`FactReadSet::absorb_canonical_signature`].
    #[inline]
    pub fn absorb_canonical_signature(&self, signature: &Arc<[FactVersionRef]>) {
        self.0.borrow_mut().absorb_canonical_signature(signature);
    }

    /// Record a non-cacheable read consumption through `&self`.
    #[inline]
    pub fn note_non_cacheable_read(&self, propagation: NonCacheablePropagation) {
        self.0.borrow_mut().note_non_cacheable_read(propagation);
    }

    /// Supply this scope's per-domain compaction basis through `&self`.
    /// See [`FactReadSet::set_aggregate_basis`].
    #[inline]
    pub fn set_aggregate_basis(&self, basis: AggregateGenerations) {
        self.0.borrow_mut().set_aggregate_basis(basis);
    }

    /// `true` when this scope's basis names at least one domain, i.e.
    /// when a live re-read could tell it anything. The short-circuit that
    /// keeps movement detection free for a scope that compacts nothing.
    #[inline]
    #[must_use]
    pub fn has_aggregate_basis(&self) -> bool {
        self.0.borrow().aggregate_basis().names_any_domain()
    }

    /// Re-read the live generations and record instability through
    /// `&self`. See [`FactReadSet::note_basis_recheck`].
    #[inline]
    pub fn note_basis_recheck(&self, live: &AggregateGenerations) {
        self.0.borrow_mut().note_basis_recheck(live);
    }

    /// Whether a domain this scope compacts against has moved.
    #[inline]
    #[must_use]
    pub fn mutation_unstable(&self) -> bool {
        self.0.borrow().mutation_unstable()
    }

    /// Whether a non-cacheable read was consumed in this scope.
    #[inline]
    #[must_use]
    pub fn non_cacheable_read_observed(&self) -> bool {
        self.0.borrow().non_cacheable_read_observed()
    }

    /// Number of observations recorded so far (pre-dedup).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// Whether no observations have been recorded.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Whether sealing this cell WOULD overflow — the non-finalising,
    /// non-emitting overflow peek through `&self`. See
    /// [`FactReadSet::would_overflow`]. Readable MID-SCOPE (the tracer
    /// accumulates monotonically), so a cacheability scope can consult its
    /// verdict at an admission point without popping the cell.
    #[inline]
    #[must_use]
    pub fn would_overflow(&self) -> bool {
        self.0.borrow_mut().would_overflow()
    }

    /// Consume the cell and return the underlying [`FactReadSet`].
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> FactReadSet {
        self.0.into_inner()
    }
}

/// Canonical ordering for [`FactVersionRef`] values. Used by
/// [`FactReadSet::finalise`] to produce byte-identical signatures
/// across permutations of the same observed set.
///
/// The order is the DERIVED [`Ord`] on [`FactVersionRef`]: enum
/// discriminant in declaration order first, then per-variant field order.
/// `FileWholeHash` < `DerivedFactHash` < `Parse` < `ResolveImports` <
/// `RouteSurface` < `FileSourceEnv` < `ProjectGeneration`.
///
/// Three properties the derive buys structurally, and that a hand-written
/// comparator does not:
///
/// * **Totality over every variant, present and future.** The derive is
///   generated from the type's own definition, so a new variant, a new
///   field, or a new nested key kind is ordered by construction. A new
///   nested type that is NOT `Ord` is a COMPILE ERROR at the derive site
///   — never a silent tie that collapses two distinct facts into one
///   comparison-equal pair and makes the sort order input-dependent.
/// * **Consistency with [`Eq`].** `a.cmp(b) == Equal` exactly when
///   `a == b`, so `sort_unstable` + `dedup` is exact set semantics: no
///   distinct fact is ever dropped, and no duplicate ever survives.
/// * **Run-to-run stability.** Every leaf is a `str`/byte/integer
///   comparison. Interned ids (`InternedName`, `InternedSpecifier`,
///   `FrameworkAdapterId`, …) wrap `Arc<str>`, whose `Ord` delegates to
///   `str` CONTENT — never the pointer, never intern-table insertion
///   order, never a randomly-seeded hash. Two processes that observe the
///   same fact set emit byte-identical signatures.
#[inline]
fn compare_fact_refs(a: &FactVersionRef, b: &FactVersionRef) -> std::cmp::Ordering {
    a.cmp(b)
}

/// TRUE when `run` is strictly increasing under [`compare_fact_refs`] —
/// i.e. it is already sorted AND carries no duplicate.
///
/// A linear `n`-comparison check with no allocation. It is what lets
/// [`FactReadSet::absorb_canonical_signature`] merge an already-finalised
/// signature in `O(n + m)` without TRUSTING the caller: an
/// `Arc<[FactVersionRef]>` carries no proof of canonicality, so the fast
/// lane verifies the precondition it depends on and a non-canonical input
/// falls back to the ordinary observation path, which sorts it.
fn is_canonical_run(run: &[FactVersionRef]) -> bool {
    run.windows(2)
        .all(|pair| compare_fact_refs(&pair[0], &pair[1]) == std::cmp::Ordering::Less)
}

/// Merge two strictly-increasing runs into one strictly-increasing run,
/// dropping cross-run duplicates. `O(left.len() + right.len())`
/// comparisons, one allocation for the output.
fn merge_canonical_runs(left: &[FactVersionRef], right: &[FactVersionRef]) -> Vec<FactVersionRef> {
    use std::cmp::Ordering;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < left.len() && j < right.len() {
        match compare_fact_refs(&left[i], &right[j]) {
            Ordering::Less => {
                out.push(left[i].clone());
                i += 1;
            }
            Ordering::Greater => {
                out.push(right[j].clone());
                j += 1;
            }
            Ordering::Equal => {
                out.push(left[i].clone());
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    out
}

#[cfg(test)]
mod finalise_tests {
    use super::*;

    fn fact(index: usize) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: format!("/fact-{index}.ts"),
            hash: [(index & 0xff) as u8; 16],
        }
    }

    #[test]
    fn finalise_encodes_non_cacheability_in_the_evidence() {
        let mut read_set = FactReadSet::new();
        read_set.observe(fact(1));
        read_set.note_non_cacheable_read(NonCacheablePropagation::Transitive);

        match read_set.finalise() {
            FactReadSetFinalise::NonCacheable(facts) => {
                assert_eq!(facts.as_ref(), &[fact(1)]);
            }
            other => panic!("non-cacheable observation finalised as {other:?}"),
        }
    }

    #[test]
    fn overflow_dominates_non_cacheability() {
        let mut read_set = FactReadSet::new();
        read_set.note_non_cacheable_read(NonCacheablePropagation::Transitive);
        for index in 0..=FACT_SIGNATURE_CAP {
            read_set.observe(fact(index));
        }

        assert!(matches!(read_set.finalise(), FactReadSetFinalise::Overflow));
    }
}

#[cfg(test)]
#[path = "fact_read_set_tests.rs"]
mod fact_read_set_tests;
