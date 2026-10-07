//! The resumable memo protocol.
//!
//! A semantic query reaches the memo as a sequence of distinct operations,
//! none of which runs the query's producer:
//!
//! 1. **Lookup** ([`SemanticGraphStore::begin_query_claim`]): a validated
//!    warm read answers at once.
//! 2. **Claim** ([`SemanticGraphStore::claim_query`]): the claiming task
//!    either finds its own producer for the key open (same-path recursion),
//!    becomes the key's producer ([`ProducerLease`]), or subscribes to the
//!    producer another task owns ([`Subscription`]).
//! 3. **Subscribe** ([`Subscription::wait`]): a subscriber waits for the
//!    other task's completion — refusing a wait that would close a cycle of
//!    waiting tasks — and validates the delivered result for its own view,
//!    or retries when the producer went away.
//! 4. **Settle** ([`ProducerLease::settle`]): the producer's output, however
//!    and wherever it was computed, closes the producer.
//! 5. **Admit** ([`SettledProducer::admit`]): the retention decision — a
//!    complete, cacheable result enters the family memo; a refusal changes
//!    nothing about the completed value.
//! 6. **Complete** ([`SettledProducer::complete`]): the result is delivered
//!    to every subscriber and returned.
//!
//! A lease and a subscription are owned values that name their task and
//! flight, so the frame holding one may be suspended between operations.
//! The synchronous composition every current caller uses is
//! [`SemanticGraphStore::acquire_query`] (lookup, claim and blocking
//! subscription) followed by settle, admit and complete around the caller's
//! own build.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::instant::Instant;
use crate::semantic_query::demand::MaterializedSet;
use crate::semantic_query::{
    CacheRead, DepSignature, PartialReasonSet, QueryError, QueryResult, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue,
};

use super::inflight::{FlightCell, InflightPanicGuard, MAX_INFLIGHT_RETRIES};
use super::prepared::{self, PreparedKeyHandle};
use super::stats::InFlightStatsGuard;
use super::tasks::{
    ExecutionScope, ExecutionTask, OpenProducer, SemanticProducers, TaskId, WaitCycle,
};
use super::{
    cancelled_cache_read, empty_signature, record_inflight_aborted_retry,
    semantic_operand_evidence, PublishedMemoCandidate, SemanticGraphStore, WarmPublishOutcome,
};

type ValueRead = CacheRead<QueryResult<SemanticQueryValue>>;
type BuildOutput = crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue>;

/// Why a claim answers with the recursion carrier instead of a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recursion {
    /// The claiming task already has the key's producer open.
    SamePath,
    /// Waiting on the key's producer would close a cycle of waiting tasks.
    WaitCycle,
}

/// What a read captures besides its value: the publication its cold build
/// admitted, or the operand evidence of whichever result answers it.
#[derive(Default)]
pub struct ReadCapture<'a> {
    publication: Option<&'a mut Option<PublishedMemoCandidate>>,
    evidence: Option<&'a mut Option<crate::semantic_query::operand::SemanticOperandEvidence>>,
    carrier: Option<&'a mut Option<verter_session_query::facts::fact_cache::ReadSetSignature>>,
}

impl<'a> ReadCapture<'a> {
    /// Capture the candidate this read's cold build admits, if any.
    pub(crate) fn publication(slot: &'a mut Option<PublishedMemoCandidate>) -> Self {
        Self {
            publication: Some(slot),
            evidence: None,
            carrier: None,
        }
    }

    /// Capture the answering result's operand evidence. Gated on the
    /// forcing authority, so evidence leaves the store only on a force read.
    pub(crate) fn operand_evidence(
        slot: &'a mut Option<crate::semantic_query::operand::SemanticOperandEvidence>,
        _authority: &crate::project_semantic_dispatch::SemanticOperandAuthority,
    ) -> Self {
        Self {
            publication: None,
            evidence: Some(slot),
            carrier: None,
        }
    }

    /// Hand the answering result's carrier to `slot` instead of bubbling it
    /// into the tracers active now: a consumer that is not running when the
    /// result arrives replays the carrier into its own tracer when it
    /// resumes.
    pub(crate) fn deferring_carrier(
        slot: &'a mut Option<verter_session_query::facts::fact_cache::ReadSetSignature>,
    ) -> Self {
        Self {
            publication: None,
            evidence: None,
            carrier: Some(slot),
        }
    }

    /// The evidence and carrier slots together.
    pub(super) fn parts(
        &mut self,
    ) -> (
        Option<&mut Option<crate::semantic_query::operand::SemanticOperandEvidence>>,
        Option<&mut Option<verter_session_query::facts::fact_cache::ReadSetSignature>>,
    ) {
        (self.evidence.as_deref_mut(), self.carrier.as_deref_mut())
    }

