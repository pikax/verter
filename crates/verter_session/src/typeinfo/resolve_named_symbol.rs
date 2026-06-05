#![deny(missing_docs)]
//! `VerterHost::resolve_named_symbol_with_audit` —
//! audited resolution of a named declaration in a file scope, with
//! optional generic instantiation and a configurable
//! [`ProjectionMode`].
//!
//! Mirrors the lifecycle of
//! [`crate::host_resolve_type_audit::resolve_type_with_audit`]:
//! registration → TLS observer install → dispatch.execute →
//! payload snapshot → finalise.
//!
//! Public API surface (the resolve-named-symbol contract):
//!
//! ```ignore
//! pub fn resolve_named_symbol_with_audit(
//!     host: &VerterHost,
//!     canonical_id: &str,
//!     name: &str,
//!     type_args: &[TypeExpr],
//!     mode: ProjectionMode,
//! ) -> AuditedResult<Option<SemanticNodeId>, TypeResolutionRequestError>;
//!
//! pub fn resolve_named_symbol(...)  -> Option<SemanticNodeId>;
//! ```
//!
//! Default-mode policy:
//! - Generic carrier (declaration-site type parameters) → `Navigate`.
//! - Non-generic decl → `Expanded`.
//! - Identity returns the alias node verbatim (no unwrap).
//!
//! Type args at the boundary are
//! [`verter_type_expr::TypeExpr`]s; the host
//! method lowers them through
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch::lower_type_expr_in_scope_with_mode`]
//! per the resolve-named-symbol contract — type_args are lowered to
//! `SemanticNodeId`s inside the host method before dispatch.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::{
    AuditedResult, ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, TypeResolutionPayload, WaitAudit,
};
use verter_type_expr::TypeExpr;

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::host_resolve_type_audit::TypeResolutionRequestError;
use crate::instant::Instant;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{
    NodeScopeId, ProjectionMode, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
};
use crate::VerterHost;

/// Sentinel that flags the caller wants the host's default mode.
/// Distinct from the `ProjectionMode` enum so callers can opt out
/// explicitly without our defaulting their `Navigate` argument.
///
/// Implementation: callers pass `Some(mode)` to fix the mode, `None`
/// to take the host's default.
pub type ResolveMode = Option<ProjectionMode>;

impl VerterHost {
    /// Resolve `name` in `canonical_id`'s top-level scope, optionally
    /// instantiating with `type_args`, returning the resolved node and
    /// the request's audit record.
    ///
    /// `mode = None` selects the host's default (Navigate for
    /// generic carriers, Expanded otherwise). `Some(mode)` overrides
    /// the default.
    ///
    /// Returns an [`crate::AuditedResult`] carrier. The error type is
    /// the shared [`TypeResolutionRequestError`] — the SAME
    /// dispatch-fault taxonomy [`Self::resolve_type_with_audit`] uses —
    /// because this path resolves through the one shared typed-IR
    /// engine, not the wire request validator. Outcome mapping:
    /// - `Ok(Some(node))` — dispatch produced a value.
    /// - `Ok(None)` — a non-fault miss (`Miss` / `RecursiveRef` /
    ///   `DeclPlaceholder` / lowering miss): the request was
    ///   well-formed but resolved no node.
    /// - `Err(fault)` — a genuine dispatch fault (`BudgetExceeded` /
    ///   `UnstableState` / `AliasCycle` / `UnsupportedIntrinsic` /
    ///   `Other` / `ValueDomainMismatch`). `ValueDomainMismatch` rides
    ///   the text-bearing `Other` carrier.
    ///
    /// The carrier's `audit` field is always populated:
    /// [`verter_audit::AuditCaptureState::ActiveStored`] on the
    /// full-capture path, or the cheap default-filled record marked
    /// [`verter_audit::AuditCaptureState::FilteredNoop`] /
    /// [`verter_audit::AuditCaptureState::AuditDisabled`].
    #[must_use]
    pub fn resolve_named_symbol_with_audit(
        &self,
        canonical_id: &str,
        name: &str,
        type_args: &[Arc<TypeExpr>],
        mode: ResolveMode,
    ) -> AuditedResult<Option<SemanticNodeId>, TypeResolutionRequestError> {
        // Registration / context setup mirrors
        // `resolve_type_with_audit`. We construct the registration
        // BEFORE installing the TLS guard so the `Noop` arm can
        // short-circuit when the consumer filter rejects the kind.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        // Thread the host's projection-op budget so this dispatch path
        // honours the same fuse as every other resolution entry-point;
        // a tripped budget surfaces as a `BudgetExceeded` dispatch
        // fault on the carrier's `Err` arm rather than running to the
        // default 2000-op cap.
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::TypeResolution,
            footprint_capture,
            timing_capture,
            None,
            self.config.projection_op_budget,
        );

        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        debug_assert!(ctx.audit_registration.get().is_none());
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        let request_start = Instant::now();
        let (outcome, effective_mode) = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                resolve_named_symbol_inner(self, canonical_id, name, type_args, mode)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                resolve_named_symbol_inner(self, canonical_id, name, type_args, mode)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record = noop_type_resolution_record(
                request_id,
                canonical_id,
                ctx.parent_request_id,
                ctx.trace_id.clone(),
                state,
            );
            return audited_from_outcome(outcome, record);
        }

        // Build the audit record. Counters come straight off the
        // RequestContext atomics, which the dispatch
        // `execute()` instrumentation has been incrementing during
        // the call.
        let payload = TypeResolutionPayload {
            query_mode: ProjectionModeTag::from(effective_mode),
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
            canonical_id: canonical_id.to_string(),
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

    /// Non-audit variant. Identical resolution semantics; the audit
    /// record is dropped at the boundary. A dispatch fault collapses to
    /// `None` here (the non-audit surface has no error channel) — use
    /// [`Self::resolve_named_symbol_with_audit`] to observe the typed
    /// fault.
    #[must_use]
    pub fn resolve_named_symbol(
        &self,
        canonical_id: &str,
        name: &str,
        type_args: &[Arc<TypeExpr>],
        mode: ResolveMode,
    ) -> Option<SemanticNodeId> {
        self.resolve_named_symbol_with_audit(canonical_id, name, type_args, mode)
            .into_result()
            .ok()
            .flatten()
    }
}

/// Package a resolve-named-symbol outcome and its audit record into the
/// [`AuditedResult`] carrier.
fn audited_from_outcome(
    outcome: Result<Option<SemanticNodeId>, TypeResolutionRequestError>,
    audit: RequestAuditRecord,
) -> AuditedResult<Option<SemanticNodeId>, TypeResolutionRequestError> {
    match outcome {
        Ok(value) => AuditedResult::ok(value, audit),
        Err(error) => AuditedResult::err(error, audit),
    }
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled path. No per-request counters are collected
/// — the payload is the zero-valued default and `capture_state`
/// records why the full path was skipped.
fn noop_type_resolution_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<u64>,
    trace_id: String,
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
        trace_id,
    }
}

/// Inner resolution function shared by the audit / non-audit entry
/// points. Returns the resolved node and the *effective* mode (after
/// default-mode resolution) so the audit payload can record what the
/// resolver actually ran with.
#[allow(clippy::type_complexity)]
fn resolve_named_symbol_inner(
    host: &VerterHost,
    canonical_id: &str,
    name: &str,
    type_args: &[Arc<TypeExpr>],
    requested_mode: ResolveMode,
) -> (
    Result<Option<SemanticNodeId>, TypeResolutionRequestError>,
    ProjectionMode,
) {
    // Build a request-bound `HostResolverContext` for the dispatch so
    // resolver-tier reads bind to a real overlay-aware view rather
    // than the panic-shimmed bare-host `impl ResolverContext for
    // VerterHost`.
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let scope_arc: Arc<str> = Arc::from(canonical_id);

    // Determine whether the decl carries declaration-site type
    // parameters. The shallow inventory is the authority — if the
    // file's `ShallowFileState::symbol(name)` returns an entry with a
    // non-empty `type_parameters` list, we treat the resolved decl as
    // a generic carrier. Value symbols (functions, etc.) are not
    // generic carriers in this sense; the `resolve_named_symbol`
    // contract is rooted on the type-side declaration.
    let is_generic_carrier = host
        .shallow_file_state(canonical_id)
        .and_then(|state| {
            state
                .symbol(name)
                .map(|sym| !sym.type_parameters.is_empty())
        })
        .unwrap_or(false);

    // Default-mode selection: generic carriers default to Navigate
    // so the declaration stays unexpanded; non-generic declarations
    // default to Expanded so callers receive the full projection.
    let effective_mode = match requested_mode {
        Some(mode) => mode,
        None => {
            if is_generic_carrier {
                ProjectionMode::Navigate
            } else {
                ProjectionMode::Expanded
            }
        }
    };

    // Lower type_args in the call-scope. Args are lowered in
    // `Navigate` mode regardless of the terminal mode — the args
    // themselves are a context inherited by the instantiation, not
    // the body that is being projected.
    let mut lowered_args: Vec<SemanticNodeId> = Vec::with_capacity(type_args.len());
    for arg in type_args {
        match dispatch.lower_type_expr_in_scope_with_mode(
            canonical_id,
            arg.as_ref(),
            ProjectionMode::Navigate,
        ) {
            Some(id) => lowered_args.push(id),
            None => {
                // Lowering miss → bail out and surface a None
                // resolution rather than partially instantiating. A
                // lowering miss is a non-fault (`Ok(None)`); a genuine
                // dispatch fault while lowering would have been raised
                // inside the dispatch itself.
                return (Ok(None), effective_mode);
            }
        }
    }

    // Resolve the bare declaration. The dispatch entry-point
    // memoises this through its `execute_cooperative` path. Note
    // that `ResolveDecl` may legitimately return an
    // `Opaque(DeclPlaceholder { … })` carrier when the symbol
    // exists but its body has not been materialised yet — that is
    // a *signal* to dispatch through `Instantiate { args: [],
    // body_mode }`, NOT a miss (per `QueryError::DeclPlaceholder`
    // contract: "Walk/enumerate code treats this as 'expandable via
    // Instantiate' rather than 'not found.'").
    let resolve_decl_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::clone(&scope_arc),
            local_scope: None,
        },
        name: Arc::from(name),
    });
    let decl_node_opt = match dispatch.execute_type_node(resolve_decl_key) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => Some(node),
        QueryResult::Recursive(node) => Some(node),
        QueryResult::Error(err) => {
            // A genuine dispatch fault on the bare-decl probe is a
            // request fault — surface it. A non-fault miss
            // (`Miss` / `RecursiveRef` / `DeclPlaceholder`) leaves the
            // fallback node `None` and the Instantiate path below
            // continues.
            match TypeResolutionRequestError::from_query_error(&err) {
                Some(fault) => return (Err(fault), effective_mode),
                None => None,
            }
        }
    };

    // Always dispatch through `Instantiate` so the body materialises
    // in the chosen mode. This is the path that lifts a
    // `DeclPlaceholder` into a concrete body. Build the
    // DeclIdentity from the file-scope and the decl name using the
    // file's current whole_hash; two callers in the same file
    // generation produce the same identity and therefore the same
    // memo key.
    //
    // Alias-unwrap policy:
    // - `Identity`: dispatch with `body_mode: Identity`, return the
    //   resolved alias-shell verbatim (do NOT unwrap).
    // - `Navigate` / `Expanded` / `Shallow`: dispatch with the
    //   chosen mode, unwrap one `SemanticNodeData::Alias(inner)`
    //   hop afterwards.
    let Some(shallow) = host.shallow_file_state(canonical_id) else {
        return (Ok(decl_node_opt), effective_mode);
    };
    let scope_node = NodeScopeId::File {
        canonical_id: Arc::clone(&scope_arc),
        whole_hash: shallow.whole_hash,
        local_scope: None,
    };
    let _ = &scope_node;
    let base = dispatch.type_slot_for(Arc::clone(&scope_arc), Arc::from(name));

    let instantiate_key = SemanticQueryKey::Instantiate {
        context: dispatch.instantiate_context_for(
            &scope_arc,
            crate::semantic_query::ProjectionReductionContext::published(effective_mode),
        ),
        base,
        args: Arc::from(lowered_args.into_boxed_slice()),
    };
    let node = match dispatch.execute_type_node(instantiate_key) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(err) => {
            // Instantiate failed. A genuine dispatch fault is surfaced
            // as `Err`; a non-fault miss falls back to the original
            // `ResolveDecl` node (when present) so callers still
            // receive *something* identifiable rather than a None.
            return (classify_dispatch_error(&err, decl_node_opt), effective_mode);
        }
    };
    let final_node = if matches!(effective_mode, ProjectionMode::Identity) {
        node
    } else {
        match materialize_through_aliases(host, &dispatch, node, effective_mode) {
            Ok(materialized) => materialized,
            // A hard dispatch fault during nested materialization
            // propagates as `Err` rather than silently degrading to
            // the un-materialised placeholder.
            Err(fault) => return (Err(fault), effective_mode),
        }
    };
    (Ok(Some(final_node)), effective_mode)
}

