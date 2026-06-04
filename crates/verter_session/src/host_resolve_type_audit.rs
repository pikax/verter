#![deny(missing_docs)]
//! `VerterHost::resolve_type_with_audit` — public audited entry-point
//! for type-resolution requests.
//!
//! Wires the shared
//! [`crate::audited_request::AuditedRequestBuilder::run`] generic
//! harness to the host-bound
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`] so
//! consumers (component-meta, LSP hover, MCP) can drive a query
//! through `SemanticQueryApi` and receive the matching
//! [`verter_audit::RequestAuditRecord`] in the same call.
//!
//! Boundary contract:
//!
//! 1. Construct an
//!    [`crate::host_audit_runtime::AuditRequestRegistration`] with
//!    `RequestKind::TypeResolution`.
//! 2. Branch on the registration's `Active` / `Noop` arm to install
//!    the matching observer in TLS (real
//!    [`crate::request_context::RequestContextGuard`] or
//!    [`verter_audit::install_noop_observer`]).
//! 3. Run the resolver query through `ProjectSemanticDispatch::execute`.
//! 4. Snapshot the per-request type-resolution counters from the
//!    active [`crate::request_context::RequestContext`].
//! 5. Build the [`verter_audit::RequestAuditRecord`] with
//!    [`verter_audit::RequestKindPayload::TypeResolution`].
//! 6. Finalise through the registration and return an
//!    [`verter_audit::AuditedResult`] carrier: `Ok(Some(node))` /
//!    `Ok(None)` for a non-fault miss / `Err(fault)` for a request
//!    fault, with the mandatory audit record alongside.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::{
    AuditedResult, ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, TypeResolutionPayload, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{ProjectionMode, SemanticNodeId, SemanticQueryApi, SemanticQueryKey};
use crate::VerterHost;

/// Outcome of a type-resolution query — the resolved
/// [`SemanticNodeId`] (or `None` if dispatch returned a miss). The
/// same shape any consumer would build for a single
/// `SemanticQueryApi::execute` call, lifted here for the public
/// `*_with_audit` API.
pub type TypeResolutionResult = SemanticNodeId;

/// Closed request-fault taxonomy for the type-resolution entry-points.
///
/// Distilled from the FAULT arms of
/// [`crate::semantic_query::QueryError`]. A `QueryError` describes
/// every way a semantic query can fail to produce a value, but only a
/// subset of those are *request faults* the public API surfaces as an
/// `Err`:
///
/// - [`crate::semantic_query::QueryError::Miss`],
///   [`crate::semantic_query::QueryError::RecursiveRef`], and
///   [`crate::semantic_query::QueryError::DeclPlaceholder`] are NOT
///   faults — they ride the success arm as `Ok(None)` (no resolved
///   node, but the request was well-formed and serviced).
/// - `UnsupportedIntrinsic`, `BudgetExceeded`, `UnstableState`,
///   `AliasCycle`, `Other`, and `ValueDomainMismatch` ARE request
///   faults — they map to the matching [`TypeResolutionRequestError`]
///   arm and ride the carrier's `Err`. `ValueDomainMismatch` has no
///   dedicated arm; it rides the text-bearing `Other` carrier.
///
/// The split is the entire reason the type-resolution entry-points
/// return an [`crate::AuditedResult`] with a typed `E` rather than a
/// `(Option<…>, …)` tuple that collapsed every error to `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeResolutionRequestError {
    /// A declaration resolved to `= intrinsic` but the active TS SDK
    /// advertises an intrinsic the verter intrinsic registry does not
    /// implement.
    UnsupportedIntrinsic {
        /// Intrinsic name the SDK advertised but verter cannot serve.
        name: Arc<str>,
    },
    /// The resolver hit one of its structured safety rails.
    BudgetExceeded(crate::semantic_query::BudgetExceededFailure),
    /// The completion fence exhausted its retry budget.
    UnstableState {
        /// Number of retry attempts the fence made before giving up.
        attempts: u8,
    },
    /// The path walker re-entered an alias on the same invocation.
    AliasCycle {
        /// The cycle participants, in walk order.
        chain: Arc<[Arc<str>]>,
    },
    /// Catch-all for text-bearing failures.
    Other(Arc<str>),
}

