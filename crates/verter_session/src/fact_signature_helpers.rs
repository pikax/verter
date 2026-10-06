//! Shared helpers for the R3/R26/R28 fact-based validation substrate
//! used by the inner component-meta caches.
//!
//! Every cache entry carries a [`ReadSetSignature`] whose `facts`
//! rail is `Arc<[FactVersionRef]>` — the cold-compute observation set
//! the producer recorded. It is the sole cache-validity rail. Warm-hit
//! reads call [`validate_fact_signature`] which walks every fact
//! through the current [`verter_session_query::facts::store_view::StoreView`] snapshot; a
//! single mismatch returns `false` and the warm hit misses, falling
//! through to cold recompute.
//!
//! Bubble-up is the dual rule: when a cache cold-compute or warm-hit
//! returns a value to its caller, the entry's fact signature merges
//! into any active outer tracer via
//! [`verter_session_query::facts::fact_read_set::FactReadSetCell::observe_borrowed_signature`].
//! That keeps the outer compute's observation set complete even when
//! inner reads come from cache.
//!
//! ## Path-precise parse facts (R28)
//!
//! Beneath the self-root (below), two helper shapes carry the Family A
//! path-precise parse facts:
//!
//! - [`fact_signature_for_canonical_member`] — keyed on `(canonical,
//!   exporter, member, space)`. The cold path reads the `Member`
//!   body fingerprint and `MemberPresence` header fact for the
//!   member.
//! - [`fact_signature_for_exported_type`] — keyed on `(canonical,
//!   type_name, space)`. The cold path observes the top-level type
//!   identity via `Export`, `LocalDecl`, and `MemberShape` facts;
//!   adding/removing/renaming a member shifts `MemberShape`.
//!
//! For caches whose key shape does NOT include a member name (e.g.
//! `AppConfigNoOverrideProofKey`), the cold path observes the
//! `SyntacticExportSet` of the contributing canonical instead. These
//! parse facts remain a refinement for cross-file consumers; the
//! self-root below is the always-on same-file edit detector.
//!
//! ## Self-version rooting (provenance-pure)
//!
//! Each of the three central helpers leads with a self-root
//! `FactVersionRef::FileWholeHash` for the defining/key canonical it
//! represents, then adds the path-precise parse facts above. The
//! self-root is the whole-hash fact for the cache entry's OWN keyed
//! canonical: any byte change to that file shifts its whole hash, so
//! a warm read that validates the self-root via
//! [`validate_fact_signature_with_self_roots`] detects a
//! same-canonical content edit and recomputes. The path-precise parse
//! facts still gate sibling-edit reuse — the self-root augments them
//! for correctness-first closure, it does not replace them.
//!
//! The three central helpers are **provenance-pure**: they never
//! consult the authoritative current-content oracle and never re-read
//! current content. The keyed canonical's content identity is a
//! caller-supplied `observed_hash` — the content version the
//! producer's value was actually computed against, captured exactly
//! once at the value source and threaded into the builder. Both the
//! self-root `FileWholeHash` and every `Parse` fact are pinned to that
//! observed version (`Parse` facts via
//! [`parse_fact_ref_for_observed_current_content`], a content-addressed
//! [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! lookup). Re-reading the canonical's *current* hash inside a
//! signature builder would open a publish race: an `upsert` landing
//! between value-compute and signature-build would root a stale value
//! by a fresh-looking current hash, which then validates on warm
//! reads instead of missing. Each builder returns `None` — refusing
//! shared-cache admission — when the observed version's parse-fact
//! registry cannot be recovered.

use std::sync::Arc;

use verter_session_query::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

use crate::cache_runtime::NonAdmissionReason;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{DepSignature, DepVersion};
use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::fact_cache::SignatureAdmission;
use verter_session_query::facts::store_view::StoreView;
use verter_session_query::facts::{
    fact_cache::{FactVersionRef, ParseFactRef},
    fact_read_set::{FactReadSet, FactReadSetFinalise, FACT_SIGNATURE_CAP},
};

/// Bracket one cold-compute closure with a push-style fact tracer.
///
/// Installs a fresh [`verter_session_query::facts::fact_read_set::FactReadSetCell`] onto the
/// TLS tracer stack, runs `f`, pops the tracer, and finalises the
/// observation set. On [`FactReadSetFinalise::Overflow`] emits a
/// [`verter_audit::structured_event::StructuredAuditEvent::FactSignatureOverflow`]
/// and increments the host's per-host
/// [`crate::VerterHost::signature_overflow_at_install`] counter.
///
/// Returns `(return_value, finalise_result, non_cacheable_read_observed)` so
/// callers decide whether to admit the result to cache or treat it as
/// non-cacheable. `non_cacheable_read_observed == true` means the traced
/// compute consumed a FENCED (ReturnOnly, `store_published == false`)
/// `IndexedReady` serve: the result's fact stamps are read from the
/// LIVE post-mutation state while its payload was computed FROM the
/// superseded artifact — an entry the read-side fact rail cannot
/// reject, so every shared-cache admission point MUST refuse it
/// (serve the value to the caller, publish nothing).
pub(crate) fn install_fact_tracer<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    f: F,
) -> (R, FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    let (value, read_set) = source.with_fact_tracer(|| {
        #[cfg(any(test, feature = "test-support"))]
        force_tracer_overflow_observations(source, None);
        f()
    });
    (value, finalise_compute_scope(source, read_set))
}

/// Finalise the read set of one compute's tracer scope: re-check its basis,
/// finalise it, and report an overflow.
fn finalise_compute_scope<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
    source: &FactTracerBasisSource<'_, W>,
    mut read_set: verter_session_query::facts::fact_read_set::FactReadSet,
) -> FactReadSetFinalise {
    note_basis_recheck(source, &mut read_set);
    let finalise = read_set.finalise();
    // The overflow audit event + host counter are emitted HERE and ONLY here —
    // at the ONE signature-CONSUMING boundary per compute. The cacheability
    // scope below deliberately uses the non-emitting `would_overflow` peek: an
    // inner overflow fans into every enclosing tracer, so an emitting nested
    // peek would multiply a single overflowing compute's event and counter
    // across each nesting level.
    if matches!(finalise, FactReadSetFinalise::Overflow) {
        crate::request_observers::push_structured_event(
            verter_audit::structured_event::StructuredAuditEvent::FactSignatureOverflow {
                candidate_size: (FACT_SIGNATURE_CAP as u32).saturating_add(1),
                cap: FACT_SIGNATURE_CAP as u32,
            },
        );
        source.record_signature_overflow();
    }
    finalise
}

/// The fact tracer of one compute that runs in steps (a continuation
/// frame's build): installed on the thread only while one of its steps
/// runs, and finalised once, as [`install_fact_tracer`] finalises its
/// scope, when the compute completes.
pub(crate) struct StepwiseFactTracer<'h, W> {
    source: FactTracerBasisSource<'h, W>,
    tracer: crate::resolver_core::resolver_context::OwnedFactTracer,
    #[cfg(any(test, feature = "test-support"))]
    forced: bool,
}

impl<'h, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    StepwiseFactTracer<'h, W>
{
    pub(crate) fn new(source: FactTracerBasisSource<'h, W>) -> Self {
        let tracer =
            crate::resolver_core::resolver_context::OwnedFactTracer::new(source.live_basis());
        Self {
            source,
            tracer,
            #[cfg(any(test, feature = "test-support"))]
            forced: false,
        }
    }

    /// Install the tracer for one step; the scope uninstalls it.
    pub(crate) fn install(
        &mut self,
    ) -> crate::resolver_core::resolver_context::OwnedTracerScope<'_> {
        let scope = self.tracer.install();
        #[cfg(any(test, feature = "test-support"))]
        if !std::mem::replace(&mut self.forced, true) {
            force_tracer_overflow_observations(&self.source, None);
        }
        scope
    }

    /// The compute completed: finalise what it observed.
    pub(crate) fn finish(self) -> FactReadSetFinalise {
        finalise_compute_scope(&self.source, self.tracer.into_read_set())
    }
}

/// Everything a fact-tracer scope needs to compose its compaction basis,
/// bound together at the ONE point that can answer both halves.
///
/// A basis has a view-derived half and a live half. The view-derived half
/// is captured ONCE, here, from a view the caller ALREADY HOLDS — a
/// borrow, not a `StoreViewManager` read. The live half is five atomic
/// loads. Neither end of a scope — installation, nor the movement
/// re-check every admission boundary performs — ever builds a store view,
/// which is the whole reason this type exists: the re-check runs on a
/// hotter path than installation, and a per-scope store-view read there
/// is the same `O(N)`-read regression the batch-fixed-view collapse
/// removed.
///
/// Host and seed travel together because they must agree. A seed captured
/// from one host's view composed against another host's counters would
/// compare stamps from different worlds; binding them in one value that
/// is only ever constructed from a single context or a single host makes
/// that unrepresentable at the call site.
enum BasisAuthority<'h, W> {
    Bound {
        port: &'h dyn crate::resolver_core::fact_validation_port::FactValidation,
        clocks: verter_session_query::facts::clocks::AggregateClockReader<W>,
    },
    Unbound {
        overflow: &'h std::sync::atomic::AtomicU64,
        #[cfg(any(test, feature = "test-support"))]
        non_cacheable: &'h std::sync::atomic::AtomicBool,
        #[cfg(any(test, feature = "test-support"))]
        observations: &'h std::sync::atomic::AtomicUsize,
    },
}