    /// Deliver the answering result's carrier: into the deferred slot, or
    /// into the tracers active now.
    fn deliver_carrier(
        &mut self,
        carrier: &verter_session_query::facts::fact_cache::ReadSetSignature,
    ) {
        match self.carrier.as_deref_mut() {
            Some(slot) => *slot = Some(carrier.clone()),
            None => carrier.bubble_via_tls(),
        }
    }

    fn evidence(
        &mut self,
    ) -> Option<&mut Option<crate::semantic_query::operand::SemanticOperandEvidence>> {
        self.evidence.as_deref_mut()
    }
}

/// One logical claim of one key: the prepared key and what earlier attempts
/// of the same claim already counted.
pub struct ClaimAttempt {
    prepared: PreparedKeyHandle,
    /// Binding relations carry transaction-local inference candidates, so
    /// their producers never serve another claimant.
    independent: bool,
    miss_recorded: bool,
    retries: usize,
}

/// The outcome of one claim attempt.
pub enum Claim<'s> {
    /// A validated warm result, or the cancellation read.
    Read(ValueRead),
    /// The recursion carrier answers the claim.
    Recursive(Recursion),
    /// Another task produces the key.
    Subscribed(Subscription<'s>),
    /// The claiming task produces the key.
    Produce(ProducerLease<'s>),
}

/// The outcome of a synchronous acquisition: a claim whose subscriptions
/// were all waited out.
pub enum Acquired<'s> {
    Read(ValueRead),
    Recursive(Recursion),
    Produce(ProducerLease<'s>),
}

/// A subscription to the producer another task owns for a key.
pub struct Subscription<'s> {
    store: &'s SemanticGraphStore,
    inflight: Arc<FlightCell>,
    waiter: TaskId,
}

/// How a subscription resolved.
pub enum Joined {
    /// The producer's result, validated for the subscriber's view.
    Read(ValueRead),
    /// Waiting would close a cycle of waiting tasks.
    Recursive(Recursion),
    /// The producer went away (or its result does not hold for the
    /// subscriber's view): claim again.
    Retry,
}

/// The claiming task's producer for one key: the in-flight admission every
/// subscriber waits on, open until the producer settles.
pub struct ProducerLease<'s> {
    store: &'s SemanticGraphStore,
    prepared: PreparedKeyHandle,
    inflight: Arc<FlightCell>,
    independent: bool,
    panic_guard: InflightPanicGuard<'s>,
    open: OpenProducer,
    stats: InFlightStatsGuard<'s>,
    started: Instant,
}

/// A settled producer: its result, ready for the retention decision and the
/// delivery to subscribers.
pub struct SettledProducer<'s> {
    store: &'s SemanticGraphStore,
    prepared: PreparedKeyHandle,
    inflight: Arc<FlightCell>,
    independent: bool,
    result: QueryResult<SemanticQueryValue>,
    dep_signature: DepSignature,
    walker_diagnostics: Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
    carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
    self_root_canonicals: Arc<[Arc<str>]>,
    satisfied_projection: MaterializedSet,
    pending_prefix_backfills: Vec<crate::project_semantic_dispatch::walk::PrefixBackfill>,
    cache_suppress: bool,
    result_is_partial: bool,
    partial_reasons: PartialReasonSet,
    admissible: bool,
    admission_linearized: bool,
    _stats: InFlightStatsGuard<'s>,
}

impl SemanticGraphStore {
    /// The synchronous entry's task for this store: the task installed on
    /// this thread, or a fresh one for the extent of the returned scope.
    pub fn enter_execution(&self) -> ExecutionScope {
        ExecutionScope::enter(&self.task_registry)
    }

    /// Whether the task installed on this thread already has `key`'s
    /// producer open, so a claim would answer with the same-path carrier.
    /// A read-only preflight: the claim stays the sole authority that
    /// records and returns the carrier.
    pub fn is_same_path_claim(&self, key: &SemanticQueryKey) -> bool {
        ExecutionScope::current(&self.task_registry)
            .is_some_and(|task| task.produces_key(key, prepared::hash_key(key)))
    }

    /// The recursion carrier for `recursion`, with `sentinel` as its value.
    pub(crate) fn recursion_read(recursion: Recursion, sentinel: SemanticNodeId) -> ValueRead {
        CacheRead {
            value: QueryResult::Recursive(sentinel),
            dep_signature: empty_signature(),
            walker_diagnostics: Arc::from([]),
            // A cross-task wait-cycle carrier is an operational escape,
            // never a warm fact; a same-path carrier is a partial the warm
            // gates already refuse.
            cache_suppress: recursion == Recursion::WaitCycle,
            result_is_partial: true,
            partial_reasons: PartialReasonSet::SAME_PATH_RECURSION,
        }
    }