/// Classify a `QueryResult::Error(err)` arm into the carrier outcome a
/// typeinfo resolution entry-point returns.
///
/// This is the single decode point the resolve / evaluate paths route
/// their dispatch errors through, mirroring
/// [`crate::host_resolve_type_audit::resolve_type_with_audit`]'s split:
/// - A genuine dispatch FAULT (`BudgetExceeded` / `UnstableState` /
///   `AliasCycle` / `UnsupportedIntrinsic` / `Other` /
///   `ValueDomainMismatch`, the last riding the text-bearing `Other`
///   carrier) → `Err(fault)`.
/// - A non-fault MISS (`Miss` / `RecursiveRef` / `DeclPlaceholder`) →
///   `Ok(fallback)`, where `fallback` is whatever identifiable node the
///   caller already resolved (e.g. the bare `ResolveDecl` node) — `None`
///   when there is none.
///
/// Both the top-level Instantiate path and the nested
/// [`materialize_through_aliases`] placeholder hop route their
/// `QueryResult::Error` arms through this single decode point, so a real
/// dispatch fault is never indistinguishable from a miss.
pub(crate) fn classify_dispatch_error(
    err: &crate::semantic_query::QueryError,
    fallback: Option<SemanticNodeId>,
) -> Result<Option<SemanticNodeId>, TypeResolutionRequestError> {
    match TypeResolutionRequestError::from_query_error(err) {
        Some(fault) => Err(fault),
        None => Ok(fallback),
    }
}