/// The clock source of a basis source bound to no context. An unbound
/// source never samples live clocks, so this type has no values.
#[derive(Clone, Copy)]
pub enum UnboundClocks {}

impl verter_session_query::facts::clocks::WorkspaceClocks for UnboundClocks {
    fn content_generation(&self) -> u64 {
        match *self {}
    }
    fn source_env_generation(&self) -> Option<u64> {
        match *self {}
    }
}

impl<'h> FactTracerBasisSource<'h, UnboundClocks> {
    /// Seed a scope that has NO bound view.
    ///
    /// Names no domain, so the scope compacts nothing and detects no
    /// movement — the same state every tracer was in before a basis
    /// existed. Deliberately NOT "read a view to find one": doing that
    /// per scope is exactly the cost this seam removes, and a scope with
    /// no bound view has no view to be validated against later either.
    #[must_use]
    pub fn unbound(host: &'h crate::VerterHost) -> Self {
        Self::unbound_with_clocks(host)
    }

    /// The unbound-observer basis: the only constructor that lets a fact
    /// tracer sit on observers the host does not own a `HostStoreView` for.
    ///
    /// Its consumer is the fact-validation proof surface, which is compiled
    /// only under `test` / `test-support`, so it carries that same gate: a
    /// shipped (no test-support) build — the wasm32 lane among them — neither
    /// carries the shape nor holds a caller. The whole fact-validation proof
    /// surface follows ONE rule: a proof-state item is present exactly where
    /// its producer or reader is compiled, and absent everywhere else, so no
    /// build configuration holds a store, counter or mirror with no reader.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn unbound_observers(
        overflow: &'h std::sync::atomic::AtomicU64,
        forcing: &'h crate::engine_test_knobs::TestKnobs,
    ) -> Self {
        Self {
            authority: BasisAuthority::Unbound {
                overflow,
                non_cacheable: &forcing.force_fact_tracer_non_cacheable_read,
                observations: &forcing.force_fact_tracer_overflow_observations,
            },
            seed: verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
        }
    }
}

/// `W` is the bound context's concrete workspace clock source, so the live
/// basis re-check samples it without a dynamic dispatch.
pub struct FactTracerBasisSource<'h, W> {
    authority: BasisAuthority<'h, W>,
    seed: verter_session_query::facts::fact_cache::AggregateBasisSeed,
}

impl<'h, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    FactTracerBasisSource<'h, W>
{
    /// Seed from a request-bound resolver context.
    ///
    /// The seed is a BORROW of the view the request boundary already
    /// bound, so this costs one virtual call and no store-view read. This
    /// is the constructor every producer that holds a context should use:
    /// a request-bound scope detects movement in the two composite
    /// domains, and an unbound one cannot.
    ///
    /// It reads the seed through
    /// [`ResolverContext::aggregate_basis_seed`](crate::resolver_core::ResolverContext::aggregate_basis_seed)
    /// rather than re-deriving it from the view. This keeps the context
    /// projection as the one compaction-basis authority and lets test doubles
    /// explicitly represent an unbound basis.
    #[must_use]
    pub fn from_ctx(
        ctx: &'h dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> Self {
        Self {
            authority: BasisAuthority::Bound {
                port: ctx,
                clocks: ctx.aggregate_clock_reader(),
            },
            seed: ctx.aggregate_basis_seed(),
        }
    }

    /// Seed from a context the producer may or may not have been given.
    ///
    /// Producers with a request context are bound to its exact basis;
    /// context-free utility callers remain explicitly unbound. No context is
    /// fabricated as a fallback.
    #[must_use]
    pub fn from_optional_ctx<C: crate::resolver_core::ResolverCapabilities<Clocks = W>>(
        host: &'h crate::VerterHost,
        ctx: Option<&'h dyn crate::resolver_core::ResolverContext<C>>,
    ) -> Self {
        match ctx {
            Some(ctx) => Self::from_ctx(ctx),
            None => Self::unbound_with_clocks(host),
        }
    }

    /// [`FactTracerBasisSource::unbound`] at any clock type, so an optional
    /// context's two arms share one source type.
    #[must_use]
    fn unbound_with_clocks(host: &'h crate::VerterHost) -> Self {
        Self {
            authority: BasisAuthority::Unbound {
                overflow: &host.signature_overflow_at_install,
                #[cfg(any(test, feature = "test-support"))]
                non_cacheable: &host.test_force.engine.force_fact_tracer_non_cacheable_read,
                #[cfg(any(test, feature = "test-support"))]
                observations: &host
                    .test_force
                    .engine
                    .force_fact_tracer_overflow_observations,
            },
            seed: verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
        }
    }

    fn record_signature_overflow(&self) {
        match &self.authority {
            BasisAuthority::Bound { port, .. } => port.record_signature_overflow(),
            BasisAuthority::Unbound { overflow, .. } => {
                overflow.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    fn tracer_forcing(&self) -> (bool, usize) {
        match &self.authority {
            BasisAuthority::Bound { port, .. } => port.tracer_forcing(),
            BasisAuthority::Unbound {
                non_cacheable,
                observations,
                ..
            } => (
                non_cacheable.load(std::sync::atomic::Ordering::Relaxed),
                observations.load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }

    /// Open a tracer scope carrying this source's basis. Thin forward to
    /// the one chokepoint, so a raw consumer that needs the finalised set
    /// installs a basis by the same route the helpers do.
    #[must_use]
    pub fn with_fact_tracer<F, R>(&self, f: F) -> (R, FactReadSet)
    where
        F: FnOnce() -> R,
    {
        self.with_fact_tracer_cell(|_cell| f())
    }

    /// [`Self::with_fact_tracer`], handing the closure the scope's cell.
    #[must_use]
    pub fn with_fact_tracer_cell<F, R>(&self, f: F) -> (R, FactReadSet)
    where
        F: FnOnce(&verter_session_query::facts::fact_read_set::FactReadSetCell) -> R,
    {
        crate::resolver_core::resolver_context::with_fact_tracer_cell(self.live_basis(), f)
    }

    /// Re-compose the basis against the CURRENT live counters.
    ///
    /// `O(1)`: the seed is already captured, so this is five atomic loads
    /// and a struct build. It is the comparison side of movement
    /// detection, and it is deliberately the SAME composition
    /// installation used — a domain this source cannot answer for is
    /// absent on both sides and never registers as spurious movement,
    /// while a clock that becomes unreadable mid-scope correctly does.
    #[must_use]
    pub(crate) fn live_basis(
        &self,
    ) -> verter_session_query::facts::fact_cache::AggregateGenerations {
        match &self.authority {
            BasisAuthority::Bound { clocks, .. } => {
                verter_session_query::facts::fact_cache::AggregateGenerations::from_seed(
                    &self.seed,
                    &clocks.live(),
                )
            }
            BasisAuthority::Unbound { .. } => {
                verter_session_query::facts::fact_cache::AggregateGenerations::default()
            }
        }
    }
}

/// Re-read the live generations into a tracer and record MUTATION
/// INSTABILITY if a domain the scope compacts against has moved.
///
/// Short-circuits on a scope with no basis: such a scope mints no
/// aggregate, so no generation movement can corrupt it and the live read
/// would tell it nothing. That keeps the check byte-cheap for every
/// tracer whose source vouches for nothing.
#[inline]
fn note_basis_recheck<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
    source: &FactTracerBasisSource<'_, W>,
    read_set: &mut FactReadSet,
) {
    if !read_set.aggregate_basis().names_any_domain() {
        return;
    }
    read_set.note_basis_recheck(&source.live_basis());
}

/// [`note_basis_recheck`] through a scope's cell, for the CACHEABILITY
/// seam — which reads its verdict MID-SCOPE and can authorise writes
/// from inside its own closure, so an exit-only check would run after
/// the write it was meant to gate.
#[inline]
fn note_basis_recheck_on_cell<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
    source: &FactTracerBasisSource<'_, W>,
    cell: &verter_session_query::facts::fact_read_set::FactReadSetCell,
) {
    if !cell.has_aggregate_basis() {
        return;
    }
    cell.note_basis_recheck(&source.live_basis());
}

/// Test-only fact-injection hook read at every tracer scope entry
/// ([`install_fact_tracer`] and [`with_cacheability_scope`]). When a knob is
/// non-zero, fan up to one-over-cap synthetic observations across TWO fact
/// domains into the freshly-installed tracer (and every enclosing one), so the
/// scope deterministically reports overflow once the per-signature cap is
/// exceeded. Splitting the observations keeps both per-domain buckets below
/// their compaction threshold; a single wide `FileWholeHash` bucket would now
/// compact to one terminal Content aggregate and would no longer exercise the
/// refusal rail. This is the in-process equivalent of a genuinely wide
/// multi-domain compute without a pathological workspace fixture.
///
/// `scope` is the entering scope's ADDRESSABLE identity: `Some(_)` for a scope
/// opened through [`named_cacheability_scope`] / [`named_fact_tracer`], `None`
/// for every other (unnamed) scope in the crate.
///
/// TWO knobs, deliberately:
///
/// - the PER-HOST STICKY `force_fact_tracer_overflow_observations` overflows
///   EVERY scope in the flow, named or not — the right tool when the boundary
///   under test is the only one whose overflow can refuse the publication;
/// - the THREAD-SCOPED TARGETED ONE-SHOT
///   (`engine_test_knobs::arm_fact_tracer_overflow_once`) is claimed by the NAMED
///   scope it was armed for, on the arming thread, and overflows that scope
///   ALONE. It is the seam for a flow with TWO tracers where either overflow
///   would independently refuse the same write: the sticky knob is
///   non-discriminating there (the test passes even if the boundary under test
///   drops its overflow), while the one-shot isolates the NAMED scope and proves
///   that boundary's rail on its own.
///
/// The one-shot is claimed by scope IDENTITY, never by scope ORDER. An
/// order-keyed one-shot is silently RETARGETED by any tracer scope added
/// upstream of the scope under test — the test keeps passing while testing a
/// different boundary. Here an unnamed upstream scope passes `None`, claims
/// nothing, and leaves the one-shot armed for its intended claimant; a named
/// scope claims only when it IS the armed target.
///
/// Placed at the SHARED installer so EVERY traced admission boundary runs it
/// rather than relying on a boundary-specific hook — a production site that
/// reverts to a raw, overflow-discarding tracer still fans the observations and
/// still fails the test. The production build compiles it out.
#[cfg(any(test, feature = "test-support"))]
fn force_tracer_overflow_observations<
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    scope: Option<crate::engine_test_knobs::TracerScope>,
) {
    let (non_cacheable, sticky) = source.tracer_forcing();
    if non_cacheable {
        crate::fact_tracing::note_non_cacheable_read_fan_out(
            verter_session_query::facts::reuse::NonCacheableReadReason::FencedServe,
        );
    }
    let once = crate::engine_test_knobs::claim_fact_tracer_overflow_once(scope);
    for i in 0..sticky.max(once).min(FACT_SIGNATURE_CAP + 1) {
        let fact = if i % 2 == 0 {
            FactVersionRef::FileWholeHash {
                canonical_id: format!("__force_tracer_overflow_{i}.ts"),
                hash: [(i & 0xff) as u8; 16],
            }
        } else {
            FactVersionRef::ProjectGeneration {
                generation: u64::MAX - i as u64,
            }
        };
        crate::resolver_core::resolver_context::observe_fan_out(fact);
    }
}

/// [`with_cacheability_scope`] for a scope that carries an ADDRESSABLE
/// [`TracerScope`](crate::engine_test_knobs::TracerScope) identity, so a test can
/// target it by NAME with the one-shot overflow knob.
///
/// Reached only through the [`named_cacheability_scope`] macro, whose production
/// arm expands to the plain, unnamed opener — the identity exists in test-support
/// builds alone.
#[cfg(any(test, feature = "test-support"))]
fn with_cacheability_scope_named<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    scope: crate::engine_test_knobs::TracerScope,
    f: F,
) -> (R, bool)
where
    F: for<'t> FnOnce(&CacheabilityProbe<'t, W>) -> R,
{
    let (value, mut read_set) = source.with_fact_tracer_cell(|cell| {
        force_tracer_overflow_observations(source, Some(scope));
        f(&CacheabilityProbe { cell, source })
    });
    note_basis_recheck(source, &mut read_set);
    let non_cacheable = read_set.mutation_unstable()
        || read_set.non_cacheable_read_observed()
        || read_set.would_overflow();
    (value, non_cacheable)
}