    /// Lookup, claim and wait: the synchronous acquisition of `key`. Never
    /// returns a subscription — it waits each one out.
    ///
    /// A claim needs the synchronous entry's task: `execution` is entered
    /// only once the warm lookup misses, so a warm read takes no task, and
    /// the caller keeps the scope for as long as the producer it may be
    /// handed runs, so every query nested in that build joins the task.
    pub fn acquire_query<'s, C: crate::resolver_core::ResolverCapabilities>(
        &'s self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        key: SemanticQueryKey,
        execution: &mut Option<ExecutionScope>,
        capture: &mut ReadCapture<'_>,
    ) -> Acquired<'s> {
        let mut attempt = match self.begin_query_claim(ctx, flags, key, capture) {
            Ok(attempt) => attempt,
            Err(read) => return Acquired::Read(read),
        };
        let task = execution
            .get_or_insert_with(|| self.enter_execution())
            .task();
        loop {
            match self.claim_query(ctx, flags, &mut attempt, task, capture) {
                Claim::Read(read) => return Acquired::Read(read),
                Claim::Recursive(recursion) => return Acquired::Recursive(recursion),
                Claim::Produce(lease) => return Acquired::Produce(lease),
                Claim::Subscribed(subscription) => {
                    match subscription.wait(ctx, flags, &mut attempt, capture) {
                        Joined::Read(read) => return Acquired::Read(read),
                        Joined::Recursive(recursion) => return Acquired::Recursive(recursion),
                        Joined::Retry => {}
                    }
                }
            }
        }
    }

    /// Begin one logical claim of `key`. `Err` answers it without a claim:
    /// a cancelled request, or a validated warm result.
    ///
    /// `flags` is the request snapshot's flag handles, borrowed once at the
    /// caller's request boundary: every cancellation observation below is a
    /// plain field read, with no per-checkpoint port dispatch.
    pub fn begin_query_claim<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        key: SemanticQueryKey,
        capture: &mut ReadCapture<'_>,
    ) -> Result<ClaimAttempt, ValueRead> {
        // Count every logical entry, warm and cold alike.
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_CALLS.fetch_add(1, Ordering::Relaxed);

        // Request cancellation is a typed ReturnOnly terminal and must be
        // observed before even a warm probe: canceled requests do not consume
        // shared semantic work or reuse a value as if they completed normally.
        if flags.is_cancelled() {
            return Err(cancelled_cache_read());
        }

        // Binding relations carry transaction-local inference candidates.
        // Their producers therefore keep the whole claim/settle/admit
        // protocol but never serve another claimant for the same key.
        let independent = matches!(
            &key,
            SemanticQueryKey::Relate {
                inference_context: Some(_),
                ..
            }
        );

        // Prepare the key ONCE per logical claim: one `family_and_slot`
        // projection, one requested-point build, one key hash — shared
        // (behind one `Arc`) by the warm probe, the in-flight table, the
        // producer registration, the panic guard and the publish. See the
        // `prepared` module for the equality contract (token equality ⟺
        // key equality).
        let prepared = PreparedKeyHandle::prepare(key);

        // Warm lookup. One lock acquisition snapshots the slot; the
        // candidate's carrier is validated strictly (self-roots through
        // `validates_self_root_whole_hash`) before it is bubbled or
        // returned, so a stale candidate misses and the claim below
        // recomputes it.
        if let Some(hit) = self.try_warm_value_hit_fast_path(ctx, &prepared, capture) {
            return Err(if flags.is_cancelled() {
                cancelled_cache_read()
            } else {
                hit
            });
        }

        // One logical miss per claim. A producer that publishes between
        // this lookup and the claim's own warm re-read is credited as a
        // hit there; that race is benign.
        crate::loop5_instrumentation::FAMILY_MEMO_MISSES.fetch_add(1, Ordering::Relaxed);
        #[cfg(any(test, feature = "test-support"))]
        crate::project_semantic_dispatch::raise::record_dispatch_cold(prepared.key());
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| {
            t.record_dispatch(prepared.key(), /* hit */ false)
        });
        tracing::debug!(
            target: "verter::memo::miss",
            key = ?prepared.key(),
            "memo_miss"
        );
        Ok(ClaimAttempt {
            prepared,
            independent,
            miss_recorded: false,
            retries: 0,
        })
    }

    /// One claim attempt of `attempt`'s key for `task`.
    pub fn claim_query<'s, C: crate::resolver_core::ResolverCapabilities>(
        &'s self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        attempt: &mut ClaimAttempt,
        task: &ExecutionTask,
        capture: &mut ReadCapture<'_>,
    ) -> Claim<'s> {
        if flags.is_cancelled() {
            return Claim::Read(cancelled_cache_read());
        }
        let prepared = &attempt.prepared;
        // 1. Warm re-read: reached on the rare race where another producer
        //    published after the lookup, or on a retry after an abort. The
        //    read validates strictly for this claimant's view.
        if let Some(hit) = self.get_validated_value_prepared(prepared, ctx, capture) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            if let Some(sched_ctx) = verter_execution::request_context::current_context() {
                sched_ctx
                    .0
                    .record_cache_event(verter_execution::request_context::CacheEventKind::Hit);
            }
            return Claim::Read(hit);
        }
        if !attempt.miss_recorded {
            // One miss per logical claim, however many attempts it takes.
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            if let Some(ctx) = verter_execution::request_context::current_context() {
                ctx.0
                    .record_cache_event(verter_execution::request_context::CacheEventKind::Miss);
            }
            attempt.miss_recorded = true;
        }

        // 2. Same-path recursion: the claiming task already produces this
        //    key. Its producer is still open, so answering it would wait on
        //    itself.
        if task.produces(prepared) {
            self.stats
                .same_path_sentinel_returns
                .fetch_add(1, Ordering::Relaxed);
            if let Some(ctx) = verter_execution::request_context::current_context() {
                ctx.0.record_cache_event(
                    verter_execution::request_context::CacheEventKind::Sentinel,
                );
            }
            return Claim::Recursive(Recursion::SamePath);
        }

        // 3. Become the producer, or subscribe to the one another task owns.
        if attempt.independent {
            let inflight = Arc::new(FlightCell::new());
            {
                let mut state = inflight.state.lock();
                state.claimed = true;
                state.owner = Some(task.id());
            }
            self.independent_inflight
                .lock()
                .entry(prepared.clone())
                .or_default()
                .push(Arc::clone(&inflight));
            return Claim::Produce(self.open_producer(prepared.clone(), inflight, true, task));
        }
        let inflight = {
            let mut table = self.inflight.lock();
            table
                .entry(prepared.clone())
                .or_insert_with(|| Arc::new(FlightCell::new()))
                .clone()
        };
        {
            let mut state = inflight.state.lock();
            if state.claimed {
                drop(state);
                return Claim::Subscribed(Subscription {
                    store: self,
                    inflight,
                    waiter: task.id(),
                });
            }
            state.claimed = true;
            state.owner = Some(task.id());
        }
        Claim::Produce(self.open_producer(prepared.clone(), inflight, false, task))
    }

    fn open_producer<'s>(
        &'s self,
        prepared: PreparedKeyHandle,
        inflight: Arc<FlightCell>,
        independent: bool,
        task: &ExecutionTask,
    ) -> ProducerLease<'s> {
        // Record the in-flight presence for peak tracking; the guard
        // decrements it on drop, panics included.
        self.stats.record_in_flight_enter();
        let stats = InFlightStatsGuard { stats: &self.stats };
        if let Some(prov) = self.provenance.as_ref() {
            prov.execute_cooperative_owner_path
                .fetch_add(1, Ordering::Relaxed);
        }
        // The producer registration and the in-flight admission are both
        // RAII guards sharing the prepared key, so a producer abandoned
        // before it settles (a panic, an early return) closes itself and
        // releases its subscribers.
        let open = task.open_producer(prepared.clone());
        let panic_guard = if independent {
            InflightPanicGuard::new_independent(
                Arc::clone(&inflight),
                &self.independent_inflight,
                prepared.clone(),
            )
        } else {
            InflightPanicGuard::new(Arc::clone(&inflight), &self.inflight, prepared.clone())
        };
        ProducerLease {
            store: self,
            prepared,
            inflight,
            independent,
            panic_guard,
            open,
            stats,
            started: Instant::now(),
        }
    }
}