/// Walk the alias / placeholder chain on a resolved node until we
/// land on a concrete body. Used by non-Identity modes to materialise
/// references that the dispatch returned as
/// `SemanticNodeData::Alias(inner)` shells or
/// `SemanticNodeData::Opaque(QueryError::DeclPlaceholder { … })`
/// carriers.
///
/// Bounded by a small step budget so a pathological cycle can't
/// hang the resolver — the dispatch's own cycle detection catches
/// genuine alias cycles and returns `Opaque(AliasCycle)` long
/// before this loop runs out of steps.
fn materialize_through_aliases(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch<'_>,
    start: SemanticNodeId,
    mode: ProjectionMode,
) -> Result<SemanticNodeId, TypeResolutionRequestError> {
    debug_assert!(!matches!(mode, ProjectionMode::Identity));
    let store = host.project_type_store().semantic_graph();
    let mut current = start;
    for _ in 0..16 {
        let data = store.node_data(current);
        match data.as_deref() {
            Some(SemanticNodeData::Alias(inner)) => {
                current = *inner;
                continue;
            }
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash: _,
                },
            )) => {
                // Materialise the placeholder by dispatching an
                // empty-args Instantiate against its identity.
                let base = dispatch.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
                let key = SemanticQueryKey::Instantiate {
                    context: dispatch.instantiate_context_for(
                        canonical_id,
                        crate::semantic_query::ProjectionReductionContext::published(mode),
                    ),
                    base,
                    args: Arc::from(Vec::new().into_boxed_slice()),
                };
                let step_result = match dispatch.execute_type_node(key) {
                    QueryResult::Value(SemanticQueryOutput { value: node, .. }) => {
                        QueryResult::Value(node)
                    }
                    QueryResult::Recursive(node) => QueryResult::Recursive(node),
                    QueryResult::Error(err) => QueryResult::Error(err),
                };
                match classify_materialization_step(step_result, current)? {
                    MaterializationStep::Continue(next) => {
                        current = next;
                        continue;
                    }
                    MaterializationStep::Stop(node) => return Ok(node),
                }
            }
            _ => return Ok(current),
        }
    }
    Ok(current)
}