/// [`install_fact_tracer_cacheability`] for an ADDRESSABLE scope. See
/// [`with_cacheability_scope_named`].
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn install_fact_tracer_cacheability_named<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    scope: crate::engine_test_knobs::TracerScope,
    f: F,
) -> (R, bool)
where
    F: FnOnce() -> R,
{
    with_cacheability_scope_named(source, scope, |_probe| f())
}

/// [`install_fact_tracer`] for an ADDRESSABLE scope. See
/// [`with_cacheability_scope_named`].
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn install_fact_tracer_named<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    scope: crate::engine_test_knobs::TracerScope,
    f: F,
) -> (R, FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    let (value, mut read_set) = source.with_fact_tracer(|| {
        force_tracer_overflow_observations(source, Some(scope));
        f()
    });
    note_basis_recheck(source, &mut read_set);
    let finalise = read_set.finalise();
    if matches!(finalise, FactReadSetFinalise::Overflow) {
        crate::request_observers::push_structured_event(
            verter_audit::structured_event::StructuredAuditEvent::FactSignatureOverflow {
                candidate_size: (FACT_SIGNATURE_CAP as u32).saturating_add(1),
                cap: FACT_SIGNATURE_CAP as u32,
            },
        );
        source.record_signature_overflow();
    }
    (value, finalise)
}

/// Open an [`install_fact_tracer_cacheability`] scope that a test can TARGET BY
/// NAME.
///
/// ```ignore
/// named_cacheability_scope!(host, TracerScope::ScriptFactsImportRoute, || { .. })
/// ```
///
/// ZERO production footprint: the arm compiled without test support expands to the plain
/// [`install_fact_tracer_cacheability`] call and DROPS the scope tokens entirely
/// — no argument, no `&'static` datum, no type. The identity exists only where a
/// test can consume it, and the production build is byte-identical to the unnamed
/// call it replaces.
///
/// Naming a scope is what makes the one-shot overflow knob TARGETED instead of
/// positional: the knob is claimed by identity, so a tracer scope added anywhere
/// UPSTREAM cannot silently retarget it (an unnamed scope claims nothing).
#[cfg(any(test, feature = "test-support"))]
macro_rules! named_cacheability_scope {
    ($source:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_cacheability_named($source, $scope, $f)
    };
}

#[cfg(not(any(test, feature = "test-support")))]
macro_rules! named_cacheability_scope {
    ($source:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_cacheability($source, $f)
    };
}

/// Open an [`install_fact_tracer`] scope that a test can TARGET BY NAME. The
/// signature-CONSUMING sibling of [`named_cacheability_scope`]; same zero
/// production footprint.
#[cfg(any(test, feature = "test-support"))]
macro_rules! named_fact_tracer {
    ($source:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer_named($source, $scope, $f)
    };
}

#[cfg(not(any(test, feature = "test-support")))]
macro_rules! named_fact_tracer {
    ($source:expr, $scope:expr, $f:expr) => {
        $crate::fact_signature_helpers::install_fact_tracer($source, $f)
    };
}

pub(crate) use {named_cacheability_scope, named_fact_tracer};

/// Proof that the current compute runs inside a CACHEABILITY TRACER SCOPE —
/// the token every shared-cache admission point requires.
///
/// A [`CacheabilityProbe`] can be minted ONLY by [`with_cacheability_scope`],
/// and the borrow it hands out cannot outlive that scope's closure. An
/// admission API that takes `&CacheabilityProbe` therefore CANNOT be reached
/// from a producer that installed no tracer — the untraced-producer class is
/// closed by the type system.
///
/// [`Self::non_cacheable`] reads the scope's verdict-so-far. The tracer
/// accumulates monotonically, so a read taken at the admission point (the END
/// of the value's compute) covers everything the compute consumed — provided
/// the scope ENCLOSES that compute. That is the discipline every producer
/// follows: the scope is the OUTERMOST bracket of the producer body, so key
/// computation, gate classification, lowering, peek, and reduce all lie inside
/// it and a pre-tracer read point cannot exist.
///
/// **`pub` but UNNAMEABLE.** The enclosing module is `pub(crate)`, so no
/// out-of-crate caller can write this type — it is `pub` only so it can appear
/// in the signature of a shared-cache funnel that IS out-of-crate reachable
/// (`RouteDb`, `ImportedRootDb`). Such a caller obtains one the only way anyone
/// does: by opening a real scope (`for_tests::with_cacheability_scope_for_tests`)
/// and receiving the borrow. The `cell` field stays private, so the token
/// cannot be constructed by struct literal either.
pub struct CacheabilityProbe<'t, W> {
    cell: &'t verter_session_query::facts::fact_read_set::FactReadSetCell,
    /// The scope's basis source, RETAINED so [`Self::non_cacheable`] can
    /// re-compose the live basis in `O(1)`. Held because this probe is an
    /// ADMISSION BOUNDARY in its own right — it can authorise a write
    /// from inside the scope's closure, so its verdict must include a
    /// fresh movement check rather than inheriting one taken on exit.
    /// Retaining the SOURCE rather than the host is what keeps that
    /// per-admission check off the store-view read path.
    source: &'t FactTracerBasisSource<'t, W>,
}

impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone> CacheabilityProbe<'_, W> {
    /// `true` when the enclosing scope's compute MUST NOT warm any shared
    /// cache. TWO INDEPENDENT non-admission conditions fold into it:
    ///
    /// 1. a NON-CACHEABLE READ — a FENCED (ReturnOnly, `store_published ==
    ///    false`) `IndexedReady` serve, a broken decl-body lease
    ///    (`LeaseMiss`), an unrootable / unadmitted import route, or an
    ///    unobservable contributor source env. The value was derived from a
    ///    served-without-publication / transient basis while its fact stamps
    ///    read the LIVE view, so the read-side fact rail cannot reject the
    ///    entry.
    /// 2. a fact-signature OVERFLOW — the compute observed more than
    ///    [`FACT_SIGNATURE_CAP`] distinct facts.
    ///
    /// Both are CACHE-ONLY: the value stays `Complete` and flows to the caller
    /// verbatim; only the shared-cache admission is refused (never
    /// `ResultCompleteness::Partial`).
    ///
    /// # Why an overflow refuses here
    ///
    /// NOT because the entry would be unrootable — at these boundaries it
    /// WOULD be rootable: the entry's `ReadSetSignature.facts` is built from
    /// ANOTHER source (the carrier's `dep_signature` via
    /// `engine_fact_signature_for_materialize_memo`, or the keyed canonical's
    /// observed hash), never from this tracer's finalised set, and that
    /// curated signature is well under the cap. The refusal is a conservative
    /// POLICY: an over-cap observation set means the compute read MORE than
    /// the curated signature enumerates, so we can no longer prove the
    /// signature COVERS everything the value depends on — a warm hit could
    /// validate the curated facts while an unenumerated dependency has moved.
    /// The rail therefore has a real cost (a legitimately fact-heavy compute
    /// is recomputed cold forever); it is not free correctness bookkeeping.
    /// # Why MUTATION INSTABILITY refuses here too
    ///
    /// A third condition folds in: a compaction domain this scope
    /// compacts against advanced since its basis was installed. It is
    /// checked HERE, at the probe, and not only on scope exit, because
    /// this probe can authorise a write from inside the closure — an
    /// exit-only check would run after the write it was meant to gate.
    /// It is a STABILITY refusal, not a cardinality one, and the
    /// distinction is preserved in the typed
    /// `FactReadSetFinalise::MutationUnstable` that the
    /// signature-consuming boundary reports; this Boolean surface
    /// carries the refusal, not its taxonomy.
    #[inline]
    pub fn non_cacheable(&self) -> bool {
        // Overflow is peeked, never finalised: no `Arc<[FactVersionRef]>`
        // allocation on the hot cold-member path, and no audit event — the
        // event stays owned by the ONE signature-consuming `install_fact_tracer`
        // boundary per compute (see its emission site).
        note_basis_recheck_on_cell(self.source, self.cell);
        self.cell.mutation_unstable()
            || self.cell.non_cacheable_read_observed()
            || self.cell.would_overflow()
    }
}

/// Open a CACHEABILITY TRACER SCOPE around a producer's ENTIRE compute and hand
/// it the scope's [`CacheabilityProbe`].
///
/// **The scope must be the OUTERMOST bracket of the producing function.** An
/// admission into a shared cache is fail-closed only if its whole compute — key
/// computation, gate classification, lowering, peek, and reduce — runs inside a
/// cacheability tracer. A tracer that starts LATE (after the lowering, say)
/// leaves a pre-tracer read point whose fenced serve is never observed, and no
/// downstream re-observation is guaranteed: structural-transit reduction does
/// not descend into composite children, so a nested reference resolved during
/// lowering is never re-read by the reduce.
///
/// Returns `(value, non_cacheable)` — the same verdict [`CacheabilityProbe`]
/// reports, sampled once after the scope pops, for a producer that admits
/// AFTER its compute rather than inside it.
pub fn with_cacheability_scope<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    f: F,
) -> (R, bool)
where
    F: for<'t> FnOnce(&CacheabilityProbe<'t, W>) -> R,
{
    let (value, mut read_set) = source.with_fact_tracer_cell(|cell| {
        #[cfg(any(test, feature = "test-support"))]
        force_tracer_overflow_observations(source, None);
        f(&CacheabilityProbe { cell, source })
    });
    note_basis_recheck(source, &mut read_set);
    let non_cacheable = read_set.mutation_unstable()
        || read_set.non_cacheable_read_observed()
        || read_set.would_overflow();
    (value, non_cacheable)
}

/// [`with_cacheability_scope`] for a producer whose admission happens AFTER the
/// traced compute returns, so it needs the verdict but not the in-scope probe.
///
/// Use this entry at every admission boundary whose entry signature is built
/// from ANOTHER source (the carrier's `dep_signature`, the keyed canonical's
/// observed hash) rather than from this tracer's finalised set — those callers
/// have no other place to see an overflow, and folding it into the verdict here
/// makes it impossible to drop. A caller that DOES consume the finalised
/// signature (building the entry's `ReadSetSignature` from it) uses
/// [`install_fact_tracer`] and routes `Overflow` through
/// [`SignatureAdmission::from_finalise`].
///
/// Present exactly where a reader is compiled: the unnamed arm of
/// [`named_cacheability_scope`] (a build without test support) and this
/// module's own tests. A test-support build opens every such scope named.
#[cfg(any(test, not(feature = "test-support")))]
pub(crate) fn install_fact_tracer_cacheability<
    F,
    R,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    source: &FactTracerBasisSource<'_, W>,
    f: F,
) -> (R, bool)
where
    F: FnOnce() -> R,
{
    with_cacheability_scope(source, |_probe| f())
}

/// Fan `sig` into every active tracer on the current thread's stack.
///
/// Thin wrapper around
/// [`crate::resolver_core::resolver_context::observe_fan_out_borrowed`]
/// with a more intention-revealing name for callers in the
/// fact-cache substrate.
#[inline]
pub(crate) fn observe_fact_signature(sig: &[FactVersionRef]) {
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(sig);
}

/// Record a contributor SOURCE-ENV identity observation
/// ([`FactVersionRef::FileSourceEnv`]) from the EXACT artifact key the
/// contributor read actually used.
///
/// `canonical_id`, `parse_key`, and `file_language_id` are sourced
/// from `artifact_key` itself, never re-derived from a canonical/path
/// at the recording site and never read back from an index entry that
/// could be stale. The `parse_env_hash` dimension is DIFFERENT: it is
/// the canonical's LIVE per-canonical parse env — the SAME dimension
/// the contributor `LowerLocator` body-source key folds — sourced
/// through the shared
/// [`crate::resolver_store::live_source_env_identity`]
/// construction the validate-side snapshot seeding also uses, so
/// record and validate compare the same dimension by construction. The
/// key's own `parse_env_hash` slot must NOT be copied into the fact: a
/// base key carries the zero sentinel there and an overlay-scoped key
/// a session discriminator — neither is an env identity, and a copied
/// sentinel would make a live parse-env move (content unchanged)
/// invisible to the rail. The fact is recorded onto the active fact
/// tracer (via [`ResolverContext::observe`]) and returned so the
/// caller can fold it into a producer-built signature.
///
/// `artifact_key = None` — the read could not supply the exact key it
/// served from — means the contributor's coherent 4-field identity is
/// UNOBSERVABLE: the API returns `None` and records NOTHING (never a
/// fabricated default). The caller must route the surrounding result
/// through `ReturnOnly` (no warm admission), matching the
/// unobservable-fact convention of the sibling
/// [`parse_fact_ref_for_observed_current_content`] builder.
///
/// The recording site is the cross-file module-augmentation contributor
/// fold (`collect_augmentation_contributions`): one observation per
/// contributor body folded into a parent value, so a warm parent hit
/// revalidates each contributor's source-env identity against the live
/// view.
pub(crate) fn observe_file_source_env_from_artifact_key<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn ResolverContext<C>,
    artifact_key: Option<&verter_session_query::source::artifact_key::FileArtifactKey>,
) -> Option<FactVersionRef> {
    let key = artifact_key?;
    let identity = ctx.source_environment(key);
    let fact = FactVersionRef::FileSourceEnv {
        canonical_id: key.canonical.as_ref().to_owned(),
        parse_env_hash: identity.parse_env_hash,
        parse_key: identity.parse_key,
        file_language_id: identity.file_language_id,
    };
    ctx.observe(fact.clone());
    Some(fact)
}

/// Convert a [`DepSignature`] into a [`Vec<FactVersionRef>`] — the
/// bridge that fans a dispatch sub-query's recorded dependency set
/// into the active fact tracer.
///
/// Per-version mapping (no generation dep is silently dropped):
///
/// - `WholeHash` → `FileWholeHash`.
/// - `ProjectGeneration` → `FactVersionRef::ProjectGeneration` — the
///   project-wide generation a sub-result depended on. Dropping it
///   would let an outer entry that observed the sub-result through the
///   tracer validate against a superseded project shape.
/// - `RouteGeneration` is **not expressible** as a `FactVersionRef` —
///   there is no `FactVersionRef::RouteGeneration` variant (route
///   generation has no authoritative validating source) — so it is
///   skipped. No production path constructs `DepVersion::RouteGeneration`;
///   this arm is the defensive floor.
pub(crate) fn dep_signature_to_fact_signature(sig: &DepSignature) -> Vec<FactVersionRef> {
    sig.iter()
        .filter_map(|(canon, ver)| match ver {
            DepVersion::WholeHash(h) => Some(FactVersionRef::FileWholeHash {
                canonical_id: canon.as_ref().to_string(),
                hash: *h,
            }),
            DepVersion::ProjectGeneration(generation) => Some(FactVersionRef::ProjectGeneration {
                generation: *generation,
            }),
            DepVersion::RouteGeneration(_) => None,
        })
        .collect()
}

