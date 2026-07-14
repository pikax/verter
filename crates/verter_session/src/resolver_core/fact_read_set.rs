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

use crate::resolver_core::{FactVersionRef, FACT_SIGNATURE_CAP};

/// Inline capacity for the observation accumulator. Empirically most
/// computes observe between 4 and 12 facts. The inline capacity is
/// sized to cover those without allocating; longer computes spill to
/// the heap exactly once.
const INLINE_CAPACITY: usize = 16;

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
    non_cacheable_read_observed: bool,
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
            .field("len", &self.observations.len())
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
            non_cacheable_read_observed: false,
            _not_send_sync: PhantomData,
        }
    }

    /// Record that a NON-CACHEABLE read (fenced serve, broken decl-body
    /// lease, unrootable route, unobservable source-env) was consumed
    /// inside this tracer's scope. Monotonic — never cleared.
    #[inline]
    pub fn note_non_cacheable_read(&mut self) {
        self.non_cacheable_read_observed = true;
    }

    /// TRUE when any non-cacheable read was consumed inside this tracer's
    /// scope.
    #[inline]
    #[must_use]
    pub fn non_cacheable_read_observed(&self) -> bool {
        self.non_cacheable_read_observed
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

    /// Number of observations recorded so far (pre-dedup).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Whether no observations have been recorded.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
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
    /// so a raw count at-or-under [`FACT_SIGNATURE_CAP`] cannot overflow
    /// and short-circuits before any sort. The over-cap branch sorts and
    /// dedups in place — the observations stay a valid multiset for a
    /// later `finalise`, which re-sorts regardless.
    #[must_use]
    pub fn would_overflow(&mut self) -> bool {
        if self.observations.len() <= FACT_SIGNATURE_CAP {
            return false;
        }
        self.observations.sort_by(compare_fact_refs);
        self.observations.dedup();
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
        // Sort canonically so two tracers that observed the same set
        // of facts in different orders produce byte-identical
        // signatures. The `FactVersionRef` `Ord` impl is derived via
        // its enum discriminant first, then per-variant field order;
        // we rely on `PartialOrd + Ord` being available below.
        self.observations.sort_by(compare_fact_refs);
        self.observations.dedup();
        if self.observations.len() > FACT_SIGNATURE_CAP {
            return FactReadSetFinalise::Overflow;
        }
        let arc: Arc<[FactVersionRef]> = Arc::from(self.observations.into_vec());
        FactReadSetFinalise::Ok(arc)
    }
}

/// Outcome of [`FactReadSet::finalise`].
#[derive(Debug, Clone)]
pub enum FactReadSetFinalise {
    /// Successfully sealed: an immutable, sorted, deduplicated
    /// signature ready to install as a `Candidate::fact_dep_signature`.
    Ok(Arc<[FactVersionRef]>),
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