impl TypeResolutionRequestError {
    /// Classify a [`crate::semantic_query::QueryError`] as either a
    /// request fault (`Some(err)` → carrier `Err`) or a non-fault miss
    /// (`None` → carrier `Ok(None)`).
    ///
    /// `Miss` / `RecursiveRef` / `DeclPlaceholder` are non-faults: the
    /// query was well-formed but produced no resolved node, which the
    /// public API represents as `Ok(None)`. Every other arm is a
    /// genuine request fault.
    #[must_use]
    pub fn from_query_error(err: &crate::semantic_query::QueryError) -> Option<Self> {
        use crate::semantic_query::QueryError;
        match err {
            QueryError::Miss
            | QueryError::RecursiveRef { .. }
            | QueryError::DeclPlaceholder { .. } => None,
            QueryError::UnsupportedIntrinsic { name } => Some(Self::UnsupportedIntrinsic {
                name: Arc::clone(name),
            }),
            QueryError::BudgetExceeded(failure) => Some(Self::BudgetExceeded(failure.clone())),
            QueryError::UnstableState { attempts } => Some(Self::UnstableState {
                attempts: *attempts,
            }),
            QueryError::AliasCycle { chain } => Some(Self::AliasCycle {
                chain: Arc::clone(chain),
            }),
            QueryError::Other(text) => Some(Self::Other(Arc::clone(text))),
            // A typed caller asked for a value domain the query could not
            // provide. This is a genuine request fault; it surfaces through
            // the text-bearing carrier. (Unreachable until non-`TypeNode`
            // value producers exist, since every live key resolves to a
            // type node.)
            QueryError::ValueDomainMismatch { expected, actual } => Some(Self::Other(Arc::from(
                format!("value-domain mismatch: expected {expected:?}, got {actual:?}").as_str(),
            ))),
        }
    }
}

impl VerterHost {
    /// Run a single semantic query through the shared
    /// [`ProjectSemanticDispatch`] and return the resolved node — or a
    /// typed [`TypeResolutionRequestError`] — alongside the per-request
    /// [`RequestAuditRecord`], packaged in a single
    /// [`crate::AuditedResult`] carrier.
    ///
    /// Mirrors
    /// [`Self::get_component_meta_with_resolution`]'s audit
    /// lifecycle: registration constructed BEFORE the TLS observer
    /// install, query executed inside the `RequestContext` window,
    /// counters snapshotted from the context, record finalised
    /// through the registration.
    ///
    /// Outcome mapping:
    /// - `Ok(Some(node))` — dispatch produced a value (or a recursive
    ///   back-edge node).
    /// - `Ok(None)` — dispatch returned a non-fault miss
    ///   (`Miss` / `RecursiveRef` / `DeclPlaceholder`): the query was
    ///   well-formed but resolved no node.
    /// - `Err(fault)` — dispatch returned a genuine request fault
    ///   (`BudgetExceeded` / `UnstableState` / `AliasCycle` /
    ///   `UnsupportedIntrinsic` / `Other` / `ValueDomainMismatch`).
    ///   `ValueDomainMismatch` rides the text-bearing `Other` carrier.
    ///
    /// The carrier's `audit` field is always populated: an active
    /// registration carries the full payload
    /// ([`verter_audit::AuditCaptureState::ActiveStored`]); a filtered
    /// or disabled registration carries the cheap default-filled record
    /// ([`verter_audit::AuditCaptureState::FilteredNoop`] /
    /// [`verter_audit::AuditCaptureState::AuditDisabled`]).
    #[must_use]
    pub fn resolve_type_with_audit(
        &self,
        query: SemanticQueryKey,
        canonical_hint: &str,
    ) -> AuditedResult<Option<TypeResolutionResult>, TypeResolutionRequestError> {
        // Stamp a fresh request id and bookkeeping for the harness'
        // multi-request guard. Mirrors the component-meta entry-point.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // Determine the projection mode from the query so the audit
        // payload reports the caller's intent (Identity for queries
        // that do not carry a mode field — Conditional, KeyOf, …).
        let query_mode = query_projection_mode(&query);

        // Construct a per-request context. Footprint accumulator is
        // disabled for type-resolution requests — they do not collect
        // semantic-footprint events the way component-meta does.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_hint),
            RequestKind::TypeResolution,
            footprint_capture,
            timing_capture,
            None,
        );