/// Walk every `FactVersionRef` in `signature` against the current
/// resolver-store view; return `false` on the first mismatch.
///
/// `O(signature.len())`; zero allocation on the empty path. Empty
/// signatures trivially validate (callers that never observed a fact
/// have no R3 oracle to consult — typical for cache entries produced
/// outside an installed tracer scope; the cache stays correct under
/// the legacy whole-hash regime).
///
/// This is the lazy (non-strict) validator. Production warm reads use
/// [`validate_fact_signature_with_self_roots`]; the only consumer of
/// the lazy form is the `cfg(any(test, feature = "test-support"))`-gated
/// `AppConfigNoOverrideProofDb::peek` plus the substrate test suite, so
/// it is gated to match (no dead surface in release).
#[cfg(any(test, feature = "test-support"))]
#[cfg_attr(not(test), allow(dead_code))]
#[inline]
#[track_caller]
pub(crate) fn validate_fact_signature(
    ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    signature: &[FactVersionRef],
) -> bool {
    signature.is_empty() || ctx.validates_fact_signature(signature)
}

/// Walk `signature` against the current resolver-store view, but
/// validate any `FileWholeHash` fact whose canonical appears in
/// `self_root_canonicals` **strictly** — an untracked or mismatched
/// self-root canonical fails validation.
///
/// This is the validation entry point for a query-identity cache
/// whose `read_set_signature` carries a self-root `FileWholeHash` for
/// its own keyed canonical (the signature shape produced by the three
/// central fact-signature helpers above). [`validate_fact_signature`]
/// alone routes a `FileWholeHash` through the lazy
/// [`verter_session_query::facts::store_view::StoreView::validates`] rule, whose
/// untracked-file arm optimistically accepts: that is correct for a
/// cross-file *dependency* fact (loaded after the view snapshot) but
/// wrong for a *self-root*, where an untracked keyed canonical means
/// the entry's own file is gone and the entry must miss.
///
/// `self_root_canonicals` is the explicit self-root-vs-dependency
/// distinction: a `FileWholeHash` whose canonical is listed is a
/// self-root and routes through the strict
/// [`verter_session_query::facts::store_view::StoreView::validates_self_root_whole_hash`];
/// every other fact (including a `FileWholeHash` for a non-listed
/// cross-file dependency) routes through the lazy `validates`, so
/// cross-file lazy permissiveness is preserved. Empty signatures
/// trivially validate.
///
/// This is the warm-read validation entry point for the
/// component-meta query-identity caches: each cache passes its own
/// keyed canonical(s) as `self_root_canonicals`, so a same-canonical
/// content edit — or a keyed canonical that became untracked — fails
/// validation strictly and the warm read recomputes.
#[inline]
#[track_caller]
pub(crate) fn validate_fact_signature_with_self_roots(
    ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    signature: &[FactVersionRef],
    self_root_canonicals: &[&str],
) -> bool {
    signature.is_empty()
        || ctx.validates_fact_signature_with_self_roots(signature, self_root_canonicals)
}

/// Bubble `signature` into **all** active fact tracers on the current
/// thread's stack (fan-out). Called by both cold-compute and warm-hit
/// paths so every outer tracer scope sees every transitive fact the
/// inner cache hit / produced.
#[inline]
pub(crate) fn bubble_fact_signature(
    _ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    signature: &[FactVersionRef],
) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Variant of [`bubble_fact_signature`] for warm-hit paths that
/// don't carry a `ResolverContext` reference (e.g. the semantic
/// graph store's fast-path warm hit). Fans into all active TLS
/// tracer scopes when any are installed; no-op otherwise.
#[inline]
pub(crate) fn bubble_fact_signature_via_tls(signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Build a [`ParseFactRef`] for `(canonical_id, key, lane)` pinned to a
/// caller-supplied **observed** content hash — a provenance-pure parse
/// fact that records the exact file identity a producer observed.
///
/// The supplied hash must equal the current content identity visible through
/// `ctx`. That request-bound authority contributes the exact source-derived
/// `ParseKey` and runtime `FileLanguage`, so a base context cannot recover an
/// overlay-only version and an overlay context cannot silently fall back to
/// base content. The subsequent content-addressed
/// [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore) lookup
/// may ignore `parse_env_hash` because parse facts are content-derived, but it
/// remains exact in content, parse identity, and language.
///
/// ## Two-identity recovery — raw owner vs analysis canonical
///
/// `canonical_id` is the **raw** owner the caller observed. Two ids are
/// in play and they MUST NOT be conflated:
///
/// * The **artifact-store lookup** is keyed by
///   `normalized_analysis_canonical(canonical_id)` — every
///   `FileArtifactStore` artifact (base via [`crate::resolver_core::request_ports::IndexedInputs::ensure_indexed_ready_serve`],
///   overlay via the overlay materialiser) is published under the
///   normalised analysis canonical as `FileArtifactKey::canonical`. A
///   lookup keyed by the raw owner misses the artifact whenever
///   `normalize(raw) != raw` (a runtime `.js` with a `.d.ts`
///   companion) — the recovery would then return `None` even though the
///   observed parse facts exist.
/// * The emitted **`ParseFactRef.canonical_id`** stays the RAW owner the
///   caller passed. The parse-domain validator
///   ([`verter_session_query::facts::store_view::StoreView::validates_parse_domain`]) keys
///   the per-file `FileFacts` snapshot by the canonical the view tracks:
///   an overlay-bearing canonical is re-rooted in
///   [`crate::resolver_store::HostStoreView::with_session_overlay`] under
///   the RAW overlay owner, and the materialize-memo signature builder
///   (`engine_fact_signature_for_materialize_memo`) requires the parse
///   fact's id to equal the observation's raw scope id. Normalising the
///   emitted id would break both.
///
/// `None` is returned when the observed hash is not current in this request
/// view, or when no artifact is cached for its exact
/// `(analysis_canonical, content_hash, parse_key, file_language)` identity.
/// The caller must then refuse shared-cache admission rather than emit a fact
/// rooted on a guessed hash.
///
/// This is the construction primitive for a cache entry whose value
/// was materialised against a specific observed file version: the
/// entry's parse fact MUST be pinned to that same observed version so
/// a warm read after a content edit genuinely misses, instead of
/// validating the observed-version fact against post-edit content. The
/// bare `ParseFactRef` (not a wrapped `FactVersionRef`) is returned so
/// a producer that roots the same canonical multiple ways can place it
/// exactly once.
pub(crate) fn parse_fact_ref_for_observed_current_content<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn ResolverContext<C>,
    canonical_id: &str,
    observed_content_hash: Hash16,
    key: FactKey,
    lane: FactLane,
) -> Option<ParseFactRef> {
    ctx.parse_fact_for_observed_content(canonical_id, observed_content_hash, key, lane)
}

/// Emit a self-root `FileWholeHash` for `canonical_id` pinned to a
/// caller-supplied **observed** content hash.
///
/// A self-root is the whole-hash fact for a cache entry's OWN keyed
/// canonical. The hash is NOT re-read from current content: it is the
/// content version the producer observed at the value source and
/// threaded into the signature builder. Pinning the self-root to the
/// observed version is what makes the value and its signature root on
/// one content identity — a re-read of the canonical's *current* hash
/// would root a stale value on post-edit content when an `upsert`
/// lands in the publish race window.
#[inline]
fn observed_self_root_fact(canonical_id: &str, observed_hash: Hash16) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical_id.to_string(),
        hash: observed_hash,
    }
}