impl Subscription<'_> {
    /// Wait for the producer's completion. Blocks the calling thread: only a
    /// synchronous entry, or a drive with nothing else to run, waits.
    pub fn wait<C: crate::resolver_core::ResolverCapabilities>(
        self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        attempt: &mut ClaimAttempt,
        capture: &mut ReadCapture<'_>,
    ) -> Joined {
        let Self {
            store,
            inflight,
            waiter,
        } = self;
        let prepared = &attempt.prepared;
        let mut state = inflight.state.lock();
        let wait_edge = if state.completed.is_none() && !state.aborted && !flags.is_cancelled() {
            let producer = state.owner.unwrap_or(waiter);
            match store.task_registry.register_wait(waiter, producer) {
                Ok(edge) => Some(edge),
                Err(WaitCycle) => return Joined::Recursive(Recursion::WaitCycle),
            }
        } else {
            None
        };
        // Cooperative wait on the flight's condvar until it completes, is
        // aborted by a canonical-invalidation sweep, or this request is
        // cancelled. Subscribers never busy-spin.
        let wait_start = Instant::now();
        // Test-only: record that this subscriber is about to SUSPEND on the
        // condvar (it holds `state` and is one statement from the wait,
        // which atomically releases `state` and parks).
        #[cfg(any(test, feature = "test-support"))]
        store.joiner_on_condvar_count.fetch_add(1, Ordering::SeqCst);
        while state.completed.is_none() && !state.aborted && !flags.is_cancelled() {
            // Timed parking is the cancellation observation rail. A canceled
            // subscriber detaches by returning; it never marks the shared
            // flight aborted, so it cannot disturb an uncancelled producer or
            // sibling.
            inflight
                .ready
                .wait_for(&mut state, std::time::Duration::from_millis(2));
        }
        drop(wait_edge);
        store
            .stats
            .waits_ms
            .fetch_add(wait_start.elapsed().as_millis() as u64, Ordering::Relaxed);
        // Every cooperative wait return counts; a retry may count again.
        store.stats.joined_waits.fetch_add(1, Ordering::Relaxed);
        if let Some(ctx) = verter_execution::request_context::current_context() {
            ctx.0
                .record_cache_event(verter_execution::request_context::CacheEventKind::JoinedWait);
        }
        if flags.is_cancelled() {
            drop(state);
            return Joined::Read(cancelled_cache_read());
        }
        if state.aborted && attempt.retries < MAX_INFLIGHT_RETRIES {
            // A concurrent canonical invalidation swept the (family, slot)
            // this flight served. Retire the exact aborted flight before
            // claiming again: the aborting producer may not have reached its
            // own retirement yet, and without this pointer-guarded removal a
            // fast subscriber could rejoin the same aborted flight until its
            // retry budget is gone.
            attempt.retries += 1;
            record_inflight_aborted_retry(&store.stats);
            drop(state);
            store.retire_inflight(prepared, &inflight, attempt.independent);
            return Joined::Retry;
        }
        let result = state.completed.clone().unwrap_or_else(|| {
            QueryResult::Error(QueryError::Other(Arc::from(
                "joiner woke without completion after retry budget exhausted",
            )))
        });
        let dep_signature = state.dep_signature.clone().unwrap_or_else(empty_signature);
        let graph_carrier = state.graph_carrier.clone();
        let producer_self_roots = Arc::clone(&state.self_root_canonicals);
        // The subscriber inherits the producer's non-cacheability and
        // partiality verbatim, so a subscriber inside an outer cold build
        // cannot let that build admit around a non-cacheable or partial
        // child.
        let cache_suppress = state.cache_suppress;
        let result_is_partial = state.result_is_partial;
        let partial_reasons = state.partial_reasons;
        let walker_diagnostics = state
            .walker_diagnostics
            .clone()
            .unwrap_or_else(|| Arc::from([]));
        // Release the flight lock before any tracer fan-out.
        drop(state);

        // View validation. A subscriber is not guaranteed to run under the
        // producer's view: two requests can carry the same key under
        // different overlays, and their results are not interchangeable. A
        // subscriber reuses the producer's result only if the producer's
        // carrier holds a view-discriminating self-root (a `FileWholeHash`
        // listed in its self-roots, validated strictly) that validates for
        // this subscriber's view, under the kernel-epoch gate. A carrier
        // without such a self-root validates vacuously for any view — a
        // suppressed empty carrier, an unrootable carrier, a view-specific
        // `Miss` — so it forks too; recomputing a genuinely view-invariant
        // result is the accepted cost. The fork retires the completed flight
        // (only while it is still the one this subscriber joined) so the
        // next claim installs a fresh flight this subscriber produces; the
        // warm re-read validated for this view misses the producer's entry
        // too, so the subscriber runs its own build.
        //
        // A recursion carrier is an operational escape, not a view-specific
        // value: it passes around a broken wait cycle without a fork.
        if !matches!(result, QueryResult::Recursive(_)) {
            if let Some(ref carrier) = graph_carrier {
                let view_validates = carrier.validate_with_self_roots(ctx, &producer_self_roots)
                    && !store.names_retired_kernel_epoch(&result);
                let lacks_view_discriminating_self_root =
                    !carrier.has_view_discriminating_self_root(&producer_self_roots);
                if !view_validates || lacks_view_discriminating_self_root {
                    store
                        .stats
                        .joiner_view_mismatch_forks
                        .fetch_add(1, Ordering::Relaxed);
                    let mut table = store.inflight.lock();
                    if table
                        .get(prepared)
                        .is_some_and(|entry| Arc::ptr_eq(entry, &inflight))
                    {
                        table.remove(prepared);
                    }
                    return Joined::Retry;
                }
            }
        }

        // Bubble the producer's path-precise fact rail into the subscriber's
        // active tracers, so an outer cold-compute scope sees every fact the
        // producer observed — the same rail a cold producer and a warm hit
        // deliver.
        if let Some(ref carrier) = graph_carrier {
            capture.deliver_carrier(carrier);
            if let Some(slot) = capture.evidence() {
                *slot = semantic_operand_evidence(carrier, &producer_self_roots, &dep_signature);
            }
        }
        if let Some(prov) = store.provenance.as_ref() {
            prov.execute_cooperative_joiner_path
                .fetch_add(1, Ordering::Relaxed);
        }
        Joined::Read(CacheRead {
            value: result,
            dep_signature,
            walker_diagnostics,
            cache_suppress,
            result_is_partial,
            partial_reasons,
        })
    }
}

