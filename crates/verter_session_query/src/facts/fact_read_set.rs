//! Push-style fact-read tracer used by cold-compute paths.
//!
//! ## What this module owns
//!
//! - [`FactReadSet`] — `!Sync` accumulator that collects
//!   [`FactVersionRef`] observations made during one cold compute and
//!   produces an immutable [`FactSet`] ready to install as a
//!   `Candidate::fact_dep_signature`.
//! - [`FactReadSetCell`] — interior-mutability wrapper so trait
//!   methods can record observations through `&self`.
//! - [`FactReadSetFinalise`] — the result of finalising a tracer: the
//!   immutable signature (`Ok`), the same complete signature marked
//!   unable to authorise admission (`NonCacheable`), or a stability
//!   refusal (`MutationUnstable`).
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
//! ## Wide signatures are sealed whole, never refused
//!
//! Signature width is never a refusal reason. Every fact a compute
//! observed is recorded and validated. Every sealed signature is ONE
//! canonical [`FactSet`] — sorted, deduplicated, iterated in canonical
//! order, with content-defined equality, ordering and hashing — at any
//! width. Sealing is deterministic — the same observed set always seals
//! to the same set — and lossless: a signature validates exactly when
//! every fact it holds validates, and projects every canonical it holds
//! into reverse-index registration. A wide set built by extending another
//! shares that set's tree nodes with every candidate, refusal summary and
//! enclosing signature that absorbs it; each node is charged against the
//! process retention account from its seal until its drop.
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
//! ## Single-compute lifetime
//!
//! A [`FactReadSetCell`] is `!Sync` by construction: it is never shared
//! between threads, so readers never race on it. It is installed on the
//! thread a compute runs on for that compute's extent — a compute that
//! suspends owns its cell between steps and installs it only while one of
//! its steps runs, so the cell moves with its task (it is `Send`) but is
//! never installed on two threads at once. This makes the tracer a true
//! per-compute substrate, not a shared accumulator.
//!
//! ## Nesting is supported
//!
//! Installations NEST: the installer pushes onto a per-thread tracer
//! STACK, and every observation fans out to ALL active cells (see
//! `resolver_context`'s fan-out chokepoints) — directly, or below a
//! delivering scope through the completed read set that scope's owner
//! hands down. An inner cold compute therefore records its facts into its
//! own cell AND into every enclosing one, so an outer compute's
//! observation set stays complete
//! while the inner one can make its OWN admission decision (a nested
//! non-cacheable read or nested mutation instability refuses the inner
//! entry without silently laundering into the outer signature). The
//! per-cell `!Send + !Sync` lifetime above is unchanged — the stack is
//! thread-local, and each cell still belongs to exactly one compute.

use std::cell::RefCell;
use std::marker::PhantomData;

use smallvec::SmallVec;

use crate::facts::fact_cache::{
    compaction_domain, AggregateGenerations, AggregatePopulation, CompactionDomain,
    DomainGenerationFact, FactVersionRef, ViewPopulation,
};
use crate::facts::fact_set::{FactSet, FactSetEdit};
use crate::resolution::ResolutionPopulation;

/// The logical width above which a signature counts as WIDE: a completed
/// structural carrier wider than this takes the strict-root rewrite when
/// its completion is what made it wide. Never a refusal threshold: a
/// canonical set of any width is sealed whole (see
/// [`seal_canonical_signature`]), never truncated or refused.
pub const WIDE_SIGNATURE_WIDTH: usize = 1_024;

