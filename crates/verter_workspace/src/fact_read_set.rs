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

use crate::fact_cache::FactVersionRef;

pub const FACT_SIGNATURE_CAP: usize = 1_024;

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
            _not_send_sync: PhantomData,
        }
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
        if self.canonical_runs.is_empty() {
            return;
        }
        let mut merged: Vec<FactVersionRef> = self.observations.as_slice().to_vec();
        for run in std::mem::take(&mut self.canonical_runs) {
            merged = merge_canonical_runs(&merged, &run);
        }
        self.observations = SmallVec::from_vec(merged);
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