impl<'s> ProducerLease<'s> {
    /// Close the producer with its build's output. `Err` is the cancellation
    /// read: a producer whose request was cancelled owns no publish right,
    /// so its flight is aborted and its value discarded.
    pub fn settle<C: crate::resolver_core::ResolverCapabilities>(
        self,
        _ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        output: BuildOutput,
    ) -> Result<SettledProducer<'s>, ValueRead> {
        let Self {
            store,
            prepared,
            inflight,
            independent,
            mut panic_guard,
            open,
            stats,
            started,
        } = self;
        let build_held_ns = started.elapsed().as_nanos() as u64;
        let BuildOutput {
            result,
            dep_signature,
            walker_diagnostics,
            cache_suppress,
            result_is_partial,
            partial_reasons,
            taint: _, // §18 taint already consumed upstream by `admit_decision`.
            observed_self_roots: _,
            graph_carrier,
            self_root_canonicals,
            pending_prefix_backfills,
            satisfied_projection,
            flow_completion,
        } = output;
        let result = prepared::enforce_projection_value_shape(prepared.key(), result);
        // §3.4 default: a non-path build (`Instantiate`, `KeyOf`, `TypeOf`,
        // …) records no path-walk hops, so its satisfied projection defaults
        // to the single terminal point the slot's mode denotes at the key's
        // path — exactly what a single-terminal compute produced. The point
        // comes from `requested_path_for_key`, which carries an authored
        // selective force's residual path, so a `Path(["wanted","leaf"])`
        // force records that path rather than the whole surface. A modeless
        // family yields `Demand::identity()`, a trivial pass.
        let satisfied_projection = if satisfied_projection.is_empty() {
            MaterializedSet::single(prepared.requested_point().clone())
        } else {
            satisfied_projection
        };
        let walker_diagnostics: Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]> =
            Arc::from(walker_diagnostics.into_boxed_slice());
        panic_guard.mark_finished();
        drop(panic_guard);
        drop(open);
        if let Some(prov) = store.provenance.as_ref() {
            prov.execute_cooperative_held_ns
                .fetch_add(build_held_ns, Ordering::Relaxed);
        }
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_COLD_BUILDS
            .fetch_add(1, Ordering::Relaxed);
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_BUILD_NS_TOTAL
            .fetch_add(build_held_ns, Ordering::Relaxed);

        if flags.is_cancelled() {
            store.abort_inflight_for_cancellation(&prepared, &inflight, independent);
            return Err(cancelled_cache_read());
        }

        // The carrier is always broadcast — bubbled into this producer's
        // outer tracers and recorded on the flight for subscribers — whether
        // or not the result is admitted.
        let carrier = match graph_carrier {
            Some(boxed) => *boxed,
            None => verter_session_query::facts::fact_cache::ReadSetSignature::new(
                crate::fact_signature_helpers::empty_fact_signature(),
            ),
        };
        // §2 admission invariant: a partial result is never admissible, so
        // it must already be non-cacheable (`finalise_traced_build_output`
        // enforces `result_is_partial ⟹ cache_suppress`).
        verter_debug_assert!(
            !result_is_partial || cache_suppress,
            "§1 invariant violated at memo admission: result_is_partial \
             without cache_suppress would launder a partial into the family memo"
        );
        // The flow-proof gate: a `FlowReturn` result is admissible ONLY with
        // the finalizer's proof token naming THIS key (which embeds the
        // result contract) and THIS value. The token is the sole positive
        // completeness authority; this gate may veto, never promote.
        let flow_proof_ok = match prepared.key() {
            SemanticQueryKey::FlowReturn(key) => match (&flow_completion, &result) {
                (Some(proof), QueryResult::Value(SemanticQueryValue::FlowReturn(value))) => {
                    proof.key() == key.as_ref() && proof.value() == value.as_ref()
                }
                _ => false,
            },
            _ => true,
        };
        // A vetoed value is UNPROVEN, not merely unpublished: force the
        // partial and suppression rails onto the read so the producer's
        // returned read — and every subscriber's — carries the taint. A
        // build that already reported partiality keeps its own classes.
        let (cache_suppress, result_is_partial, partial_reasons) =
            if !flow_proof_ok && !result_is_partial && matches!(result, QueryResult::Value(_)) {
                (
                    true,
                    true,
                    partial_reasons.union(PartialReasonSet::FLOW_RETURN_UNVERIFIED),
                )
            } else {
                (cache_suppress, result_is_partial, partial_reasons)
            };
        let admissible = !(cache_suppress || result_is_partial || !flow_proof_ok);
        Ok(SettledProducer {
            store,
            prepared,
            inflight,
            independent,
            result,
            dep_signature,
            walker_diagnostics,
            carrier,
            self_root_canonicals,
            satisfied_projection,
            pending_prefix_backfills,
            cache_suppress,
            result_is_partial,
            partial_reasons,
            admissible,
            admission_linearized: false,
            _stats: stats,
        })
    }
}

