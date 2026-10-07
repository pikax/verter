//! Request-bound adapter implementations for `ResolverContext`.
//!
//! The method-free marker composes five dyn-compatible services: indexed
//! inputs, owned lowering, routing, fact validation and execution submission.
//! The reads a request makes once per semantic node or more — cancellation,
//! the project generation and the live aggregate clocks — are not port
//! methods: they are handles on the engine-owned [`RequestSnapshot`] the host
//! captures once when it admits the request. Their answers are owned records, typed source demands or an
//! opaque engine attachment; no ambient host/store/config getter is exposed.
//! Two capability refinements — expression-source selection over owned
//! lowering, live clock sampling over fact validation — hand out concrete
//! capability types named by the context's [`ResolverCapabilities`] family,
//! so the operations made on a selected capability dispatch statically.
//!
//! Private lifecycle adapters select the captured request view and completion
//! overlay. The query facade owns execution and output capabilities, and nested
//! semantic demands reuse that facade. The concrete host remains confined to
//! these private backend owners.
//!
//! The adapter markers in [`sealed`] are `pub`, because the host implements
//! them from its own crate and Rust has no friend visibility. Any crate can
//! name and implement them, so they state a workspace contract rather than
//! enforce a visibility seal. The request-bound guarantee is held by
//! construction instead: production requests use `HostResolverContext` or
//! `SessionResolverContext`, which only the host's request entry boundary
//! builds, and direct-host support remains test-only.

use std::sync::Arc;

use super::fact_validation_port::{FactValidation, LiveFactValidation};
use super::request_ports::{
    CancellationCheckpoint, ExecutionSubmission, ExpressionSourceSelection, HostAttachmentPort,
    IndexedInputs, OwnedLowering, RouteLookup,
};

use crate::fact_tracing::tracing as tracer_stack;

use verter_session_query::analysis::types::Hash16;

/// The adapter markers a host implements beside `ResolverContext` (and its
/// request-bound refinement). They are part of the engine's public adapter
/// contract: the host crate implements them for its request-bound contexts.
/// What stays sealed is the host's request-bound lifecycle construction, not
/// this contract.
#[doc(hidden)]
pub mod sealed {
    /// Marker every `ResolverContext` implementation carries.
    pub trait Sealed {}

    /// Narrower marker carried by [`super::RequestBoundResolverContext`]
    /// implementations.
    ///
    /// The host's production implementations are its two genuinely
    /// request-bound contexts. Its test-support direct-host seam receives the
    /// marker only in configurations where its entire `ResolverContext`
    /// implementation is compile-visible.
    pub trait RequestBoundSealed {}
}

/// A single, tear-free observation of a materialize-memo scope's
/// content identity.
///
/// The materialize-memo publish site
/// (`meta_resolve/materialize/field_types.rs`) needs the scope's
/// content version for two distinct consumers that MUST agree:
///
/// 1. the `NodeScopeId::File { whole_hash }` the projector lowers the
///    `TypeExpr` against — the lowered value's semantic identity;
/// 2. the `ShapeCacheDb` entry's fact-signature self-root — the
///    view-correct shared-cache admission gate.
///
/// Sourcing those from two separate oracles (`shallow_file_state` for
/// the scope id, `authoritative_current_content_hash` for the
/// signature) can tear: an edit landing between the two reads roots a
/// value lowered under `H1` on a signature self-rooted at `H2`. This
/// type closes the tear: the publish site takes ONE
/// `MaterializeScopeObservation` and feeds [`Self::whole_hash`] to
/// BOTH consumers, plus the pinned [`Self::syntactic_export_set`] to
/// the signature builder. Both come from the same
/// `Arc<IndexedReady>` — internally consistent by construction
/// (`FileArtifactStore` is content-addressed; `indexed.whole_hash ==
/// indexed.shallow_state.whole_hash`).
#[derive(Clone)]
pub struct MaterializeScopeObservation {
    /// The scope canonical this observation describes.
    pub canonical_id: Arc<str>,
    /// The content observation whose `whole_hash` roots both
    /// the lowering `NodeScopeId` and the signature self-root.
    pub observed_whole_hash: Hash16,
    pub observed_shallow_hash: Hash16,
    /// The scope's `SyntacticExportSet` parse fact, pinned to
    /// the observed content hash via
    /// [`crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content`].
    /// `None` when the observed version's parse-fact registry is not
    /// recoverable — the publish site then refuses shared-cache
    /// admission while still returning the freshly-computed value.
    pub syntactic_export_set: Option<verter_session_query::facts::fact_cache::ParseFactRef>,
}