        // BEFORE installing the TLS guard: construct the registration.
        // The `Noop` arm short-circuits when the consumer filter
        // rejects `TypeResolution`; the `Active` arm captures a slot
        // in the host's active-request registry.
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        debug_assert!(
            ctx.audit_registration.get().is_none(),
            "freshly-constructed RequestContext must have no audit_registration",
        );
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        // Install the matching TLS observer at the audit boundary.
        // Active registrations install the real
        // `RequestContextGuard`; Noop installs
        // `verter_audit::NoOpObserver` so emit sites still see
        // `Some(observer)` without paying downstream cost.
        //
        // Bind the dispatch ctor to a request-scoped
        // `HostResolverContext` so cache validators inside the
        // dispatch chain inherit the overlay-aware view instead of
        // paying a fresh workspace-sweep cost per call.
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let request_start = Instant::now();
        let result = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
                dispatch.execute_type_node(query)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
                dispatch.execute_type_node(query)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // Decode the dispatch result into the carrier outcome:
        // - Value / Recursive → Ok(Some(node)).
        // - Error(non-fault miss) → Ok(None).
        // - Error(request fault) → Err(fault).
        let outcome: Result<Option<TypeResolutionResult>, TypeResolutionRequestError> =
            match &result {
                crate::semantic_query::QueryResult::Value(
                    crate::semantic_query::SemanticQueryOutput { value: node, .. },
                ) => Ok(Some(*node)),
                crate::semantic_query::QueryResult::Recursive(node) => Ok(Some(*node)),
                crate::semantic_query::QueryResult::Error(err) => {
                    match TypeResolutionRequestError::from_query_error(err) {
                        Some(fault) => Err(fault),
                        None => Ok(None),
                    }
                }
            };