impl SettledProducer<'_> {
    /// The retention decision. An admissible result enters the family memo
    /// (with its prefix backfills); a non-cacheable or partial one does not,
    /// and neither does one the retention account refuses — none of which
    /// changes the completed value. `Err` is the cancellation read: a
    /// cancelled request whose admission did not linearize aborts the flight.
    pub fn admit<C: crate::resolver_core::ResolverCapabilities>(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        capture: &mut ReadCapture<'_>,
    ) -> Result<(), ValueRead> {
        let store = self.store;
        if flags.is_cancelled() {
            return Err(self.abort_for_cancellation());
        }
        let mut root_publication = None;
        if self.admissible {
            let outcome = store.warm_publish_one(
                ctx,
                flags,
                &self.prepared,
                &self.result,
                &self.walker_diagnostics,
                &self.carrier,
                &self.dep_signature,
                &self.self_root_canonicals,
                &self.satisfied_projection,
                &self.inflight,
            );
            let published = match outcome {
                WarmPublishOutcome::Published(candidate) => {
                    self.admission_linearized = true;
                    root_publication = Some(candidate);
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        let gate = store.cold_winner_post_admission_gate.lock().clone();
                        if let Some(barrier) = gate {
                            barrier.wait();
                            barrier.wait();
                        }
                    }
                    true
                }
                WarmPublishOutcome::Skipped => true,
                WarmPublishOutcome::Aborted => false,
            };
            if !self.admission_linearized && flags.is_cancelled() {
                if let Some(slot) = capture.publication.as_deref_mut() {
                    *slot = None;
                }
                return Err(self.abort_for_cancellation());
            }
            // Test-only injection point, parked after the parent entry
            // published and before the prefix backfills, so a race test can
            // abort this producer's still-registered flight in the exact
            // window the `published` gate alone does not cover.
            #[cfg(any(test, feature = "test-support"))]
            {
                let gate = store.cold_winner_pre_backfill_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
            // Prefix backfills publish after the parent entry is warm. An
            // aborted parent (a canonical invalidation or a generation reset
            // raced this build) interned against a stale id epoch, so its
            // backfills are stale too; each backfill re-checks the abort
            // under the entries lock as well, so a reset that lands during
            // this loop skips the rest.
            if published {
                for backfill in std::mem::take(&mut self.pending_prefix_backfills) {
                    self.admission_linearized |= store.warm_publish_one_if_absent(
                        ctx,
                        flags,
                        backfill.key,
                        QueryResult::Value(backfill.node),
                        self.carrier.clone(),
                        self.dep_signature.clone(),
                        Arc::clone(&self.self_root_canonicals),
                        backfill.satisfied_projection,
                        &self.inflight,
                        self.admission_linearized,
                    );
                }
            }
        } else {
            tracing::debug!(
                target: "verter::memo::suppress",
                key = ?self.prepared.key(),
                "cache_suppress=true; refusing memo insertion (build-output suppression)"
            );
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.memo_publish_suppressed.fetch_add(1, Ordering::Relaxed);
            }
        }
        if !self.admission_linearized && flags.is_cancelled() {
            if let Some(slot) = capture.publication.as_deref_mut() {
                *slot = None;
            }
            return Err(self.abort_for_cancellation());
        }
        if let (Some(slot), Some(candidate)) =
            (capture.publication.as_deref_mut(), root_publication)
        {
            *slot = Some(candidate);
        }
        Ok(())
    }

    fn abort_for_cancellation(&self) -> ValueRead {
        self.store.abort_inflight_for_cancellation(
            &self.prepared,
            &self.inflight,
            self.independent,
        );
        cancelled_cache_read()
    }

    /// Deliver the result: bubble its carrier into this producer's active
    /// tracers, complete the flight for every subscriber, retire the flight
    /// and return the read.
    pub fn complete<C: crate::resolver_core::ResolverCapabilities>(
        self,
        _ctx: &dyn crate::resolver_core::ResolverContext<C>,
        capture: &mut ReadCapture<'_>,
    ) -> ValueRead {
        let Self {
            store,
            prepared,
            inflight,
            independent,
            result,
            dep_signature,
            walker_diagnostics,
            carrier,
            self_root_canonicals,
            cache_suppress,
            result_is_partial,
            partial_reasons,
            ..
        } = self;
        if let Some(slot) = capture.evidence() {
            *slot = semantic_operand_evidence(&carrier, &self_root_canonicals, &dep_signature);
        }
        // Bubble the carrier into this producer's still-active outer
        // tracers — cacheable or not. The build installed its own tracer and
        // popped it before the carrier existed, so the synthesised self-root
        // `FileWholeHash` facts live only on the carrier; bubbling here makes
        // a cold-built child, a warm-hit child and a joined child deliver the
        // identical fact set to their parent.
        capture.deliver_carrier(&carrier);

        // Complete the flight and wake its subscribers. An abort planted by
        // an invalidation stays: subscribers that wake on it retry rather
        // than read the stale result.
        {
            let mut state = inflight.state.lock();
            if !state.aborted {
                state.completed = Some(result.clone());
                state.dep_signature = Some(dep_signature.clone());
                // Subscribers bubble the same carrier this producer bubbled
                // and validate it for their own views against these
                // self-roots; they inherit its non-cacheability and
                // partiality so no enclosing build admits around them.
                state.graph_carrier = Some(Box::new(carrier));
                state.self_root_canonicals = Arc::clone(&self_root_canonicals);
                state.cache_suppress = cache_suppress;
                state.result_is_partial = result_is_partial;
                state.partial_reasons = partial_reasons;
                state.walker_diagnostics = Some(Arc::clone(&walker_diagnostics));
            }
        }
        inflight.ready.notify_all();

        // Retire the flight whatever the admission decided: a flight left
        // behind would let a later claim — after an invalidation evicted the
        // entry — latch onto its stale completion. The removal is
        // pointer-guarded, so a fresh flight a forking subscriber installed
        // for the same key is never evicted.
        store.retire_inflight(&prepared, &inflight, independent);

        CacheRead {
            value: result,
            dep_signature,
            walker_diagnostics,
            cache_suppress,
            result_is_partial,
            partial_reasons,
        }
    }
}