impl MaterializeScopeObservation {
    /// The observed scope content version. Feeds both the lowering
    /// `NodeScopeId::File { whole_hash }` and the signature self-root —
    /// a single source, so the two cannot disagree.
    #[inline]
    pub fn whole_hash(&self) -> verter_session_query::facts::store_view::ResolverHash16 {
        self.observed_whole_hash
    }
}

/// A request's project-generation clock: a handle on the counter the host
/// advances when configuration, environment, project identity or workspace
/// authority is reset. Every [`Self::current`] reads the live counter, so a
/// reset that lands mid-request is visible to the next read.
#[derive(Clone)]
pub struct ProjectGenerationClock(Arc<std::sync::atomic::AtomicU64>);

impl ProjectGenerationClock {
    /// Read `clock`, which its owner keeps advancing.
    #[must_use]
    pub fn new(clock: Arc<std::sync::atomic::AtomicU64>) -> Self {
        Self(clock)
    }

    /// The current project generation.
    #[inline]
    #[must_use]
    pub fn current(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The cancellation and project-generation handles of one request.
///
/// Both are handles, never sampled values: capturing them is constant-cost,
/// and every read observes the live state.
#[derive(Clone)]
pub struct RequestFlags {
    cancellation: CancellationCheckpoint,
    project_generation: ProjectGenerationClock,
}

impl RequestFlags {
    #[must_use]
    pub fn new(project_generation: ProjectGenerationClock) -> Self {
        Self {
            cancellation: CancellationCheckpoint::new(),
            project_generation,
        }
    }

    /// The request's cancellation checkpoint.
    #[inline]
    #[must_use]
    pub fn cancellation_checkpoint(&self) -> CancellationCheckpoint {
        self.cancellation
    }

    /// Whether the request is cancelled now.
    #[cfg_attr(feature = "test-support", track_caller)]
    #[inline]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// The current project generation.
    #[inline]
    #[must_use]
    pub fn current_project_generation(&self) -> u64 {
        self.project_generation.current()
    }
}

/// What the host captures once when it admits a request, for the engine's
/// hot paths to read as plain fields: the request's [`RequestFlags`] and its
/// live aggregate clock reader.
///
/// It holds handles, tokens and flags only — never workspace or store state —
/// so capturing it is constant-cost, and no read through it is stale: a
/// cancellation, a project reset or a workspace edit that lands mid-request
/// is observed by the next read.
#[derive(Clone)]
pub struct RequestSnapshot<W> {
    flags: RequestFlags,
    clocks: verter_session_query::facts::clocks::AggregateClockReader<W>,
}

impl<W> RequestSnapshot<W> {
    #[must_use]
    pub fn new(
        project_generation: ProjectGenerationClock,
        clocks: verter_session_query::facts::clocks::AggregateClockReader<W>,
    ) -> Self {
        Self {
            flags: RequestFlags::new(project_generation),
            clocks,
        }
    }

    /// The request's cancellation and project-generation handles.
    #[inline]
    #[must_use]
    pub fn flags(&self) -> &RequestFlags {
        &self.flags
    }

    /// The request's live aggregate clock reader.
    #[inline]
    #[must_use]
    pub fn clocks(&self) -> &verter_session_query::facts::clocks::AggregateClockReader<W> {
        &self.clocks
    }

    /// Whether the request is cancelled now.
    #[cfg_attr(feature = "test-support", track_caller)]
    #[inline]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flags.is_cancelled()
    }

    /// The current project generation.
    #[inline]
    #[must_use]
    pub fn current_project_generation(&self) -> u64 {
        self.flags.current_project_generation()
    }
}

/// Restricted host facade for resolver-tier code (`resolver_core/*`,
/// `meta_resolve/*` post-moves, `component_meta_caches.rs`,
/// `project_semantic_dispatch/*`).
///
/// `ResolverContext` composes five request ports, their two capability
/// refinements and the adapter markers in [`sealed`]. All service traits are
/// dyn-compatible and expose no ambient host access.
///
/// The trait is `pub` because the host implements it from its own crate. It
/// is not an integration point: the only production implementers are the
/// host's request-bound contexts.
///
/// `C` names the concrete capability types the request ports hand out, so a
/// selected expression source or a live clock sample dispatches statically
/// after the one request-port call that selected it.
pub trait ResolverContext<C: ResolverCapabilities>:
    sealed::Sealed
    + sealed::RequestBoundSealed
    + IndexedInputs
    + OwnedLowering
    + ExpressionSourceSelection<ExpressionDemand = C::ExpressionDemand>
    + RouteLookup
    + FactValidation
    + LiveFactValidation<Clocks = C::Clocks>
    + ExecutionSubmission<MacroMirrors = C::MacroMirrors>
    + HostAttachmentPort<HostAttachment = C::HostAttachment>
{
}

/// The concrete capability types one family of request contexts hands out.
///
/// The host chooses them once; every request context it builds — base or
/// overlay — shares them, so a request structure carrying `C` is
/// instantiated once per host family, not once per context kind.
pub trait ResolverCapabilities: 'static {
    /// The owned expression-source demand handle a selection returns.
    type ExpressionDemand: verter_session_query::source::demand::ExpressionSourceDemand
        + Clone
        + 'static;
    /// The workspace clock source a live aggregate sample reads.
    type Clocks: verter_session_query::facts::clocks::WorkspaceClocks + Clone + 'static;
    /// The composition-owned attachment the host hands out beside the engine.
    /// Opaque to the engine.
    type HostAttachment: ?Sized + 'static;
    /// The request-retained macro hot-mirror selector an engine binding holds.
    type MacroMirrors: crate::resolver_core::request_ports::MacroMirrorSource + 'static;
}