/// Build a provenance-pure, path-precise signature for a cache whose
/// validity depends on a single MEMBER of an exporter type.
///
/// The builder is **provenance-pure**: it never consults the
/// authoritative current-content oracle and never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the producer's
/// value was actually computed against, captured once at the value
/// source. The signature leads with a self-root `FileWholeHash` pinned
/// to that observed hash, then adds the path-precise parse facts:
/// `MemberPresence(exporter, member, space)` (header fact — bumps on
/// add/remove/rename/kind-change) and `Member(exporter, member,
/// space)` (body fingerprint — bumps on body edit). Both parse facts
/// are content-addressed against `observed_hash` via
/// [`parse_fact_ref_for_observed_current_content`] — they record the
/// fact hashes live when the producer observed the file, not whatever
/// is current at signature-build time.
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered (no
/// content-addressed artifact for `(canonical_id, observed_hash)`).
/// The caller still returns the freshly-computed value, it only
/// forgoes the shared cache.
///
/// Use this helper for caches keyed on `(canonical, exporter,
/// member, space)` — slot-binding member reads and member-keyed
/// dispatch member projection.
///
/// Test-only: no production producer composes a member-keyed signature
/// this way (its former dedicated walker-DB consumer was deleted). The
/// `query_identity_self_root_substrate_tests` substrate suite exercises
/// this helper to characterise the observed-hash self-root prepend for
/// member-keyed scopes, matching the `fact_signature_for_canonical_surface`
/// precedent.
#[cfg(any(test, feature = "test-support"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn fact_signature_for_canonical_member<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn ResolverContext<C>,
    canonical_id: &str,
    exporter: &str,
    member: &str,
    space: SymbolSpace,
    observed_hash: Hash16,
) -> SignatureAdmission {
    let exporter_name = InternedName::from(exporter);
    let member_name = verter_type_expr::facts::FactPropertyKey::identifier(member);
    let presence_key = FactKey::MemberPresence {
        exporter: exporter_name.clone(),
        name: member_name.clone(),
        space,
    };
    let body_key = FactKey::Member {
        exporter: exporter_name,
        name: member_name,
        space,
    };
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the path-precise `MemberPresence` / `Member` parse facts pinned
    // to the SAME observed content version.
    let presence_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        presence_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let body_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        body_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(presence_fact),
        FactVersionRef::Parse(body_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Build a provenance-pure signature for a cache whose validity
/// depends on the IDENTITY of a top-level type declared at
/// `canonical_id` — the Family A producer pattern for caches keyed on
/// `(canonical, type_name)`.
///
/// The builder is **provenance-pure**: it never consults the
/// authoritative current-content oracle and never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the producer's
/// value was computed against, captured once at the value source. The
/// signature leads with a self-root `FileWholeHash` pinned to that
/// observed hash, then adds the top-level-identity parse facts:
/// - `Export(name, space)` — present iff the type is exported under
///   that name.
/// - `LocalDecl(name, space)` — present iff the type is declared
///   locally (non-exported).
/// - `MemberShape(exporter=name, space)` — the ordered member list
///   fingerprint; bumps when members are added/removed/renamed.
///
/// All three parse facts are content-addressed against `observed_hash`
/// via [`parse_fact_ref_for_observed_current_content`].
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered. The caller
/// still returns the freshly-computed value.
pub(crate) fn fact_signature_for_exported_type<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn ResolverContext<C>,
    canonical_id: &str,
    type_name: &str,
    space: SymbolSpace,
    observed_hash: Hash16,
) -> SignatureAdmission {
    let name = InternedName::from(type_name);
    let export_key = FactKey::Export {
        name: name.clone(),
        space,
    };
    let local_decl_key = FactKey::LocalDecl {
        name: name.clone(),
        space,
    };
    let member_shape_key = FactKey::MemberShape {
        exporter: name,
        space,
    };
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the top-level-identity `Export` / `LocalDecl` / `MemberShape`
    // parse facts pinned to the SAME observed content version.
    let export_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        export_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let local_decl_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        local_decl_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let member_shape_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        member_shape_key,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(export_fact),
        FactVersionRef::Parse(local_decl_fact),
        FactVersionRef::Parse(member_shape_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Build a provenance-pure, whole-canonical signature for a cache
/// whose cold-compute reads the file's surface fingerprint (e.g. a
/// binding-walker that enumerates every export).
///
/// The builder is **provenance-pure**: it never re-reads current
/// content. The keyed canonical's content identity is supplied by the
/// caller as `observed_hash` — the content version the value was
/// computed against, captured once at the value source. The signature
/// leads with a self-root `FileWholeHash` pinned to that observed
/// hash, then observes the `SyntacticExportSet` parse fact
/// content-addressed against the SAME observed version via
/// [`parse_fact_ref_for_observed_current_content`]. Returns `None`
/// when the observed version's parse-fact registry cannot be
/// recovered, refusing shared-cache admission.
///
/// Test-only: no production producer composes a whole-surface
/// signature this way. The `query_identity_self_root_substrate_tests`
/// substrate suite exercises this helper to characterise the
/// observed-hash self-root prepend.
///
/// Returns [`SignatureAdmission::NonCacheable`] with
/// [`NonAdmissionReason::UnresolvedProvenance`] when the observed
/// version's parse-fact registry cannot be recovered.
#[cfg(any(test, feature = "test-support"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn fact_signature_for_canonical_surface<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn ResolverContext<C>,
    canonical_id: &str,
    observed_hash: Hash16,
) -> SignatureAdmission {
    // Lead with the observed-hash self-root `FileWholeHash`, then add
    // the `SyntacticExportSet` surface parse fact pinned to the SAME
    // observed content version.
    let surface_fact = match parse_fact_ref_for_observed_current_content(
        ctx,
        canonical_id,
        observed_hash,
        FactKey::SyntacticExportSet,
        FactLane::Semantic,
    ) {
        Some(fact) => fact,
        None => {
            return SignatureAdmission::NonCacheable(NonAdmissionReason::UnresolvedProvenance);
        }
    };
    let entries: Vec<FactVersionRef> = vec![
        observed_self_root_fact(canonical_id, observed_hash),
        FactVersionRef::Parse(surface_fact),
    ];
    SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(entries)))
}

/// Empty signature constructor for cache entries published outside
/// any observable cold-compute pass (e.g. test fixtures, synthetic
/// publish paths). Validator trivially accepts; readers fall back to
/// the existing whole-hash regime in the legacy producer.
#[inline]
pub(crate) fn empty_fact_signature() -> Arc<[FactVersionRef]> {
    Arc::from(Vec::<FactVersionRef>::new())
}

/// Maps a `(canonical, DepVersion)` fence accumulator into the
/// `Arc<[FactVersionRef]>` form a fact-signature carrier holds (the
/// owner-import-surface entry is the live consumer).
///
/// Per-version mapping (no generation dep is silently dropped — a
/// dropped generation dep would let the warm-cache validator confirm
/// a value rooted on a superseded project shape):
///
/// - `WholeHash` → `FileWholeHash` — the observed content version.
/// - `ProjectGeneration` → `FactVersionRef::ProjectGeneration` — a
///   project-shape change bumps the counter and rejects the entry; a
///   pure file-content edit never bumps it, so this does not
///   over-invalidate.
/// - `RouteGeneration` → the whole function returns `None`. Route
///   generation has no authoritative validating source (there is no
///   production emitter and the fence validator treats it as
///   always-valid), so an entry rooted on it could not detect a
///   content edit to the route-observed file. A `None` result signals
///   the value is not safely cacheable; the caller must not admit it.
#[must_use]
pub(crate) fn fact_signature_from_fence(
    fence: &[(Arc<str>, crate::semantic_query::DepVersion)],
) -> Option<Arc<[FactVersionRef]>> {
    use crate::semantic_query::DepVersion;
    let mut out: Vec<FactVersionRef> = Vec::with_capacity(fence.len());
    for (canonical, version) in fence.iter() {
        match version {
            DepVersion::WholeHash(hash) => {
                out.push(FactVersionRef::FileWholeHash {
                    canonical_id: canonical.as_ref().to_string(),
                    hash: *hash,
                });
            }
            DepVersion::ProjectGeneration(generation) => {
                out.push(FactVersionRef::ProjectGeneration {
                    generation: *generation,
                });
            }
            DepVersion::RouteGeneration(_) => {
                // Route generation cannot be soundly rooted — refuse
                // the whole signature rather than rooting an entry on a
                // fact that cannot catch a content edit.
                return None;
            }
        }
    }
    Some(Arc::from(out))
}

/// The pair a structural-carrier signature producer returns: the
/// path-precise `FactVersionRef` signature plus the explicit self-root
/// canonical set the warm-read validator checks **strictly**. A producer
/// returns `Result<StructuralCarrierReadSet, NonAdmissionReason>` so
/// refusal modes (`SelfRootConflict`, `RouteGenerationDependency`) reach
/// the caller verbatim; `ref_cycle_read_set` uses the same typed result for
/// torn roots, unresolved strict-world provenance, and completed-carrier
/// overflow.
pub(crate) type StructuralCarrierReadSet = (Arc<[FactVersionRef]>, Arc<[Arc<str>]>);

/// Finalize a structural cache carrier after every self-root, fence, prelude,
/// and traced fact has been merged.
///
/// Oversized self-root sets are replaced by one strict-world witness only
/// after every root validates in the exact effective view. Any remaining
/// over-cap completed carrier is refused here, at the terminal publication
/// boundary rather than at the earlier tracer finalization boundary.
pub(crate) fn bound_completed_structural_carrier(
    view: &dyn StoreView,
    mut facts: Vec<FactVersionRef>,
    mut self_root_canonicals: Vec<Arc<str>>,
) -> Result<StructuralCarrierReadSet, NonAdmissionReason> {
    facts.sort_unstable();
    facts.dedup();
    self_root_canonicals.sort();
    self_root_canonicals.dedup();

    if facts.len() > FACT_SIGNATURE_CAP && !self_root_canonicals.is_empty() {
        let mut roots: Vec<(Arc<str>, Hash16)> = Vec::with_capacity(self_root_canonicals.len());
        for canonical in &self_root_canonicals {
            let Some(hash) = facts.iter().find_map(|fact| match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash }
                    if canonical_id == canonical.as_ref() =>
                {
                    Some(*hash)
                }
                _ => None,
            }) else {
                return Err(NonAdmissionReason::UnresolvedProvenance);
            };
            roots.push((Arc::clone(canonical), hash));
        }
        let root_refs: Vec<(&str, Hash16)> = roots
            .iter()
            .map(|(canonical, hash)| (canonical.as_ref(), *hash))
            .collect();
        let world = view
            .mint_strict_self_root_world(&root_refs)
            .ok_or(NonAdmissionReason::UnresolvedProvenance)?;

        facts.retain(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. } => !self_root_canonicals
                .binary_search_by(|root| root.as_ref().cmp(canonical_id.as_str()))
                .is_ok(),
            _ => true,
        });
        facts.push(FactVersionRef::StrictSelfRootWorld(world));
        facts.sort_unstable();
        facts.dedup();
        self_root_canonicals.clear();
    }

    if facts.len() > FACT_SIGNATURE_CAP {
        return Err(NonAdmissionReason::SignatureOverflow);
    }
    Ok((Arc::from(facts), Arc::from(self_root_canonicals)))
}