/// Test composition of the protocol around a synchronous build closure, the
/// shape the memo's own tests drive it through.
#[cfg(any(test, feature = "test-support"))]
impl SemanticGraphStore {
    #[must_use = "the CacheRead carries both the resolved node id and the dep signature callers must fold into their dependency-fact set"]
    #[allow(dead_code)] // test-support can be enabled without the in-crate memo tests
    pub fn execute_cooperative<F, R, O, C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> CacheRead<QueryResult<SemanticNodeId>>
    where
        F: FnOnce() -> O,
        O: Into<crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticNodeId>>,
        R: FnOnce() -> SemanticNodeId,
    {
        super::narrow_cache_read(self.execute_cooperative_value(
            ctx,
            key,
            recursion_sentinel,
            || {
                let output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                    SemanticNodeId,
                > = build().into();
                BuildOutput::from(output)
            },
        ))
    }

    #[allow(dead_code)] // test-support can be enabled without the in-crate memo tests
    pub fn execute_cooperative_value<F, R, O, C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> ValueRead
    where
        F: FnOnce() -> O,
        O: Into<BuildOutput>,
        R: FnOnce() -> SemanticNodeId,
    {
        self.execute_cooperative_captured(
            ctx,
            key,
            recursion_sentinel,
            build,
            &mut ReadCapture::default(),
        )
    }