// Sealed marker — `VerterHost` is the base implementer,
// `HostResolverContext` is the request-bound wrapper that carries a
// borrowed `HostStoreView`, and `SessionResolverContext` is the
// overlay-aware wrapper that delegates every method to a borrowed host
// alongside an overlay-rooted view.
/// Marker subtrait: a [`ResolverContext`] that is genuinely
/// REQUEST-BOUND — it carries a per-request [`HostStoreView`] (and, for a
/// session query, an overlay) constructed at the request entry boundary,
/// so [`ResolverContext::is_request_bound`] is `true` and every artifact
/// serve is view-correct for the requesting caller.
///
/// Every [`ResolverContext`] requires the request-bound marker, and the query
/// host port retains this narrower trait to state its request-bound API
/// contract directly.
///
/// Marked via [`sealed::RequestBoundSealed`]. That marker is `pub`, so it
/// states the contract rather than enforcing it: a new implementation is a
/// visible `impl` that review must catch, not a compile error. The host's
/// direct `VerterHost` implementation exists only behind the compile-absent
/// production test-support fence.
///
/// The marker deliberately does NOT distinguish a base
/// [`crate::resolver_core::HostResolverContext`] from an overlay
/// [`crate::resolver_core::SessionResolverContext`] — both are
/// request-bound. Overlay-vs-base correctness stays the caller's
/// obligation (chosen at the request entry) and its regression coverage is
/// tracked separately.
pub trait RequestBoundResolverContext<C: ResolverCapabilities>:
    ResolverContext<C> + sealed::RequestBoundSealed
{
}

// Production request contexts carry the seal. The direct host receives it
// only in the explicitly test-only configuration below.
// Compile-time dyn-compatibility check. If a future trait edit
// accidentally introduces an unbound associated type, a generic method, or a
// `where Self: Sized` bound that breaks dyn-compatibility, naming the trait
// object in this signature fails inside this file at compile time long
// before a callsite-cascade error.
#[allow(dead_code)]
fn assert_resolver_context_obj_safe<C: ResolverCapabilities>(_: &dyn ResolverContext<C>) {}
// The request-bound refinement is used as `&dyn RequestBoundResolverContext`
// by the query host port, so it must stay dyn-compatible too. A marker
// subtrait of a dyn-compatible trait adding no new methods is dyn-safe;
// this pins it against a future edit.
#[allow(dead_code)]
fn assert_request_bound_resolver_context_obj_safe<C: ResolverCapabilities>(
    _: &dyn RequestBoundResolverContext<C>,
) {
}
/// Fan `fact` into every active tracer on the current thread's stack.
///
/// Used by the rewritten `compile_fact_emission` and any other producer
/// that must deliver a single observation to all nested tracer scopes.
/// No-op when the stack is empty.
#[inline]
pub fn observe_fan_out(fact: verter_session_query::facts::fact_cache::FactVersionRef) {
    tracer_stack::observe_fan_out(fact);
}

// ---------------------------------------------------------------------------
// Request-bound lifecycle adapters — the ONE shared `ResolverContext`
// implementation behind `HostResolverContext` and `SessionResolverContext`.
// ---------------------------------------------------------------------------

/// Borrowed-slice variant of [`observe_fan_out`]. Used by
/// `bubble_fact_signature_via_tls` and other warm-hit bubble-up paths.
/// No-op when the stack is empty or `sig` is empty.
#[inline]
pub fn observe_fan_out_borrowed(sig: &[verter_session_query::facts::fact_cache::FactVersionRef]) {
    tracer_stack::observe_fan_out_borrowed(sig);
}

