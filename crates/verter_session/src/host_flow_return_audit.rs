#![deny(missing_docs)]
//! `VerterHost::get_flow_return_type_with_audit` — the single public
//! audited entry-point for whole-function flow-return inference.
//!
//! Wires the standard audit lifecycle (registration constructed BEFORE
//! the TLS observer install, producer body run under the matching
//! guard, per-request counters snapshotted at finalize) around one
//! `SemanticQueryKey::FlowReturn` demand routed through the shared
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`] —
//! never a second resolver.
//!
//! Outcome mapping (the locked design's split result/carrier contract,
//! C1): a COMPLETE evaluation — including a DEGRADED SUCCESS carrying
//! `FlowReturnResult::degradation` — rides the [`AuditedResult`] `Ok`
//! arm; a genuine NO-VALUE outcome (the typed
//! [`crate::semantic_query::FlowReturnFailure`] class) rides the `Err`
//! arm as [`FlowReturnError::Failure`]. Both arms carry the audit
//! record.
//!
//! Cold-vs-warm audit contract: a warm family hit emits NO
//! `FlowReturnStarted` structured event and its record reports
//! `from_cache = true` with `cold_computes == 0`; the cold-path
//! emission helpers construct no event payload without an installed
//! accumulator (see [`crate::flow_return_audit`]).
//!
//! Partiality projection: `observed_partiality` reduces the outcome's
//! OWN typed reason — the [`FlowReturnResult::degradation`] the
//! finalizer refused to seal, or the [`FlowReturnError`] on the `Err`
//! arm — onto the payload's `partiality` field, so a caller reading the
//! record sees WHY a request was partial, not only THAT it was. This is
//! the SOLE projection of that vocabulary onto the wire surface, and it
//! is read-only: the outcome is already bound when it runs, and nothing
//! in admission, warm/cold classification, or cache identity reads it
//! back.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::{
    AuditedResult, FlowDegradationTag, FlowFailureTag, FlowPartialityTag,
    FlowReturnInferencePayload, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, FlowReturnFailure, FlowReturnResult, FlowReturnUnsupported,
    ResolveCallFailure, ReturnProjectionDemand,
};
use crate::VerterHost;

/// Typed `Err` arm of the flow-return [`AuditedResult`] carrier —
/// genuine NO-VALUE outcomes only. A degraded-but-usable result is NOT
/// an error: it rides the `Ok` arm as a [`FlowReturnResult`] with
/// `degradation: Some(_)` (the split result/carrier contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowReturnError {
    /// The evaluation produced no value — the typed
    /// [`FlowReturnFailure`] class (missing function, unsupported
    /// control surface, torn state, empty recursive cycle, unmodeled
    /// demand point, budget exhaustion).
    Failure(FlowReturnFailure),
    /// The host could not pin a proven-current store view within the
    /// bounded retry window; the query was not resolved against
    /// superseded state.
    UnstableState {
        /// Number of retry attempts made before giving up.
        attempts: u8,
    },
}

impl VerterHost {
    /// Resolve one whole-function flow return through the shared
    /// dispatch and return the result — or a typed
    /// [`FlowReturnError`] — alongside the per-request
    /// [`RequestAuditRecord`], packaged in one [`AuditedResult`].
    ///
    /// `function` is the content-free served-function identity (the
    /// declaration anchor plus part/overload ordinal); `demand` is the
    /// return-projection point. Production's canonical point is
    /// [`ReturnProjectionDemand::whole_return`]; any narrower point is
    /// accepted as key data and currently fails CLOSED with the typed
    /// `UnmodeledDemandPoint` failure (never a silently widened
    /// whole-return result).
    ///
    /// The carrier's `audit` field is always populated: an active
    /// registration carries the full `FlowReturnInference` payload
    /// ([`verter_audit::AuditCaptureState::ActiveStored`]); a filtered
    /// or disabled registration carries the cheap default-filled
    /// record ([`verter_audit::AuditCaptureState::FilteredNoop`] /
    /// [`verter_audit::AuditCaptureState::AuditDisabled`]).
    #[must_use]
    pub fn get_flow_return_type_with_audit(
        &self,
        function: &verter_type_expr::facts::FlowFunctionReturnIdentity,
        demand: ReturnProjectionDemand,
    ) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
        let canonical_id: &str = function.anchor.canonical_id.as_ref();
        let function_symbol: &str = function.anchor.symbol.as_ref();

        // Stamp a fresh request id and bookkeeping for the harness'
        // multi-request guard. Mirrors the other audited entry-points.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // Construct a per-request context. The footprint-attachment
        // pipeline plants the per-request accumulator (and workspace
        // VFS audit sink) so flow-return requests attach a mined
        // footprint when `footprint_capture=true`.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let footprint_scope = crate::typeinfo::footprint_attach::TypeinfoFootprintScope::install(
            self,
            request_id,
            footprint_capture,
        );
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::FlowReturnInference,
            footprint_capture,
            timing_capture,
            footprint_scope.accumulator(),
        );

        // BEFORE installing the TLS guard: construct the registration.
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        verter_debug_assert!(
            ctx.audit_registration.get().is_none(),
            "freshly-constructed RequestContext must have no audit_registration",
        );
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        // Resolve against a PROVEN-CURRENT snapshot (this entry-point
        // returns the value with no outer publish fence). On sustained
        // churn surface the typed `UnstableState` error rather than
        // answering from superseded state.
        let request_start = Instant::now();
        let outcome: Result<Arc<FlowReturnResult>, FlowReturnError> =
            match crate::typeinfo::current_store_view_for_query(self) {
                Some(current_view) => {
                    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                    let host_ctx = crate::resolver_core::HostResolverContext::from_current(
                        self,
                        &current_view,
                        overlay,
                    );
                    let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext =
                        &host_ctx;
                    let run = |dispatch: &ProjectSemanticDispatch<'_>| {
                        let key = dispatch.flow_return_key_with_demand(function, demand.clone());
                        match dispatch.execute_flow_return(key) {
                            crate::semantic_query::FlowReturnStep::Complete(result) => {
                                Ok(Arc::new(result))
                            }
                            crate::semantic_query::FlowReturnStep::NoValue(failure) => {
                                Err(FlowReturnError::Failure(failure))
                            }
                            // A hold cannot surface at a fresh top-level
                            // transaction (no in-flight frame exists to
                            // re-enter); treat a torn surfacing as
                            // undecided, never a fabricated value.
                            crate::semantic_query::FlowReturnStep::Hold(_) => {
                                Err(FlowReturnError::Failure(FlowReturnFailure::Unresolved))
                            }
                        }
                    };
                    match registration.as_ref() {
                        AuditRequestRegistration::Active(_) => {
                            let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                            let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
                            run(&dispatch)
                        }
                        AuditRequestRegistration::Noop => {
                            let _noop_guard = verter_audit::install_noop_observer();
                            let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
                            run(&dispatch)
                        }
                    }
                }
                None => Err(FlowReturnError::UnstableState {
                    attempts: crate::typeinfo::TYPEINFO_CURRENT_VIEW_RETRY_ATTEMPTS as u8,
                }),
            };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // Filtered kinds: return the cheap default-filled record. The
        // query still ran; no payload was collected and nothing is
        // published.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record =
                noop_flow_return_record(request_id, canonical_id, ctx.parent_request_id, state);
            return audited_from_outcome(outcome, record);
        }

        // Build the audit record — only the `Active` arm reaches here.
        // Snapshot the per-request flow counters from the context.
        let cold_computes = ctx.flow_return_cold_computes.load(Ordering::Relaxed);
        let payload = FlowReturnInferencePayload {
            function_symbol: function_symbol.to_string(),
            cold_computes,
            budget_exceeded_events: ctx.flow_return_budget_exceeded.load(Ordering::Relaxed),
            cycle_reentry_holds: ctx.flow_return_cycle_reentries.load(Ordering::Relaxed),
            partiality: observed_partiality(&outcome),
        };
        // Cold-vs-warm contract, counter side: a request that produced
        // a value with ZERO cold evaluations was served warm.
        let from_cache = outcome.is_ok() && cold_computes == 0;

        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };
        let store = RequestStoreAudit {
            cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
            ..RequestStoreAudit::default()
        };
        let memory = RequestMemoryAudit {
            process_rss_peak_bytes: ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            ..RequestMemoryAudit::default()
        };
        let waits = if ctx.timing_capture {
            Some(WaitAudit {
                lock_wait_ns: ctx.lock_wait_ns.load(Ordering::Relaxed),
                queue_wait_ns: ctx.queue_wait_ns.load(Ordering::Relaxed),
                lock_acquisitions: ctx.lock_acquisitions.load(Ordering::Relaxed),
            })
        } else {
            None
        };

        // Finalise the footprint through the shared miner (drain +
        // per-file attribution + deterministic mine). `(None, [])`
        // when capture is off.
        let (footprint, files) =
            crate::typeinfo::footprint_attach::mine_typeinfo_footprint(self, &ctx);

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_id.to_string(),
            target_identity: Some(verter_audit::RequestTargetIdentity::registered(
                canonical_id,
            )),
            kind: RequestKind::FlowReturnInference,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache,
            timings,
            memory,
            store,
            footprint,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files,
            waits,
            kind_payload: RequestKindPayload::FlowReturnInference(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: ctx.trace_id.clone(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        audited_from_outcome(outcome, cloned)
    }
}

/// Project the outcome's OWN typed partiality reason onto the audit
/// payload — the single observability projection of the flow
/// vocabulary, and a pure read.
///
/// It reads the reason the evaluator and the finalizer already certified
/// ([`FlowReturnResult::degradation`] on the usable arm, the typed
/// [`FlowReturnError`] on the no-value arm); it never re-derives one,
/// infers one from text, or guesses. Nothing here feeds admission: the
/// outcome is already bound, `from_cache` is computed from the cold
/// counter, and the returned tag is written into the audit record only.
///
/// `None` is the complete, warm-admissible outcome.
fn observed_partiality(
    outcome: &Result<Arc<FlowReturnResult>, FlowReturnError>,
) -> Option<FlowPartialityTag> {
    match outcome {
        Ok(result) => result
            .degradation()
            .map(|reason| FlowPartialityTag::Degraded(degradation_tag(reason))),
        Err(FlowReturnError::Failure(failure)) => {
            Some(FlowPartialityTag::NoValue(failure_tag(*failure)))
        }
        Err(FlowReturnError::UnstableState { .. }) => {
            Some(FlowPartialityTag::NoValue(FlowFailureTag::UnstableState))
        }
    }
}

/// Map one typed degradation reason onto its closed wire tag.
///
/// EXHAUSTIVE, with no wildcard arm and no `Debug` formatting: a new
/// [`FlowReturnDegradation`] or [`FlowGap`] variant fails to compile
/// here rather than silently collapsing into an existing bucket.
fn degradation_tag(degradation: FlowReturnDegradation) -> FlowDegradationTag {
    match degradation {
        FlowReturnDegradation::FlowGap(gap) => match gap {
            FlowGap::GuardNarrowing => FlowDegradationTag::GapGuardNarrowing,
            FlowGap::NominalRelation => FlowDegradationTag::GapNominalRelation,
            FlowGap::ClosureCapture => FlowDegradationTag::GapClosureCapture,
            FlowGap::AbruptCompletion => FlowDegradationTag::GapAbruptCompletion,
            FlowGap::UnmodeledExpression => FlowDegradationTag::GapUnmodeledExpression,
        },
        FlowReturnDegradation::NonCallableBinding => FlowDegradationTag::NonCallableBinding,
        FlowReturnDegradation::UnrepresentableCallee => FlowDegradationTag::UnrepresentableCallee,
        FlowReturnDegradation::FailedBindingInitializer => {
            FlowDegradationTag::FailedBindingInitializer
        }
        FlowReturnDegradation::UnappliedWriteEffect => FlowDegradationTag::UnappliedWriteEffect,
        FlowReturnDegradation::ConditionalVarDefinition => {
            FlowDegradationTag::ConditionalVarDefinition
        }
        FlowReturnDegradation::UnreducedDeclaredUnion => FlowDegradationTag::UnreducedDeclaredUnion,
        FlowReturnDegradation::UnresolvedValue => FlowDegradationTag::UnresolvedValue,
        FlowReturnDegradation::UnmodeledPosition => FlowDegradationTag::UnmodeledPosition,
    }
}

/// Map one typed no-value failure onto its closed wire tag.
///
/// EXHAUSTIVE through the nested closed enums too, so `Unsupported`,
/// `CallResolution` and `Budget` each surface WHICH inner reason
/// stopped the evaluation instead of one collapsed bucket.
fn failure_tag(failure: FlowReturnFailure) -> FlowFailureTag {
    match failure {
        FlowReturnFailure::Missing => FlowFailureTag::Missing,
        FlowReturnFailure::Unsupported(unsupported) => match unsupported {
            FlowReturnUnsupported::Loop => FlowFailureTag::UnsupportedLoop,
            FlowReturnUnsupported::Jump => FlowFailureTag::UnsupportedJump,
            FlowReturnUnsupported::InvokedClosureEffect => {
                FlowFailureTag::UnsupportedInvokedClosureEffect
            }
            FlowReturnUnsupported::With => FlowFailureTag::UnsupportedWith,
            FlowReturnUnsupported::ModuleDeclaration => {
                FlowFailureTag::UnsupportedModuleDeclaration
            }
        },
        FlowReturnFailure::Unresolved => FlowFailureTag::Unresolved,
        FlowReturnFailure::EmptyCycle => FlowFailureTag::EmptyCycle,
        FlowReturnFailure::UnmodeledDemandPoint => FlowFailureTag::UnmodeledDemandPoint,
        FlowReturnFailure::CallResolution(call) => match call {
            ResolveCallFailure::NotCallable => FlowFailureTag::CallNotCallable,
            ResolveCallFailure::NoApplicableOverload => FlowFailureTag::CallNoApplicableOverload,
            ResolveCallFailure::Undecidable => FlowFailureTag::CallUndecidable,
            ResolveCallFailure::Budget => FlowFailureTag::CallBudget,
        },
        FlowReturnFailure::Budget(reason) => match reason {
            verter_type_expr::facts::InferenceUnavailableReason::DepthBudgetExceeded => {
                FlowFailureTag::BudgetDepthExceeded
            }
            verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded => {
                FlowFailureTag::BudgetWorkExceeded
            }
        },
    }
}

/// Package a flow-return outcome and its audit record into the
/// [`AuditedResult`] carrier.
fn audited_from_outcome(
    outcome: Result<Arc<FlowReturnResult>, FlowReturnError>,
    audit: RequestAuditRecord,
) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
    match outcome {
        Ok(value) => AuditedResult::ok(value, audit),
        Err(error) => AuditedResult::err(error, audit),
    }
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled flow-return path. No per-request counters
/// are collected — the payload is the zero-valued default and
/// `capture_state` records why the full path was skipped.
fn noop_flow_return_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<u64>,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        target_identity: Some(verter_audit::RequestTargetIdentity::registered(
            canonical_id,
        )),
        kind: RequestKind::FlowReturnInference,
        parent_request_id: parent_request_id.map(|id| id.to_string()),
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::FlowReturnInference(FlowReturnInferencePayload::default()),
        capture_state,
        trace_id: String::new(),
    }
}