    #[allow(dead_code)] // test-support can be enabled without the in-crate memo tests
    pub fn execute_cooperative_value_capturing_publication<
        F,
        R,
        O,
        C: crate::resolver_core::ResolverCapabilities,
    >(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
        publication: &mut Option<PublishedMemoCandidate>,
    ) -> ValueRead
    where
        F: FnOnce() -> O,
        O: Into<BuildOutput>,
        R: FnOnce() -> SemanticNodeId,
    {
        self.execute_cooperative_captured(
            ctx,
            key,
            recursion_sentinel,
            build,
            &mut ReadCapture::publication(publication),
        )
    }

    fn execute_cooperative_captured<F, R, O, C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
        capture: &mut ReadCapture<'_>,
    ) -> ValueRead
    where
        F: FnOnce() -> O,
        O: Into<BuildOutput>,
        R: FnOnce() -> SemanticNodeId,
    {
        // Test-support entry: capture the request snapshot's flag handles
        // once here, so the claim/settle/admit protocol below reads plain
        // fields exactly as the production dispatch path does.
        let flags = crate::resolver_core::fact_validation_port::FactValidation::request_flags(ctx);
        let mut execution = None;
        match self.acquire_query(ctx, flags, key, &mut execution, capture) {
            Acquired::Read(read) => read,
            Acquired::Recursive(recursion) => Self::recursion_read(recursion, recursion_sentinel()),
            Acquired::Produce(lease) => match lease.settle(ctx, flags, build().into()) {
                Err(read) => read,
                Ok(mut settled) => match settled.admit(ctx, flags, capture) {
                    Err(read) => read,
                    Ok(()) => settled.complete(ctx, capture),
                },
            },
        }
    }
}