/// Whether any fact tracer is installed on the current thread's stack.
///
/// A cheap early-out for observation producers whose FACT DERIVATION has a
/// non-trivial cost (a predicate check, a hash derivation, an owned
/// canonical clone): with no active tracer the observation would be a
/// no-op, so the producer skips the derivation entirely.
#[inline]
pub fn fact_tracer_installed() -> bool {
    tracer_stack::current_tracer().is_some()
}

/// The fact reads a computation made, for replay into scopes that were
/// not live while it ran.
#[derive(Debug, Clone)]
pub struct RecordedFactReads {
    /// Every distinct fact fanned out while the computation ran: its own
    /// reads, and the receipt of each completed result it consumed.
    pub facts: std::sync::Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    /// Whether any non-cacheable read was marked, local-only included.
    pub non_cacheable: bool,
}

/// Where the active scopes stood when a computation began, so its
/// observations can be replaced by its receipt if it completes
/// ([`complete_with_receipt`]).
pub(crate) use tracer_stack::EvidenceMarks;

/// Mark every active tracer and recorder before a computation whose
/// completed result may be answered by a receipt.
#[inline]
pub(crate) fn mark_evidence() -> EvidenceMarks {
    tracer_stack::mark_evidence()
}

/// A computation that ran between `start` and `end` completed with the
/// evidence `reads`: mint its receipt and replace, in every scope still
/// active, what it observed in that range with the receipt. Returns the
/// receipt, the one fact a later consumer of the result observes instead of
/// its reads.
pub(crate) fn complete_with_receipt(
    start: &EvidenceMarks,
    end: &EvidenceMarks,
    reads: &RecordedFactReads,
) -> verter_session_query::facts::fact_cache::FactVersionRef {
    let receipt = verter_session_query::facts::fact_cache::FactVersionRef::Receipt(
        verter_session_query::facts::fact_cache::ResultReceipt::new(reads.facts.to_vec()),
    );
    tracer_stack::collapse_evidence(start, end, &receipt);
    receipt
}

/// Run `work` under a passive fact-read recorder and return what it read.
///
/// Recording changes nothing the installed tracers observe (see
/// [`tracer_stack::FactReadRecorder`]). A producer skips deriving an
/// observation when no tracer is installed, so a recording is complete only
/// if a tracer was installed throughout — the caller checks
/// [`fact_tracer_installed`] first.
pub(crate) fn record_fact_reads<R>(work: impl FnOnce() -> R) -> (R, RecordedFactReads) {
    let recorder = tracer_stack::FactReadRecorder::default();
    let result = {
        let _scope = tracer_stack::install_recorder(&recorder);
        work()
    };
    let (facts, non_cacheable) = recorder.into_parts();
    (
        result,
        RecordedFactReads {
            facts: facts.into(),
            non_cacheable,
        },
    )
}

// ── `with_fact_tracer` installer ──────────────────────────────────────
//
// One cold compute on one thread holds a `FactReadSetCell` for its
// lifetime. The installer plants the cell into a TLS slot and the
// trait method [`ResolverContext::current_fact_tracer`] reads it.
//
// **Why this is NOT an R18 violation.** R18 forbids hidden global
// view state — views must be passed explicitly so concurrent
// sessions don't see each other's overlays. The fact tracer is a
// different substrate: it is per-compute, per-thread instrumentation
// that NEVER stores host state and NEVER influences resolver
// semantics. The TLS slot is a back-end for the
// [`crate::VerterHost::with_fact_tracer`] RAII scope and is reachable
// only through the documented trait method. The contract is:
//   1. Each installer brackets one traced scope on one thread.
//   2. Nesting IS supported: the active tracers form a per-thread STACK
//      (`ACTIVE_TRACERS`), and every observation / non-cacheability mark
//      fans out to ALL active levels, so an inner scope's observations are
//      also seen by every enclosing scope. An inner `with_fact_tracer`
//      pushes a second cell and pops it on drop (RAII, including on
//      unwind). This is what lets the evaluator-scoped carrier observer
//      nest inside a cold build's tracer.
//   3. Readers must go through `ResolverContext::current_fact_tracer`,
//      never through the TLS slot directly. The slot is private to
//      `crate::fact_tracing::tracing`.
//
// The trait-method discipline is the architectural contract. The TLS
// implementation is hidden inside `crate::fact_tracing::tracing` and is not
// part of any public surface.

/// RAII guard that clears the TLS tracer slot on drop.
///
/// Internal to the `with_fact_tracer` machinery. Returned by
/// [`install_tracer`] so the caller's `with_fact_tracer` closure
/// can hold the guard for the closure's duration.
struct TracerScope;