        // Filtered kinds: return the cheap default-filled record. The
        // query still ran; no payload was collected and nothing is
        // published.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record = noop_type_resolution_record(
                request_id,
                canonical_hint,
                ctx.parent_request_id,
                state,
            );
            return audited_from_outcome(outcome, record);
        }

        // Build the audit record. Only the `Active` arm reaches here.
        // Snapshot per-request counters from the active context AFTER
        // dispatch completes (the context is still alive — the guard
        // dropped above only un-installed it from TLS, not the
        // owning Arc on this stack frame).
        let payload = TypeResolutionPayload {
            query_mode: ProjectionModeTag::from(query_mode),
            hops: u32::try_from(ctx.type_resolution_hops.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            navigations: u32::try_from(ctx.type_resolution_navigations.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            expansions: u32::try_from(ctx.type_resolution_expansions.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            conditional_decisions: u32::try_from(
                ctx.type_resolution_conditional_decisions
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            ref_root_cycle_hits: u32::try_from(
                ctx.type_resolution_ref_root_cycle_hits
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            projection_ops_executed: u32::try_from(
                ctx.type_resolution_projection_ops.load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            depth_high_water: ctx.type_resolution_depth_high_water.load(Ordering::Relaxed),
            recursion_limit_reached: ctx
                .type_resolution_recursion_limit_reached
                .load(Ordering::Relaxed),
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
        };

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

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_hint.to_string(),
            kind: RequestKind::TypeResolution,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::TypeResolution(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: ctx.trace_id.clone(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        audited_from_outcome(outcome, cloned)
    }
}

/// Package a type-resolution outcome and its audit record into the
/// [`AuditedResult`] carrier.
fn audited_from_outcome(
    outcome: Result<Option<TypeResolutionResult>, TypeResolutionRequestError>,
    audit: RequestAuditRecord,
) -> AuditedResult<Option<TypeResolutionResult>, TypeResolutionRequestError> {
    match outcome {
        Ok(value) => AuditedResult::ok(value, audit),
        Err(error) => AuditedResult::err(error, audit),
    }
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled type-resolution path. No per-request
/// counters are collected — the payload is the zero-valued default and
/// `capture_state` records why the full path was skipped.
fn noop_type_resolution_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<u64>,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::TypeResolution,
        parent_request_id: parent_request_id.map(|id| id.to_string()),
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::TypeResolution(TypeResolutionPayload::default()),
        capture_state,
        trace_id: String::new(),
    }
}

/// Project the caller-visible projection mode from a
/// [`SemanticQueryKey`]. Variants without a `mode` field map to
/// [`ProjectionMode::Identity`] — they do not consume a projection
/// budget.
fn query_projection_mode(key: &SemanticQueryKey) -> ProjectionMode {
    match key {
        SemanticQueryKey::ProjectPath { context, .. } => context.mode,
        SemanticQueryKey::ProjectMember { mode, .. }
        | SemanticQueryKey::IndexedAccess { mode, .. }
        | SemanticQueryKey::ResolveMacroPayload { mode, .. } => *mode,
        SemanticQueryKey::Instantiate { context, .. } => context.mode,
        SemanticQueryKey::ResolveDecl(_)
        | SemanticQueryKey::KeyOf { .. }
        | SemanticQueryKey::MappedType { .. }
        | SemanticQueryKey::Conditional { .. }
        | SemanticQueryKey::TypeOf { .. }
        | SemanticQueryKey::NormalizeUnion { .. }
        | SemanticQueryKey::NormalizeIntersection { .. }
        | SemanticQueryKey::ResolvedNamedType { .. }
        | SemanticQueryKey::Relate { .. } => ProjectionMode::Identity,
    }
}

#[cfg(test)]
mod request_error_classification_tests {
    use super::TypeResolutionRequestError;
    use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
    use crate::semantic_query::QueryError;
    use std::sync::Arc;

    fn budget_failure() -> BudgetExceededFailure {
        BudgetExceededFailure {
            domain: BudgetDomain::SolverResolveSteps,
            limit: 10,
            actual: 11,
            context: "test".to_string(),
        }
    }

    // ── Non-faults ride Ok(None): the classifier returns `None` so the
    //    entry-point maps them to `Ok(None)` rather than `Err`. This is
    //    the exact behaviour the pre-change `QueryResult::Error(_) =>
    //    None` collapse had for EVERY arm — so this half alone would
    //    pass against the old tree. The discriminating half is below.

    #[test]
    fn miss_is_not_a_request_fault() {
        assert!(TypeResolutionRequestError::from_query_error(&QueryError::Miss).is_none());
    }

    #[test]
    fn recursive_ref_is_not_a_request_fault() {
        let err = QueryError::RecursiveRef {
            name: Arc::from("Tree"),
        };
        assert!(TypeResolutionRequestError::from_query_error(&err).is_none());
    }

    #[test]
    fn decl_placeholder_is_not_a_request_fault() {
        let err = QueryError::DeclPlaceholder {
            canonical_id: Arc::from("/a.ts"),
            name: Arc::from("Foo"),
            whole_hash: Default::default(),
        };
        assert!(TypeResolutionRequestError::from_query_error(&err).is_none());
    }

    // ── Faults ride Err: the classifier returns `Some(fault)` so the
    //    entry-point maps them to `Err(..)`. THIS is the discriminating
    //    half: against the pre-change tree the resolver collapsed every
    //    `QueryResult::Error(_)` (faults included) to `None` (→ Ok(None)
    //    in carrier terms), so a test asserting a fault surfaces as
    //    `Some(Err)` could not be written against the old behaviour. It
    //    fails on the old collapse and passes on the new split.

    #[test]
    fn budget_exceeded_is_a_request_fault() {
        let err = QueryError::BudgetExceeded(budget_failure());
        assert_eq!(
            TypeResolutionRequestError::from_query_error(&err),
            Some(TypeResolutionRequestError::BudgetExceeded(budget_failure())),
            "BudgetExceeded must surface as a request fault, NOT collapse to Ok(None)"
        );
    }

    #[test]
    fn unstable_state_is_a_request_fault() {
        let err = QueryError::UnstableState { attempts: 3 };
        assert_eq!(
            TypeResolutionRequestError::from_query_error(&err),
            Some(TypeResolutionRequestError::UnstableState { attempts: 3 }),
        );
    }

    #[test]
    fn alias_cycle_is_a_request_fault() {
        let chain: Arc<[Arc<str>]> = Arc::from(vec![Arc::from("A"), Arc::from("B")]);
        let err = QueryError::AliasCycle {
            chain: Arc::clone(&chain),
        };
        assert_eq!(
            TypeResolutionRequestError::from_query_error(&err),
            Some(TypeResolutionRequestError::AliasCycle { chain }),
        );
    }

    #[test]
    fn unsupported_intrinsic_is_a_request_fault() {
        let err = QueryError::UnsupportedIntrinsic {
            name: Arc::from("NoSuchIntrinsic"),
        };
        assert_eq!(
            TypeResolutionRequestError::from_query_error(&err),
            Some(TypeResolutionRequestError::UnsupportedIntrinsic {
                name: Arc::from("NoSuchIntrinsic"),
            }),
        );
    }

    #[test]
    fn other_is_a_request_fault() {
        let err = QueryError::Other(Arc::from("boom"));
        assert_eq!(
            TypeResolutionRequestError::from_query_error(&err),
            Some(TypeResolutionRequestError::Other(Arc::from("boom"))),
        );
    }
}