    /// Record a non-cacheable read consumption through `&self`.
    #[inline]
    pub fn note_non_cacheable_read(&self) {
        self.0.borrow_mut().note_non_cacheable_read();
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

/// Stable ordering for [`FactVersionRef`] values. Used by
/// [`FactReadSet::finalise`] to produce byte-identical signatures
/// across permutations of the same observed set.
///
/// Order: enum discriminant first, then per-variant field order.
/// `FileWholeHash` < `DerivedFactHash` < `Parse` < `ResolveImports` <
/// `RouteSurface` < `FileSourceEnv` < `ProjectGeneration`.
fn compare_fact_refs(a: &FactVersionRef, b: &FactVersionRef) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let da = discriminant_rank(a);
    let db = discriminant_rank(b);
    match da.cmp(&db) {
        Ordering::Equal => {}
        non_eq => return non_eq,
    }
    match (a, b) {
        (
            FactVersionRef::FileWholeHash {
                canonical_id: ca,
                hash: ha,
            },
            FactVersionRef::FileWholeHash {
                canonical_id: cb,
                hash: hb,
            },
        ) => ca.cmp(cb).then_with(|| ha.cmp(hb)),
        (
            FactVersionRef::DerivedFactHash {
                canonical_id: ca,
                kind: ka,
                hash: ha,
            },
            FactVersionRef::DerivedFactHash {
                canonical_id: cb,
                kind: kb,
                hash: hb,
            },
        ) => ca
            .cmp(cb)
            .then_with(|| (*ka as u8).cmp(&(*kb as u8)))
            .then_with(|| ha.cmp(hb)),
        (FactVersionRef::Parse(a), FactVersionRef::Parse(b)) => compare_parse_fact(a, b),
        (FactVersionRef::ResolveImports(a), FactVersionRef::ResolveImports(b)) => {
            compare_resolve_imports_fact(a, b)
        }
        (FactVersionRef::RouteSurface(a), FactVersionRef::RouteSurface(b)) => {
            compare_route_surface_fact(a, b)
        }
        (
            FactVersionRef::FileSourceEnv {
                canonical_id: ca,
                parse_env_hash: pa,
                parser_version: va,
                file_language_id: la,
            },
            FactVersionRef::FileSourceEnv {
                canonical_id: cb,
                parse_env_hash: pb,
                parser_version: vb,
                file_language_id: lb,
            },
        ) => ca
            .cmp(cb)
            .then_with(|| pa.cmp(pb))
            .then_with(|| va.cmp(vb))
            // `FileLanguage` carries open-set ids without a total
            // order; compare the stable Debug form (same convention as
            // the per-domain `FactKey` comparisons below).
            .then_with(|| format!("{la:?}").cmp(&format!("{lb:?}"))),
        (
            FactVersionRef::ProjectGeneration { generation: ga },
            FactVersionRef::ProjectGeneration { generation: gb },
        ) => ga.cmp(gb),
        // Cross-variant ordering already handled by discriminant_rank.
        _ => Ordering::Equal,
    }
}

#[inline]
fn discriminant_rank(fact: &FactVersionRef) -> u8 {
    match fact {
        FactVersionRef::FileWholeHash { .. } => 0,
        FactVersionRef::DerivedFactHash { .. } => 1,
        FactVersionRef::Parse(_) => 2,
        FactVersionRef::ResolveImports(_) => 3,
        FactVersionRef::RouteSurface(_) => 4,
        FactVersionRef::FileSourceEnv { .. } => 5,
        FactVersionRef::ProjectGeneration { .. } => 6,
    }
}

fn compare_parse_fact(
    a: &crate::resolver_core::ParseFactRef,
    b: &crate::resolver_core::ParseFactRef,
) -> std::cmp::Ordering {
    a.canonical_id
        .cmp(&b.canonical_id)
        .then_with(|| format!("{:?}", a.key).cmp(&format!("{:?}", b.key)))
        .then_with(|| (a.lane as u8).cmp(&(b.lane as u8)))
        .then_with(|| a.expected_hash.cmp(&b.expected_hash))
}

fn compare_resolve_imports_fact(
    a: &crate::resolver_core::ResolveImportsFactRef,
    b: &crate::resolver_core::ResolveImportsFactRef,
) -> std::cmp::Ordering {
    a.canonical_id
        .cmp(&b.canonical_id)
        .then_with(|| format!("{:?}", a.key).cmp(&format!("{:?}", b.key)))
        .then_with(|| (a.lane as u8).cmp(&(b.lane as u8)))
        .then_with(|| a.expected_hash.cmp(&b.expected_hash))
}

fn compare_route_surface_fact(
    a: &crate::resolver_core::RouteSurfaceFactRef,
    b: &crate::resolver_core::RouteSurfaceFactRef,
) -> std::cmp::Ordering {
    a.canonical_id
        .cmp(&b.canonical_id)
        .then_with(|| format!("{:?}", a.key).cmp(&format!("{:?}", b.key)))
        .then_with(|| (a.lane as u8).cmp(&(b.lane as u8)))
        .then_with(|| a.expected_hash.cmp(&b.expected_hash))
}