/// The next action the placeholder-materialisation loop takes after a
/// nested `Instantiate` dispatch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MaterializationStep {
    /// Advance the loop to `next` (the dispatch produced a fresh node).
    Continue(SemanticNodeId),
    /// Stop and return `node` as the materialised result.
    Stop(SemanticNodeId),
}

/// Decide the loop's next step from the nested `Instantiate` result.
///
/// This is the single decode point the placeholder-materialisation loop
/// (in both [`materialize_through_aliases`] and its
/// `evaluate_type_expression` sibling) routes its nested dispatch result
/// through, mirroring the top-level [`classify_dispatch_error`] split:
/// - `Value(next)` / `Recursive(next)` → `Continue(next)`, unless the
///   dispatch returned the same `current` placeholder (no progress), in
///   which case `Stop(current)`.
/// - `Error(err)` → a genuine dispatch FAULT propagates as
///   `Err(fault)`; a non-fault miss keeps the degraded `current` node as
///   `Ok(Stop(current))`.
pub(crate) fn classify_materialization_step(
    result: QueryResult<SemanticNodeId>,
    current: SemanticNodeId,
) -> Result<MaterializationStep, TypeResolutionRequestError> {
    match result {
        QueryResult::Value(next) | QueryResult::Recursive(next) => {
            if next == current {
                // No progress — give up on the degraded node.
                Ok(MaterializationStep::Stop(current))
            } else {
                Ok(MaterializationStep::Continue(next))
            }
        }
        QueryResult::Error(err) => match classify_dispatch_error(&err, Some(current)) {
            Ok(node) => Ok(MaterializationStep::Stop(node.unwrap_or(current))),
            Err(fault) => Err(fault),
        },
    }
}