impl Drop for TracerScope {
    fn drop(&mut self) {
        tracer_stack::clear();
    }
}

/// A fact tracer whose cell a suspendable compute owns: it is installed on
/// the thread only while one of the compute's steps runs
/// ([`Self::install`]), and yields its read set once, when the compute
/// completes.
pub(crate) struct OwnedFactTracer {
    cell: Box<verter_session_query::facts::fact_read_set::FactReadSetCell>,
}

impl OwnedFactTracer {
    pub(crate) fn new(
        basis: verter_session_query::facts::fact_cache::AggregateGenerations,
    ) -> Self {
        let cell = Box::new(verter_session_query::facts::fact_read_set::FactReadSetCell::new());
        cell.set_aggregate_basis(basis);
        Self { cell }
    }

    /// Install the cell until the returned scope drops, unwinding included.
    pub(crate) fn install(&self) -> OwnedTracerScope<'_> {
        tracer_stack::install(&self.cell);
        OwnedTracerScope {
            _tracer: std::marker::PhantomData,
        }
    }

    pub(crate) fn into_read_set(self) -> verter_session_query::facts::fact_read_set::FactReadSet {
        (*self.cell).into_inner()
    }
}

/// One installation of an [`OwnedFactTracer`]; dropping it uninstalls
/// the cell, which it borrows so the cell outlives the installation.
pub(crate) struct OwnedTracerScope<'t> {
    _tracer: std::marker::PhantomData<&'t OwnedFactTracer>,
}

impl Drop for OwnedTracerScope<'_> {
    fn drop(&mut self) {
        tracer_stack::clear();
    }
}

/// Install a request-owned basis without granting host access to the compute.
pub fn with_fact_tracer_cell<F, R>(
    basis: verter_session_query::facts::fact_cache::AggregateGenerations,
    f: F,
) -> (R, verter_session_query::facts::fact_read_set::FactReadSet)
where
    F: FnOnce(&verter_session_query::facts::fact_read_set::FactReadSetCell) -> R,
{
    let cell = verter_session_query::facts::fact_read_set::FactReadSetCell::new();
    cell.set_aggregate_basis(basis);
    // Push onto the tracer stack. The RAII guard pops on drop
    // (including on panic unwind) so no dangling pointer remains.
    tracer_stack::install(&cell);
    let scope = TracerScope;
    let result = f(&cell);
    // Explicit drop so the stack is popped before we consume
    // `cell.into_inner()`. After this point no `&FactReadSetCell`
    // can leak out of TLS.
    drop(scope);
    (result, cell.into_inner())
}

#[cfg(test)]
mod request_snapshot_tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use verter_session_query::facts::clocks::{
        AggregateClockReader, BracketedGenerationRead, ProjectGenerationRead, WorkspaceClocks,
    };

    use super::{ProjectGenerationClock, RequestSnapshot};

    #[derive(Clone)]
    struct Content(Arc<AtomicU64>);
    impl WorkspaceClocks for Content {
        fn content_generation(&self) -> u64 {
            self.0.load(Ordering::Acquire)
        }
        fn source_env_generation(&self) -> Option<u64> {
            None
        }
    }

    /// A snapshot is captured once per request, so it must hold handles, not
    /// sampled values: a project reset or a workspace edit that lands after
    /// the capture has to be visible to the request's next read, or a
    /// mid-request change would go undetected at publish.
    #[test]
    fn reads_through_a_captured_snapshot_observe_later_changes() {
        let project = Arc::new(AtomicU64::new(3));
        let content = Arc::new(AtomicU64::new(10));
        let imports = Arc::new(AtomicU64::new(0));
        let routes = Arc::new(AtomicU64::new(0));
        let snapshot = RequestSnapshot::new(
            ProjectGenerationClock::new(Arc::clone(&project)),
            AggregateClockReader::new(
                Content(Arc::clone(&content)),
                ProjectGenerationRead::new(Arc::clone(&project)),
                BracketedGenerationRead::new(imports),
                BracketedGenerationRead::new(routes),
            ),
        );
        assert_eq!(snapshot.current_project_generation(), 3);
        assert_eq!(snapshot.clocks().live().content, 10);

        project.fetch_add(1, Ordering::AcqRel);
        content.fetch_add(5, Ordering::AcqRel);

        assert_eq!(snapshot.current_project_generation(), 4);
        assert_eq!(snapshot.flags().current_project_generation(), 4);
        let live = snapshot.clocks().live();
        assert_eq!(live.content, 15);
        assert_eq!(live.workspace_shape, 4);
    }
}