/// A cache entry's dependency signature — the path-precise fact
/// signature captured by an `install_fact_tracer` scope.
///
/// `facts` is the sole cache-validity rail: the fact-tracer
/// observation set the producer recorded. Warm-hit reads validate
/// `facts` against the live store view (self-roots strict, cross-file
/// dependency facts lazy) and bubble it into any active outer tracer.
///
/// The carrier is `pub(crate)`. Cache entries store a single
/// `read_set_signature: ReadSetSignature` field. The shared cold-build
/// helper builds the carrier when the tracer finalises; warm-hit paths
/// call `validate_with_self_roots(ctx, &self_roots)` BEFORE
/// `bubble(ctx)`.
///
/// Invariants:
/// - `validate_with_self_roots(ctx, &self_roots)` returns true only
///   when `facts` validates, with every `FileWholeHash` fact for a
///   listed self-root canonical validated **strictly**. An empty
///   carrier with no self-roots validates vacuously.
/// - `bubble(ctx)` fans `facts` into every active outer tracer on the
///   current TLS stack.
/// - `canonical_ids()` returns the canonical IDs referenced by
///   `facts`, deduplicated by string identity. The reverse index
///   registers a (canonical → entry) mapping for each yielded ID.
/// - `is_overflow()` returns true when the producer's tracer finalised
///   with `FactReadSetFinalise::Overflow` — the materialised result
///   is valid but the path-precise signature is too large to admit
///   safely. Cache consumers route overflowed values through
///   `ComputeAdmission::ReturnOnly` (return without admitting).
pub use verter_session_query::facts::fact_cache::ReadSetSignature;

pub(crate) trait ReadSetSignatureExt {
    fn validate_with_self_roots(
        &self,
        ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
        self_root_canonicals: &[Arc<str>],
    ) -> bool;
    fn has_view_discriminating_self_root(&self, self_root_canonicals: &[Arc<str>]) -> bool;
    fn records_missing_dependency_fact(&self) -> bool;
    fn records_negative_resolution_fact(&self) -> bool;
    fn bubble(&self, ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation);
    fn bubble_via_tls(&self);
}

impl ReadSetSignatureExt for ReadSetSignature {
    /// Validate the fact rail against the host's live state, validating
    /// every `FileWholeHash` fact whose canonical is listed in
    /// `self_root_canonicals` **strictly**.
    ///
    /// Returns `true` only when `facts` validates. Any `FileWholeHash`
    /// for a listed self-root canonical routes through the strict
    /// [`verter_session_query::facts::store_view::StoreView::validates_self_root_whole_hash`]
    /// (an untracked or hash-mismatched self-root fails); every other
    /// fact — including a `FileWholeHash` for a non-listed cross-file
    /// dependency — keeps the lazy
    /// [`verter_session_query::facts::store_view::StoreView::validates`] permissiveness.
    /// An overflow carrier always fails; an empty carrier with no
    /// self-roots validates vacuously.
    ///
    /// This is the strict warm-read validation entry point for a
    /// query-identity cache whose entry records its keyed (or
    /// file-derived input) canonicals as `self_root_canonicals`: a
    /// same-canonical content edit, or a self-root canonical the live
    /// store view no longer tracks, fails validation.
    #[inline]
    #[track_caller]
    fn validate_with_self_roots(
        &self,
        ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
        self_root_canonicals: &[Arc<str>],
    ) -> bool {
        if self.overflowed {
            return false;
        }
        let self_root_refs: Vec<&str> = self_root_canonicals.iter().map(Arc::as_ref).collect();
        validate_fact_signature_with_self_roots(ctx, &self.facts, &self_root_refs)
    }

    /// Whether [`Self::validate_with_self_roots`] would actually
    /// **discriminate by view** for `self_root_canonicals` — i.e.
    /// whether the carrier carries at least one self-root fact that the
    /// strict validator routes through
    /// [`verter_session_query::facts::store_view::StoreView::validates_self_root_whole_hash`].
    ///
    /// `validate_with_self_roots` only rejects a cross-view reuse when a
    /// `FileWholeHash` whose canonical is listed in
    /// `self_root_canonicals` mismatches the live store view. Every
    /// other fact — an empty fact rail, a `FileWholeHash` for a
    /// non-listed cross-file *dependency*, a `ProjectGeneration` rail —
    /// routes through the lazy / permissive path, which an unrelated
    /// overlay validates **vacuously**. So a carrier whose `facts` hold
    /// no `FileWholeHash` for any listed self-root canonical cannot
    /// discriminate a follower running under a different overlay: the
    /// validation passes regardless of the follower's view.
    ///
    /// Returns `true` iff at least one `FileWholeHash` fact in `facts`
    /// has a canonical that appears in `self_root_canonicals`. An
    /// overflow carrier never carries a self-root (`facts` is empty),
    /// an empty `self_root_canonicals` slice can never match, and a
    /// synthetic empty-fact carrier holds no `FileWholeHash` at all —
    /// all three return `false`.
    ///
    /// The in-flight joiner gate uses this to refuse cross-view reuse
    /// of ANY winner whose carrier could only ever validate vacuously —
    /// a tracer-overflow carrier, an unrootable build carrying only
    /// cross-file dependency facts, or a non-suppressed
    /// `QueryResult::Error(Miss)` from a declaration missing under the
    /// winner's overlay. For all of these `validate_with_self_roots` is
    /// not a real view check, so a follower under a possibly-different
    /// overlay must fork and recompute rather than coalesce onto the
    /// winner's view-specific result. The fork is not gated on
    /// `cache_suppress`.
    #[inline]
    fn has_view_discriminating_self_root(&self, self_root_canonicals: &[Arc<str>]) -> bool {
        if self
            .facts
            .iter()
            .any(|fact| matches!(fact, FactVersionRef::StrictSelfRootWorld(_)))
        {
            return true;
        }
        if self_root_canonicals.is_empty() {
            return false;
        }
        self.facts.iter().any(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. } => self_root_canonicals
                .iter()
                .any(|root| root.as_ref() == canonical_id.as_str()),
            _ => false,
        })
    }

    /// Whether this carrier records the invalidation rail that roots a
    /// `Partial(MissingDependency)` result — the §18.2 admission narrowing.
    ///
    /// A missing-dependency result (`import { X } from './missing'` where
    /// `./missing` does not yet exist) is fact-rooted-cacheable ONLY when
    /// the producer recorded the import-route rail: the sealed resolution
    /// transaction's own observations, which include the exhausted probe
    /// set for the miss. When the dependency later appears, the observed
    /// `PathProbe` advances and the warm read misses (lazy cross-file
    /// invalidation, the normal rail). The bare presence of arbitrary
    /// file facts is NOT sufficient — a positive `FileWholeHash` would
    /// warm-admit a degraded result with no rail that the dependency's
    /// appearance can invalidate. The `admit_decision` rule consults
    /// THIS, never the taint enum class, to decide `Warm` vs
    /// `ReturnOnly` for `Partial(MissingDependency)`.
    #[inline]
    fn records_missing_dependency_fact(&self) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact,
                FactVersionRef::ResolveImports(inner) if inner.resolution_fact().is_some()
            )
        })
    }

    /// Whether this carrier records the negative-resolution rail that roots
    /// a `Partial(UnresolvedReference)` result — the §18.2 admission
    /// narrowing.
    ///
    /// An unresolved reference over well-formed syntax is
    /// fact-rooted-cacheable ONLY when the resolver recorded the negative
    /// resolved-import fact — a `ResolvedImportClause` / `ResolvedReexportBinding`
    /// whose `resolved_canonical` is the
    /// [`UNRESOLVED_SENTINEL`](verter_session_query::resolution::unresolved::UNRESOLVED_SENTINEL).
    /// When the reference later resolves, the producer records a real
    /// canonical, the fact's hash shifts, and the warm read misses. A
    /// POSITIVE `ResolveImports` fact must NOT qualify — it carries no
    /// negative rail, so trusting it would warm-admit a degraded result.
    /// `admit_decision` consults THIS, never the taint enum class.
    #[inline]
    fn records_negative_resolution_fact(&self) -> bool {
        self.facts.iter().any(|fact| match fact {
            FactVersionRef::ResolveImports(
                verter_session_query::facts::fact_cache::ResolveImportsFactRef::Semantic {
                    key:
                        FactKey::ResolvedImportClause {
                            resolved_canonical, ..
                        }
                        | FactKey::ResolvedReexportBinding {
                            resolved_canonical, ..
                        },
                    ..
                },
            ) => {
                resolved_canonical.as_ref()
                    == verter_session_query::resolution::unresolved::UNRESOLVED_SENTINEL
            }
            _ => false,
        })
    }

    /// Bubble the path-precise fact set into every active outer
    /// tracer on the current TLS stack. No-op when the tracer stack
    /// is empty or `facts` is empty.
    #[inline]
    fn bubble(&self, ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation) {
        bubble_fact_signature(ctx, &self.facts);
    }

    /// Bubble via TLS only — for fast-path warm hits that don't
    /// thread a `ResolverContext` reference. Equivalent to
    /// `bubble_fact_signature_via_tls(&self.facts)`.
    #[inline]
    fn bubble_via_tls(&self) {
        bubble_fact_signature_via_tls(&self.facts);
    }
}