/// Per-domain precision threshold: a compaction domain stays PRECISE while
/// its deduplicated bucket holds at most this many facts, and lifts to its
/// terminal aggregate at the first fact beyond it.
pub const FACT_DOMAIN_PRECISE_MAX: usize = 1_024;

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
    // No bucket can cross the threshold in a set no wider than it, and only
    // an aggregate already present is lifted otherwise.
    if facts.len() <= FACT_DOMAIN_PRECISE_MAX
        && !facts
            .iter()
            .any(|fact| matches!(fact, FactVersionRef::DomainGeneration(_)))
    {
        return false;
    }
    let mut precise: rustc_hash::FxHashMap<BucketKey, usize> = rustc_hash::FxHashMap::default();
    let mut already_lifted: rustc_hash::FxHashSet<BucketKey> = rustc_hash::FxHashSet::default();
    for fact in facts.iter() {
        // A self-root world and a consumed result's receipt are terminal
        // witnesses of their own: neither is a precise fact of one domain,
        // and a receipt reaches facts of every domain, so lifting it into
        // one domain's aggregate would drop the rest.
        if matches!(
            fact,
            FactVersionRef::StrictSelfRootWorld(_) | FactVersionRef::Receipt(_)
        ) {
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
        if matches!(
            fact,
            FactVersionRef::StrictSelfRootWorld(_) | FactVersionRef::Receipt(_)
        ) {
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
    sort_fact_refs(&mut kept);
    kept.dedup();
    *facts = kept;
    true
}

/// Inline capacity for the observation accumulator. Empirically most
/// computes observe between 4 and 12 facts directly, and a fact is large,
/// so the inline storage covers the common few without making every
/// tracer — created per computation and moved as it is installed and
/// sealed — carry room for the widest; longer computes spill to the heap
/// exactly once.
const INLINE_CAPACITY: usize = 4;

/// How many of the latest observations, and of the latest absorbed runs,
/// a repeated read is matched against before it is kept again.
const RECENT_WINDOW: usize = 16;

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

/// What a non-cacheable mark says about the value it taints, as far as an
/// admission must tell causes apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonCacheableBasis {
    /// The read was complete and current; the process only declined to
    /// retain it. A value built on it is the same answer, uncached.
    RetentionDeclined,
    /// Any other non-cacheable read, or a mark that does not say why.
    Untrusted,
}

/// Push-style accumulator for cold-compute fact reads.
///
/// Construct via [`FactReadSet::new`], record observations via
/// [`FactReadSet::observe`] / [`FactReadSet::observe_borrowed_signature`],
/// and seal via [`FactReadSet::finalise`].
///
/// Unbounded in width: finalisation seals a canonical set of any width
/// whole instead of refusing it.
///
/// `!Sync` by design: the tracer is one compute's state and is never shared.
/// It is `Send` so a suspended compute that owns its tracer between steps
/// moves with its task to whichever thread runs the next step, installed
/// there for that step alone. The `PhantomData<Cell<()>>` marker enforces
/// this at compile time.
pub struct FactReadSet {
    observations: SmallVec<[FactVersionRef; INLINE_CAPACITY]>,
    /// Already-canonical signatures absorbed wholesale from a completed
    /// compute (see [`FactReadSet::absorb_canonical_signature`]). Kept as
    /// separate strictly-increasing runs instead of being splatted into
    /// `observations`, so finalisation MERGES them in `O(n)` instead of
    /// re-sorting a set that was already sorted once. Every run in here has
    /// been verified strictly increasing at insertion.
    canonical_runs: Vec<FactSet>,
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
    /// TRUE once a non-cacheable mark OTHER than a declined retention was
    /// observed. Sticky; a mark that does not say why counts.
    untrusted_non_cacheable: bool,
    /// Live generation of each compaction domain, supplied by whoever
    /// installed this tracer. A domain absent from the basis never
    /// compacts — see [`compact_domains`].
    aggregate_basis: AggregateGenerations,
    /// TRUE once a domain THIS scope compacts against was observed to
    /// have advanced since its basis was installed. Sticky — a scope
    /// cannot become stable again, and cannot exempt itself from a
    /// generation it moved.
    mutation_unstable: bool,
    /// How many times [`Self::canonicalise`] rewrote `observations` in
    /// place. An [`ObservationMark`] taken before a rewrite no longer
    /// addresses a suffix of what was observed after it.
    rewrites: u32,
    _not_sync: PhantomData<std::cell::Cell<()>>,
}

/// Where a scope's observations stood at one instant: two marks, taken when
/// a result's computation began and when it ended, bound exactly what that
/// computation fanned into the scope (see
/// [`FactReadSet::collapse_into_receipt`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationMark {
    len: usize,
    canonical_len: usize,
    rewrites: u32,
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
            untrusted_non_cacheable: false,
            aggregate_basis: AggregateGenerations::default(),
            mutation_unstable: false,
            rewrites: 0,
            _not_sync: PhantomData,
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
    /// A mark recorded here does not say why, so it counts as
    /// [`NonCacheableBasis::Untrusted`].
    #[inline]
    pub fn note_non_cacheable_read(&mut self, propagation: NonCacheablePropagation) {
        self.note_non_cacheable_read_with_basis(propagation, NonCacheableBasis::Untrusted);
    }

    /// [`Self::note_non_cacheable_read`], stating what the mark says about
    /// the value it taints.
    #[inline]
    pub fn note_non_cacheable_read_with_basis(
        &mut self,
        propagation: NonCacheablePropagation,
        basis: NonCacheableBasis,
    ) {
        if self.non_cacheable_propagation != Some(NonCacheablePropagation::Transitive) {
            self.non_cacheable_propagation = Some(propagation);
        }
        if basis == NonCacheableBasis::Untrusted {
            self.untrusted_non_cacheable = true;
        }
    }

    /// TRUE when any non-cacheable read was consumed inside this tracer's
    /// scope.
    #[inline]
    #[must_use]
    pub fn non_cacheable_read_observed(&self) -> bool {
        self.non_cacheable_propagation.is_some()
    }

    /// TRUE when a non-cacheable read was consumed and EVERY one was a
    /// declined retention: the value is complete and current, only
    /// uncached.
    #[inline]
    #[must_use]
    pub fn non_cacheable_only_declined_retention(&self) -> bool {
        self.non_cacheable_propagation.is_some() && !self.untrusted_non_cacheable
    }

    /// Record one observed fact.
    ///
    /// Exact duplicates in a bounded recent window are short-circuited; the
    /// finalisation pass performs the full sort + dedup across all
    /// observations.
    #[inline]
    pub fn observe(&mut self, fact: FactVersionRef) {
        let (seen, comparisons) = self.recently_observed(&fact);
        record_signature_work(comparisons);
        if !seen {
            self.observations.push(fact);
        }
    }

    /// [`Self::observe`] for a borrowed fact: it is copied only when the
    /// recent window does not already hold it.
    #[inline]
    pub fn observe_ref(&mut self, fact: &FactVersionRef) {
        let (seen, comparisons) = self.recently_observed(fact);
        record_signature_work(comparisons);
        if !seen {
            self.observations.push(fact.clone());
        }
    }

    /// Bulk-record a previously finalised signature.
    ///
    /// Used when a higher-tier cold compute consumes a lower-tier
    /// cached result: the caller "inherits" the callee's observed
    /// facts into the current tracer. A bounded recent window prevents
    /// repeated small witnesses from growing live storage per warm read;
    /// finalisation dedups observations outside that window.
    #[inline]
    pub fn observe_borrowed_signature(&mut self, sig: &[FactVersionRef]) {
        self.observations.reserve(sig.len());
        let mut compared = 0;
        for fact in sig {
            let (seen, comparisons) = self.recently_observed(fact);
            compared += comparisons;
            if !seen {
                self.observations.push(fact.clone());
            }
        }
        record_signature_work(compared);
    }

    /// Whether `fact` is among the last [`RECENT_WINDOW`] observations,
    /// with the number of comparisons that decided it.
    #[inline]
    fn recently_observed(&self, fact: &FactVersionRef) -> (bool, usize) {
        let window = &self.observations[self.observations.len().saturating_sub(RECENT_WINDOW)..];
        match window.iter().rposition(|observed| observed == fact) {
            Some(position) => (true, window.len() - position),
            None => (false, window.len()),
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
    /// A [`FactSet`] is canonical by construction, so the run is kept as is
    /// with no check, and finalisation merges it with the other runs and
    /// the local observations into the same canonical set — there is
    /// exactly one canonicaliser.
    #[inline]
    pub fn absorb_canonical_signature(&mut self, signature: &FactSet) {
        if signature.is_empty() || self.recently_absorbed(signature) {
            return;
        }
        self.canonical_runs.push(signature.clone());
    }

    /// Whether one of the last [`RECENT_WINDOW`] absorbed runs is
    /// `signature`. An immutable retained run already proved its canonical
    /// order; replaying it must not grow live storage for each warm read.
    #[inline]
    fn recently_absorbed(&self, signature: &FactSet) -> bool {
        let recent =
            &self.canonical_runs[self.canonical_runs.len().saturating_sub(RECENT_WINDOW)..];
        let mut comparisons = 0;
        let mut seen = false;
        for run in recent.iter().rev() {
            comparisons += 1;
            if run.ptr_eq(signature) {
                seen = true;
                break;
            }
            if run.len() == signature.len() {
                comparisons += run.len();
                if run == signature {
                    seen = true;
                    break;
                }
            }
        }
        record_signature_work(comparisons);
        seen
    }

    /// The mark of everything observed so far.
    #[inline]
    #[must_use]
    pub fn mark(&self) -> ObservationMark {
        ObservationMark {
            len: self.observations.len(),
            canonical_len: self.canonical_runs.len(),
            rewrites: self.rewrites,
        }
    }

    /// Record that a completed result, computed between `start` and `end`,
    /// is answered by `receipt`: the observations it fanned into this scope
    /// in that range are replaced by the receipt, which stands for exactly
    /// them (its evidence was recorded from the same fan-out); anything
    /// observed after `end` stays. When the observations were rewritten in
    /// place since `start`, the range no longer exists as such, so the
    /// receipt is added beside them instead: never a fact lost, only one
    /// fewer collapse.
    pub fn collapse_into_receipt(
        &mut self,
        start: ObservationMark,
        end: ObservationMark,
        receipt: FactVersionRef,
    ) {
        let intact = start.rewrites == self.rewrites
            && end.rewrites == self.rewrites
            && start.len <= end.len
            && end.len <= self.observations.len()
            && start.canonical_len <= end.canonical_len
            && end.canonical_len <= self.canonical_runs.len();
        if intact {
            self.observations.drain(start.len..end.len);
            self.canonical_runs
                .drain(start.canonical_len..end.canonical_len);
            self.observations.insert(start.len, receipt);
        } else {
            self.observations.push(receipt);
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

    /// The absorbed run that holds every fact this scope observed, when its
    /// own seal reproduces it ([`seals_unchanged`]): the widest run, every
    /// other run a subset of it and every local observation in it.
    fn covering_run(&self) -> Option<usize> {
        let mut widest = 0;
        let mut index = 1;
        while index < self.canonical_runs.len() {
            if self.canonical_runs[index].len() > self.canonical_runs[widest].len() {
                widest = index;
            }
            index += 1;
        }
        let run = self.canonical_runs.get(widest)?;
        if !seals_unchanged(run) {
            return None;
        }
        let mut compared = 0;
        let mut covered = true;
        for observed in &self.observations {
            compared += 1;
            if !run.contains(observed) {
                covered = false;
                break;
            }
        }
        let mut other = 0;
        while covered && other < self.canonical_runs.len() {
            if other != widest {
                compared += self.canonical_runs[other].len();
                covered = run.is_superset(&self.canonical_runs[other]);
            }
            other += 1;
        }
        record_signature_work(compared);
        covered.then_some(widest)
    }

    /// Collapse the tracer into ONE strictly-increasing observation vector:
    /// sort + dedup the local observations, then merge every absorbed
    /// canonical run into them.
    ///
    /// Idempotent, and leaves the tracer in an equivalent state (all facts
    /// in `observations`, no runs outstanding).
    fn canonicalise(&mut self) -> FactSet {
        self.rewrites = self.rewrites.wrapping_add(1);
        let runs = std::mem::take(&mut self.canonical_runs);
        let mut edit = if runs.is_empty() {
            // Nothing absorbed: the local observations are the whole set.
            // Observations held inline are copied straight into the set's
            // one slice; spilled ones hand their vector over.
            let mut local = std::mem::take(&mut self.observations);
            sort_fact_refs(&mut local);
            local.dedup();
            if local.spilled() {
                FactSet::from_canonical_vec(local.into_vec()).edit()
            } else {
                FactSet::from_canonical_slice(&local).edit()
            }
        } else {
            // The widest absorbed run is the base, shared; every other run
            // and every local observation is inserted into it, so the set
            // pays only for what the base lacks.
            let mut widest = 0;
            for (index, run) in runs.iter().enumerate() {
                if run.len() > runs[widest].len() {
                    widest = index;
                }
            }
            let base = &runs[widest];
            let mut edit = base.edit();
            for (index, run) in runs.iter().enumerate() {
                if index == widest || run.ptr_eq(base) {
                    continue;
                }
                for fact in run.iter() {
                    record_signature_work(1);
                    if !edit.contains(fact) {
                        edit.insert(fact.clone());
                    }
                }
            }
            for fact in std::mem::take(&mut self.observations) {
                edit.insert(fact);
            }
            edit
        };
        // Domain-wise lifting runs LAST, on the deduplicated set: the
        // threshold is a property of the DISTINCT facts a domain
        // contributed, not of how many times they were observed. Running
        // it here — inside the one canonicaliser — is what makes every
        // finalised signature, from every entry point, compact
        // identically. A receipt a sibling receipt consumed is already
        // inside it.
        drop_subsumed_receipts_in(&mut edit);
        compact_domains_in(&mut edit, &self.aggregate_basis);
        edit.finish()
    }

    /// Seal the tracer into its immutable, complete signature.
    ///
    /// One canonical [`FactSet`], sharing the storage of the widest witness
    /// it absorbed. Every observed fact survives into the signature, however
    /// many there are.
    #[must_use]
    pub fn finalise(mut self) -> FactReadSetFinalise {
        // Width of the read set this compute observed, charged before the
        // stability outcome is decided so an unstable compute is still
        // counted for what it actually read.
        verter_audit::attribute_n!(ReadSetSignatureBuild, self.observations.len());
        if self.mutation_unstable {
            return FactReadSetFinalise::MutationUnstable;
        }
        // A scope whose every observation lies in one completed witness it
        // replayed seals as that witness: the union is the witness, and
        // canonicalising it again would rebuild the same facts.
        if let Some(widest) = self.covering_run() {
            let set = self.canonical_runs.swap_remove(widest);
            return if self.non_cacheable_propagation.is_some() {
                FactReadSetFinalise::NonCacheable(set)
            } else {
                FactReadSetFinalise::Ok(set)
            };
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
            let set = self.canonicalise();
            crate::probe_tally!(OBS_POST_DEDUP, set.len());
            if self.non_cacheable_propagation.is_some() {
                FactReadSetFinalise::NonCacheable(set)
            } else {
                FactReadSetFinalise::Ok(set)
            }
        }
    }
}

/// Outcome of [`FactReadSet::finalise`].
#[derive(Debug, Clone)]
pub enum FactReadSetFinalise {
    /// Successfully sealed: an immutable, sorted, deduplicated signature,
    /// complete at any width, ready to install as a
    /// `Candidate::fact_dep_signature`.
    Ok(FactSet),
    /// The observation set is complete and remains available for bubbling into
    /// an enclosing tracer, but this compute consumed a read whose validating
    /// basis cannot be represented by those facts. The value may be returned;
    /// these facts must never authorize shared-cache admission.
    NonCacheable(FactSet),
    /// A compaction domain this scope was COMPACTING advanced between
    /// its basis being installed and this finalisation, so the terminal
    /// aggregate would claim the domain held as of a generation these
    /// observations do not come from.
    ///
    /// Terminal on the first unstable attempt — no automatic retry.
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

    /// Record one borrowed observation through `&self`. See
    /// [`FactReadSet::observe_ref`].
    #[inline]
    pub fn observe_ref(&self, fact: &FactVersionRef) {
        self.0.borrow_mut().observe_ref(fact);
    }

    /// Bulk-record a previously finalised signature through `&self`.
    #[inline]
    pub fn observe_borrowed_signature(&self, sig: &[FactVersionRef]) {
        self.0.borrow_mut().observe_borrowed_signature(sig);
    }

    /// Absorb an already-canonical signature through `&self`. See
    /// [`FactReadSet::absorb_canonical_signature`].
    #[inline]
    pub fn absorb_canonical_signature(&self, signature: &FactSet) {
        self.0.borrow_mut().absorb_canonical_signature(signature);
    }

    /// The mark of everything observed so far, through `&self`. See
    /// [`FactReadSet::mark`].
    #[inline]
    #[must_use]
    pub fn mark(&self) -> ObservationMark {
        self.0.borrow().mark()
    }

    /// Replace a completed result's observations with its receipt through
    /// `&self`. See [`FactReadSet::collapse_into_receipt`].
    #[inline]
    pub fn collapse_into_receipt(
        &self,
        start: ObservationMark,
        end: ObservationMark,
        receipt: FactVersionRef,
    ) {
        self.0
            .borrow_mut()
            .collapse_into_receipt(start, end, receipt);
    }

    /// Record a non-cacheable read consumption through `&self`.
    #[inline]
    pub fn note_non_cacheable_read(&self, propagation: NonCacheablePropagation) {
        self.0.borrow_mut().note_non_cacheable_read(propagation);
    }

    /// Record a non-cacheable read consumption and its basis through
    /// `&self`. See [`FactReadSet::note_non_cacheable_read_with_basis`].
    #[inline]
    pub fn note_non_cacheable_read_with_basis(
        &self,
        propagation: NonCacheablePropagation,
        basis: NonCacheableBasis,
    ) {
        self.0
            .borrow_mut()
            .note_non_cacheable_read_with_basis(propagation, basis);
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

    /// Consume the cell and return the underlying [`FactReadSet`].
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> FactReadSet {
        self.0.into_inner()
    }
}

/// Whether sealing the canonical `run` alone reproduces it: no receipt in
/// it can consume another, and no domain can lift (the run is no wider
/// than [`FACT_DOMAIN_PRECISE_MAX`]) or already holds an aggregate.
fn seals_unchanged(run: &FactSet) -> bool {
    if run.len() > FACT_DOMAIN_PRECISE_MAX {
        return false;
    }
    let (receipts, aggregate) = terminal_tail(run.iter());
    receipts < 2 && !aggregate
}

/// The receipts, and whether an aggregate, a canonical set holds. Every
/// receipt, self-root world and aggregate sorts after every precise fact
/// (the canonical order is the variant order), so only the set's tail is
/// read.
fn terminal_tail<'f>(facts: impl DoubleEndedIterator<Item = &'f FactVersionRef>) -> (usize, bool) {
    let mut receipts = 0;
    let mut aggregate = false;
    for fact in facts.rev() {
        match fact {
            FactVersionRef::Receipt(_) => receipts += 1,
            FactVersionRef::DomainGeneration(_) => aggregate = true,
            FactVersionRef::StrictSelfRootWorld(_) => {}
            _ => break,
        }
    }
    (receipts, aggregate)
}

/// [`crate::facts::fact_cache::drop_subsumed_receipts`] on a set under
/// construction: only its receipts (its tail) are read.
fn drop_subsumed_receipts_in(edit: &mut FactSetEdit) {
    let receipts: Vec<crate::facts::receipt::ResultReceipt> = edit
        .iter()
        .rev()
        .map_while(|fact| match fact {
            FactVersionRef::Receipt(receipt) => Some(Some(receipt.clone())),
            FactVersionRef::DomainGeneration(_) | FactVersionRef::StrictSelfRootWorld(_) => {
                Some(None)
            }
            _ => None,
        })
        .flatten()
        .collect();
    if receipts.len() < 2 {
        return;
    }
    let mut kept: Vec<FactVersionRef> = receipts
        .iter()
        .cloned()
        .map(FactVersionRef::Receipt)
        .collect();
    let before = kept.len();
    crate::facts::fact_cache::drop_subsumed_receipts(&mut kept);
    if kept.len() == before {
        return;
    }
    for receipt in receipts {
        let fact = FactVersionRef::Receipt(receipt);
        if !kept.contains(&fact) {
            edit.remove(&fact);
        }
    }
}

/// [`compact_domains`] on a set under construction: a set that cannot lift
/// (no wider than one bucket's bound and holding no aggregate) is left as
/// it is without being read; otherwise the lifted facts are removed and the
/// aggregates inserted, every untouched node staying shared.
fn compact_domains_in(edit: &mut FactSetEdit, basis: &AggregateGenerations) {
    if edit.len() <= FACT_DOMAIN_PRECISE_MAX && !terminal_tail(edit.iter()).1 {
        return;
    }
    let before: Vec<FactVersionRef> = edit.iter().cloned().collect();
    let mut after = before.clone();
    if !compact_domains(&mut after, basis) {
        return;
    }
    let (mut i, mut j) = (0, 0);
    let mut removed = Vec::new();
    let mut added = Vec::new();
    loop {
        match (before.get(i), after.get(j)) {
            (Some(left), Some(right)) => match left.cmp(right) {
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => {
                    removed.push(left.clone());
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    added.push(right.clone());
                    j += 1;
                }
            },
            (Some(left), None) => {
                removed.push(left.clone());
                i += 1;
            }
            (None, Some(right)) => {
                added.push(right.clone());
                j += 1;
            }
            (None, None) => break,
        }
    }
    for fact in &removed {
        edit.remove(fact);
    }
    for fact in added {
        edit.insert(fact);
    }
}

/// Seal an arbitrary fact collection as a complete signature: sort,
/// deduplicate, and drop receipts another receipt in the set already
/// consumed. The result is one canonical set, whole at any width.
///
/// The one sealer for a signature a producer assembles by hand (a union
/// of operand evidence, a self-root rewrite) rather than through a
/// tracer. It never compacts: compaction needs a tracer's live basis.
#[must_use]
pub fn seal_canonical_signature(mut facts: Vec<FactVersionRef>) -> FactSet {
    sort_dedup_facts(&mut facts);
    crate::facts::fact_cache::drop_subsumed_receipts(&mut facts);
    FactSet::from_canonical_vec(facts)
}

/// Seal `base` together with `extra`: the sealed set shares `base`'s
/// storage and pays only for what `extra` adds. The same facts as
/// [`seal_canonical_signature`] over `base` and `extra` together.
#[must_use]
pub fn seal_onto(base: &FactSet, extra: impl IntoIterator<Item = FactVersionRef>) -> FactSet {
    let mut edit = base.edit();
    for fact in extra {
        edit.insert(fact);
    }
    seal_edit(edit)
}

/// Seal a set under construction: the sealer's receipt rule (a receipt
/// another receipt of the set consumed is dropped), applied to its tail.
#[must_use]
pub fn seal_edit(mut edit: FactSetEdit) -> FactSet {
    drop_subsumed_receipts_in(&mut edit);
    edit.finish()
}

/// The most facts appended past a canonical prefix that are placed into it
/// one by one rather than by sorting the whole collection.
const APPENDED_TAIL_MAX: usize = 8;

/// Sort `facts` into canonical order and drop duplicates. A hand-assembled
/// signature is usually a canonical set with a few facts appended (a
/// self-root beside the traced set): those are placed into the strictly
/// increasing prefix by binary search, which is the same set in the same
/// order as a full sort and deduplication.
fn sort_dedup_facts(facts: &mut Vec<FactVersionRef>) {
    let mut prefix = facts.len().min(1);
    let mut compared = 0;
    while prefix < facts.len() && {
        compared += 1;
        compare_fact_refs(&facts[prefix - 1], &facts[prefix]) == std::cmp::Ordering::Less
    } {
        prefix += 1;
    }
    record_signature_work(compared);
    if prefix == facts.len() {
        return;
    }
    if facts.len() - prefix > APPENDED_TAIL_MAX {
        sort_fact_refs(facts);
        facts.dedup();
        return;
    }
    let tail = facts.split_off(prefix);
    for fact in tail {
        let mut compared = 0;
        let found = facts.binary_search_by(|held| {
            compared += 1;
            compare_fact_refs(held, &fact)
        });
        record_signature_work(compared);
        if let Err(at) = found {
            facts.insert(at, fact);
        }
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
///
/// Each comparison is one unit of signature work. A pass counts its
/// comparisons and records them once ([`record_signature_work`]), so a
/// sort or merge of `n` facts records `n`-proportional work without a
/// thread-local update per comparison.
#[inline]
fn compare_fact_refs(a: &FactVersionRef, b: &FactVersionRef) -> std::cmp::Ordering {
    a.cmp(b)
}

/// Sort `facts` into the canonical fact order ([`compare_fact_refs`]),
/// recording each comparison as signature work.
fn sort_fact_refs(facts: &mut [FactVersionRef]) {
    let mut compared = 0;
    facts.sort_unstable_by(|a, b| {
        compared += 1;
        compare_fact_refs(a, b)
    });
    record_signature_work(compared);
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static SIGNATURE_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Actual proof ordering, summary and visitation work, independent of the
/// solver's query-step counter. No observability state exists in production.
#[inline]
pub(crate) fn record_signature_work(units: usize) {
    #[cfg(any(test, feature = "test-support"))]
    SIGNATURE_WORK.with(|work| work.set(work.get().saturating_add(units)));
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = units;
}

#[cfg(any(test, feature = "test-support"))]
pub fn signature_work_for_tests() -> usize {
    SIGNATURE_WORK.with(std::cell::Cell::get)
}

#[cfg(test)]
mod finalise_tests {
    use super::*;

    fn fact(index: usize) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: format!("/fact-{index}.ts").into(),
            hash: [(index & 0xff) as u8; 16],
        }
    }

    #[test]
    fn repeated_small_signatures_keep_bounded_live_observations() {
        for canonical in [false, true] {
            let mut read_set = FactReadSet::new();
            let signature: FactSet = FactSet::from([fact(1), fact(2), fact(3)]);
            for _ in 0..10_000 {
                if canonical {
                    read_set.absorb_canonical_signature(&signature);
                } else {
                    read_set.observe_borrowed_signature(&signature.to_vec());
                }
            }
            assert_eq!(read_set.len(), signature.len());
            let mut changed = fact(2);
            if let FactVersionRef::FileWholeHash { hash, .. } = &mut changed {
                *hash = [99; 16];
            }
            read_set.observe_borrowed_signature(&[changed.clone()]);
            assert_eq!(read_set.len(), signature.len() + 1);
            let FactReadSetFinalise::Ok(facts) = read_set.finalise() else {
                panic!("stable signature replay must remain cacheable");
            };
            assert!(facts.contains(&changed));
            assert!(signature.iter().all(|fact| facts.contains(fact)));
        }
    }

    #[test]
    fn canonical_run_batches_keep_proof_work_proportional() {
        let [small, large] = [128, 512].map(|count| {
            let before = signature_work_for_tests();
            let mut read_set = FactReadSet::new();
            for index in 0..count {
                read_set.absorb_canonical_signature(&FactSet::from([fact(index)]));
            }
            let FactReadSetFinalise::Ok(signature) = read_set.finalise() else {
                panic!("stable canonical runs must remain cacheable");
            };
            let work = signature_work_for_tests() - before;
            let actual = signature
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(actual, (0..count).map(fact).collect());
            println!("canonical_runs={count} merge_proof_work={work}");
            work
        });
        assert!(
            large <= 6 * small,
            "canonical merge work grew from {small} to {large} over 4x input"
        );
    }

    #[test]
    fn consumed_receipt_replaces_canonical_runs_and_keeps_outside_facts() {
        let before = fact(0);
        let child: FactSet = FactSet::from([fact(1), fact(2)]);
        let after = fact(3);
        let mut read_set = FactReadSet::new();
        read_set.observe(before.clone());
        let start = read_set.mark();
        read_set.absorb_canonical_signature(&child);
        let end = read_set.mark();
        read_set.observe(after.clone());
        let receipt =
            FactVersionRef::Receipt(crate::facts::receipt::ResultReceipt::new(child.to_vec()));
        read_set.collapse_into_receipt(start, end, receipt.clone());
        let FactReadSetFinalise::Ok(signature) = read_set.finalise() else {
            panic!("stable consumed evidence must remain cacheable");
        };
        assert_eq!(
            signature,
            seal_canonical_signature(vec![before, receipt, after])
        );
        let mut leaves = std::collections::BTreeSet::new();
        let mut walk = crate::facts::receipt::ReceiptWalk::default();
        for entry in signature.iter() {
            assert!(crate::facts::fact_cache::validates_through_receipts(
                entry,
                &mut walk,
                |leaf| {
                    leaves.insert(leaf.clone());
                    true
                }
            ));
        }
        assert_eq!(leaves, (0..4).map(fact).collect());
    }

    #[test]
    fn finalise_encodes_non_cacheability_in_the_evidence() {
        let mut read_set = FactReadSet::new();
        read_set.observe(fact(1));
        read_set.note_non_cacheable_read(NonCacheablePropagation::Transitive);

        match read_set.finalise() {
            FactReadSetFinalise::NonCacheable(facts) => {
                assert_eq!(facts.to_vec(), &[fact(1)]);
            }
            other => panic!("non-cacheable observation finalised as {other:?}"),
        }
    }

    /// A scope is retention-only while every non-cacheable mark it saw was a
    /// declined retention; one untrustworthy or unexplained mark, in either
    /// order, ends that for good.
    #[test]
    fn only_declined_retention_survives_no_untrusted_mark() {
        let declined = |read_set: &mut FactReadSet| {
            read_set.note_non_cacheable_read_with_basis(
                NonCacheablePropagation::Transitive,
                NonCacheableBasis::RetentionDeclined,
            );
        };
        let mut read_set = FactReadSet::new();
        assert!(!read_set.non_cacheable_only_declined_retention());
        declined(&mut read_set);
        declined(&mut read_set);
        assert!(read_set.non_cacheable_only_declined_retention());
        read_set.note_non_cacheable_read(NonCacheablePropagation::LocalOnly);
        assert!(!read_set.non_cacheable_only_declined_retention());
        declined(&mut read_set);
        assert!(
            !read_set.non_cacheable_only_declined_retention(),
            "a later declined retention never clears an untrusted mark"
        );
        assert!(matches!(
            read_set.finalise(),
            FactReadSetFinalise::NonCacheable(_)
        ));

        let mut read_set = FactReadSet::new();
        read_set.note_non_cacheable_read_with_basis(
            NonCacheablePropagation::Transitive,
            NonCacheableBasis::Untrusted,
        );
        declined(&mut read_set);
        assert!(!read_set.non_cacheable_only_declined_retention());
    }

    #[test]
    fn a_wide_non_cacheable_set_keeps_every_fact() {
        let mut read_set = FactReadSet::new();
        read_set.note_non_cacheable_read(NonCacheablePropagation::Transitive);
        for index in 0..=WIDE_SIGNATURE_WIDTH {
            read_set.observe(fact(index));
        }

        let FactReadSetFinalise::NonCacheable(facts) = read_set.finalise() else {
            panic!("a wide non-cacheable observation set stays non-cacheable, never refused");
        };
        let signature = crate::facts::fact_cache::ReadSetSignature::new(facts);
        let mut leaves = 0usize;
        assert!(signature.all_leaves(|_| {
            leaves += 1;
            true
        }));
        assert_eq!(leaves, WIDE_SIGNATURE_WIDTH + 1);
    }
}

#[cfg(test)]
#[path = "fact_read_set_tests.rs"]
mod fact_read_set_tests;

#[cfg(test)]
mod canonical_signature_tests {
    use super::*;

    fn fact(index: usize) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: std::sync::Arc::from(format!("/canonical-{index:03}.ts")),
            hash: [3; 16],
        }
    }

    #[test]
    fn absorbing_a_sealed_signature_does_no_order_work_and_keeps_its_storage() {
        let mut producer = FactReadSet::new();
        for index in (0..64).rev() {
            producer.observe(fact(index));
        }
        let FactReadSetFinalise::Ok(signature) = producer.finalise() else {
            panic!("the producer is complete and stable");
        };
        let before = signature_work_for_tests();
        let mut consumer = FactReadSet::new();
        consumer.absorb_canonical_signature(&signature);
        let FactReadSetFinalise::Ok(sealed) = consumer.finalise() else {
            panic!("sealed consumption remains cacheable");
        };
        assert_eq!(sealed, signature);
        assert!(sealed.ptr_eq(&signature), "the covering witness is the set");
        assert!(
            signature_work_for_tests() - before < signature.len(),
            "a sealed set is canonical by construction: no adjacent-order check"
        );
    }

    #[test]
    fn an_unordered_borrowed_signature_is_canonicalised_by_the_one_sealer() {
        let malformed = [fact(7), fact(2), fact(7)];
        let mut consumer = FactReadSet::new();
        consumer.observe_borrowed_signature(&malformed);
        let FactReadSetFinalise::Ok(facts) = consumer.finalise() else {
            panic!("ordering alone cannot refuse a raw observation set");
        };
        assert_eq!(facts, FactSet::from([fact(2), fact(7)]));
    }

    #[test]
    fn finalisation_preserves_non_cacheable_and_mutation_verdicts() {
        let mut declined = FactReadSet::new();
        declined.observe(fact(1));
        declined.note_non_cacheable_read(NonCacheablePropagation::Transitive);
        assert!(matches!(
            declined.finalise(),
            FactReadSetFinalise::NonCacheable(_)
        ));
        let mut moved = FactReadSet::new();
        moved.set_aggregate_basis(AggregateGenerations {
            content: Some(crate::facts::fact_cache::AggregateStamp::Generation(1)),
            view_population: Some(crate::facts::fact_cache::ViewPopulation::Base),
            ..AggregateGenerations::default()
        });
        moved.note_basis_recheck(&AggregateGenerations {
            content: Some(crate::facts::fact_cache::AggregateStamp::Generation(2)),
            view_population: Some(crate::facts::fact_cache::ViewPopulation::Base),
            ..AggregateGenerations::default()
        });
        assert!(matches!(
            moved.finalise(),
            FactReadSetFinalise::MutationUnstable
        ));
    }
}