/// Build the fact signature for a `MaterializeMemoDb` entry.
///
/// A `MaterializeMemoDb` entry caches the materialised form of a type
/// expression in a `scope` canonical. The builder is **provenance-pure**:
/// it never consults the authoritative current-content oracle and
/// never calls a helper that can re-read current content. Every file
/// identity it emits is supplied by the caller as an *observed*
/// value — the content version the materialiser actually worked
/// against.
///
/// The scope's content identity arrives as ONE
/// [`crate::resolver_core::MaterializeScopeObservation`] — a single
/// `Arc<IndexedReady>`. The keyed-scope `whole_hash` and the keyed-scope
/// `SyntacticExportSet` parse fact therefore both descend from the same
/// observation: the builder physically cannot be handed a raw hash
/// from one source and a parse fact from another. The publish site
/// builds the value's `NodeScopeId::File` from the same observation's
/// `whole_hash`, so the memo value and its fact signature root on the
/// identical scope hash — no torn read.
///
/// Parameters:
///
/// - `observed_scope` — the single tear-free scope observation. Its
///   [`crate::resolver_core::MaterializeScopeObservation::whole_hash`]
///   is the keyed-scope self-root hash AND the hash baked into the
///   value's `NodeScopeId::File`.
/// - `observed_scope_syntactic_export_set` — the scope's
///   `SyntacticExportSet` parse fact, pinned to the observation's
///   `whole_hash` (the publish closure unwraps it from
///   `observed_scope.syntactic_export_set` — passing it explicitly
///   keeps the `None`-refuses-admission control flow at the call
///   site). A `debug_assert` confirms it agrees with the observation.
/// - `materialized_dep_signature` — every canonical the materialisation
///   walk observed, each tagged with the
///   [`crate::semantic_query::DepVersion`] the materialiser recorded.
///
/// The keyed scope is self-rooted by an observed-hash `FileWholeHash`
/// plus the observed-version `Parse` fact. Re-reading the scope's
/// *current* hash would be wrong: an edit landing in the race window
/// between materialisation and this signature write-through would
/// otherwise publish the stale `MaterializedOutputTypeExpr` rooted by a
/// fresh-looking current hash, which then validates on warm reads
/// instead of missing.
///
/// Returns `None` when the signature cannot be built strictly enough
/// to admit the entry to the shared memo. A `None` result refuses
/// cache admission only — the caller still returns the
/// freshly-computed `MaterializedOutputTypeExpr`. `None` is returned when:
///
/// - `observed_scope_syntactic_export_set` is a `Parse` fact for a
///   canonical other than the observed scope (caller-supplied
///   observation does not describe the keyed scope), or
/// - an observed dependency names the scope canonical with a
///   `WholeHash` that disagrees with the observation's `whole_hash` (a
///   torn / mixed observation of the scope), or
/// - an observed dependency carries a `RouteGeneration` version (see
///   below).
///
/// Per-`DepVersion` rooting:
///
/// - `DepVersion::WholeHash(observed)` — the materialiser observed
///   that file's content version. The OBSERVED hash is preserved
///   verbatim in the emitted `FileWholeHash`. A dependency entry that
///   names the scope itself is collapsed onto the scope self-root: it
///   must agree with the observation's `whole_hash` or admission is
///   refused.
/// - `DepVersion::ProjectGeneration(observed)` — the materialiser
///   observed the project-wide resolver/config/lib generation, not
///   that file's content. It is rooted by a
///   [`verter_session_query::facts::fact_cache::FactVersionRef::ProjectGeneration`]
///   carrying the OBSERVED generation: a project-shape change bumps
///   the counter and rejects the memo. A pure file-content edit does
///   not bump the generation, so this fact does not over-invalidate.
/// - `DepVersion::RouteGeneration(_)` — route generation is not a
///   real validating fact: there is no authoritative route-generation
///   counter and no production emitter. The fact-rail validator
///   rejects it fail-safe (the `RouteGeneration` arm returns `false`)
///   so a stale entry rooted on it cannot survive. Rooting it would
///   be unsound (it cannot detect a content edit to the observed
///   file). The function therefore returns `None` so the entry is NOT
///   admitted to the shared `MaterializeMemoDb`; no production path
///   constructs the variant.
pub(crate) fn engine_fact_signature_for_materialize_memo(
    observed_scope: &crate::resolver_core::MaterializeScopeObservation,
    observed_scope_syntactic_export_set: verter_session_query::facts::fact_cache::ParseFactRef,
    materialized_dep_signature: &crate::semantic_query::DepSignature,
) -> verter_session_query::facts::fact_cache::SignatureAdmission {
    use crate::cache_runtime::NonAdmissionReason;
    use crate::semantic_query::DepVersion;
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_session_query::facts::fact_cache::SignatureAdmission;

    let scope_canonical_id = observed_scope.canonical_id.as_ref();
    let observed_scope_whole_hash = observed_scope.whole_hash();
    // Both hashes were sampled from the same retained source observation.
    verter_debug_assert_eq!(
        observed_scope.observed_shallow_hash,
        observed_scope_whole_hash,
        "MaterializeScopeObservation must describe one internally-consistent source",
    );

    if observed_scope_syntactic_export_set.canonical_id.as_str() != scope_canonical_id {
        // The supplied parse fact resolves to a DIFFERENT canonical
        // than the keyed scope — provenance is resolved (we have a
        // parse fact), just attributed to the wrong file. This is a
        // self-root / canonical conflict, not unresolved provenance:
        // the fact is fully attributed, only its self-root identity
        // disagrees with the keyed scope. Audit telemetry tracks the
        // two failure modes distinctly.
        return SignatureAdmission::NonCacheable(NonAdmissionReason::SelfRootConflict);
    }

    let mut entries = Vec::with_capacity(2 + materialized_dep_signature.len());

    entries.push(FactVersionRef::FileWholeHash {
        canonical_id: scope_canonical_id.to_string(),
        hash: observed_scope_whole_hash,
    });
    entries.push(FactVersionRef::Parse(observed_scope_syntactic_export_set));

    for (observed_canonical, dep_version) in materialized_dep_signature.iter() {
        match dep_version {
            DepVersion::WholeHash(observed_hash) => {
                if observed_canonical.as_ref() == scope_canonical_id {
                    // The keyed scope is already self-rooted above by
                    // the observed-hash `FileWholeHash`. A dependency
                    // entry for the scope itself must agree with that
                    // single observation; a disagreement is a torn
                    // read and refuses shared admission.
                    if *observed_hash != observed_scope_whole_hash {
                        return SignatureAdmission::NonCacheable(
                            NonAdmissionReason::SelfRootConflict,
                        );
                    }
                    continue;
                }
                entries.push(FactVersionRef::FileWholeHash {
                    canonical_id: observed_canonical.as_ref().to_string(),
                    hash: *observed_hash,
                });
            }
            DepVersion::ProjectGeneration(observed_generation) => {
                entries.push(FactVersionRef::ProjectGeneration {
                    generation: *observed_generation,
                });
            }
            DepVersion::RouteGeneration(_) => {
                // Route generation has no real validating source —
                // refuse shared memo admission rather than rooting
                // the entry with a fact that cannot catch a content
                // edit to the observed canonical.
                return SignatureAdmission::NonCacheable(
                    NonAdmissionReason::RouteGenerationDependency,
                );
            }
        }
    }
    SignatureAdmission::Cacheable(
        verter_session_query::facts::fact_cache::ReadSetSignature::new(std::sync::Arc::from(
            entries,
        )),
    )
}

/// Merge the `dep_signature` entries from a `CacheRead` (or any
/// `&[(Arc<str>, DepVersion)]` slice) into a per-frame `local_fence`.
///
/// The `dep_signature_merges` / `dep_signature_intern_hits` audit
/// counters are NOT bumped here — they are owned by the shared dispatch
/// fact fan-in and the semantic signature interner.
/// This helper is now a pure fence merge for its remaining
/// non-dispatch-fan-in callers.
pub(crate) fn merge_dep_signature_into_local_fence(
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    incoming: &[(Arc<str>, crate::semantic_query::DepVersion)],
) {
    for entry in incoming {
        local_fence.push(entry.clone());
    }
}

#[cfg(test)]
#[path = "fact_signature_helpers_tests.rs"]
mod fact_signature_helpers_tests;
