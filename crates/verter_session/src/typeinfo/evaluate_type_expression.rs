#![deny(missing_docs)]
//! `VerterHost::evaluate_type_expression_with_audit` —
//! synthesise a scratch TypeScript file rooted at the requested
//! `scope`, evaluate one trailing
//! `type __VerterScratch = <expression>;` declaration, and return
//! the resolved semantic-graph node id.
//!
//! See [`super::types::EvaluateTypeExpressionRequest`] for the
//! request shape. The scratch URI contract is:
//!
//! ```text
//! verter://typeinfo/<sha256(scope_canonical || "\0"
//!                          || expression
//!                          || "\0"
//!                          || serialize(extra_imports))[..16]>.ts
//! ```
//!
//! Cacheable requests publish `(uri, node_id)` to the host-owned
//! [`super::scratch_cache::ScratchCache`] so a repeat request reuses
//! the synthesised file. Non-cacheable requests evict the upserted
//! scratch file at the end of the call (eviction preserves the
//! scratch's semantic-graph memo for cross-mode materialized-point
//! satisfaction; only miss/fault terminals fully remove).
//!
//! The scratch is upserted before resolution and only gains an LRU
//! owner once a cacheable request reaches the `scratch_cache` write.
//! Any miss/failure between the upsert and that write (a non-current
//! store view, a missing shallow surface, a resolution error) returns
//! with *this* request's scratch unowned, so those paths remove it
//! regardless of `cacheable` — otherwise a repeatedly-missing cacheable
//! request leaks orphaned host/scheduler state. The removal is
//! **ownership-aware** (see [`remove_scratch`]): the URI is
//! content-addressed, so a concurrent sibling request for the same
//! triple may have reached the success path and now own the same URI in
//! `scratch_cache`; the cleanup re-checks ownership under the
//! `scratch_cache` lock and skips removal when a sibling owns it, so it
//! never deletes a cache-owned host file. Removal is a full
//! `host.remove` (not `host.evict`): a scratch URI is synthetic, so the
//! "reload from disk later" eviction state would strand it.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use verter_audit::{
    AuditedResult, ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, TypeResolutionPayload, WaitAudit,
};

use super::types::{EvaluateTypeExpressionRequest, ImportSpec, NamedImport};
use crate::host_audit_runtime::AuditRequestRegistration;
use crate::host_resolve_type_audit::TypeResolutionRequestError;
use crate::instant::Instant;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{
    ProjectionMode, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput,
};
use crate::types::UpsertRequest;
use crate::VerterHost;

/// Name of the synthetic alias produced by the scratch file. The
/// expression body is wrapped as `type <NAME> = <expression>;` and
/// resolved by name afterwards.
const SCRATCH_ALIAS_NAME: &str = "__VerterScratch";

/// `verter://typeinfo/<hash>.ts` — base URI scheme for synthesised
/// scratch files. Distinct from `verter-virtual` (LSP's scheme) so
/// scratch files never collide with user-visible virtual ids.
const SCRATCH_URI_PREFIX: &str = "verter://typeinfo/";

impl VerterHost {
    /// Evaluate `req.expression` in `req.scope`'s context and return
    /// the resolved semantic-graph node id alongside the audit
    /// record.
    ///
    /// **Scratch URI**: a sha256 of
    /// `scope_canonical || \0 || expression || \0 || serialize(extra_imports)`,
    /// truncated to 16 bytes (32 hex chars), prefixed with
    /// `verter://typeinfo/`, and suffixed `.ts`. Two scopes with the
    /// same expression produce different URIs — `scope_canonical` is
    /// hashed explicitly.
    ///
    /// **Cache discipline**:
    /// - `cacheable: true` — first call synthesises + upserts +
    ///   resolves; subsequent calls with the same URI return the
    ///   cached `node_id` directly (the audit record still emits, with
    ///   `from_cache: true`).
    /// - `cacheable: false` — synthesis is one-shot; the scratch file
    ///   is removed at the end of the call.
    ///
    /// **LRU eviction**: at default capacity 64 the oldest-accessed
    /// entry is evicted on cold insertion of a 65th URI. The evicted
    /// entry's scratch file is also fully removed from the host so
    /// memory does not grow unbounded.
    ///
    /// Returns an [`crate::AuditedResult`] carrier. The error type is
    /// the shared [`TypeResolutionRequestError`] — the SAME
    /// dispatch-fault taxonomy [`Self::resolve_type_with_audit`] uses —
    /// because this path resolves through the one shared typed-IR
    /// engine, not the wire request validator. Outcome mapping:
    /// - `Ok(Some(node))` — dispatch produced a value.
    /// - `Ok(None)` — a non-fault miss: a dispatch miss classified by
    ///   `TypeResolutionRequestError::from_query_error`
    ///   (`Miss` / `RecursiveRef` / `DeclPlaceholder` or a typed
    ///   semantic sentinel), an upsert failure, or a missing scratch
    ///   shallow state — the request was well-formed but resolved no
    ///   node.
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
    pub fn evaluate_type_expression_with_audit(
        &self,
        req: EvaluateTypeExpressionRequest,
    ) -> AuditedResult<Option<SemanticNodeId>, TypeResolutionRequestError> {
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let canonical_scope: Arc<str> = Arc::from(req.scope.as_str());
        // Thread the host's projection-op budget so this dispatch path
        // honours the same fuse as every other resolution entry-point;
        // a tripped budget surfaces as a `BudgetExceeded` dispatch
        // fault on the carrier's `Err` arm rather than running to the
        // default 2000-op cap.
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            request_id,
            Arc::clone(&canonical_scope),
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
        let scratch_uri = compute_scratch_uri(&req.scope, &req.expression, &req.extra_imports);

        let (outcome, from_cache) = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                evaluate_inner(self, &req, &scratch_uri)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                evaluate_inner(self, &req, &scratch_uri)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record = noop_evaluate_record(
                request_id,
                &req.scope,
                ctx.parent_request_id,
                from_cache,
                ctx.trace_id.clone(),
                state,
            );
            return audited_from_outcome(outcome, record);
        }

        let payload = TypeResolutionPayload {
            query_mode: ProjectionModeTag::from(req.mode),
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
            semantic_query_dispatch_mask: ctx.type_resolution_dispatched_query_tags_mask(),
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
            canonical_id: req.scope.clone(),
            kind: RequestKind::TypeResolution,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache,
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

/// Package an evaluate-type-expression outcome and its audit record
/// into the [`AuditedResult`] carrier.
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
/// the filtered / disabled evaluate path. No per-request counters are
/// collected — the payload is the zero-valued default and
/// `capture_state` records why the full path was skipped.
fn noop_evaluate_record(
    request_id: u64,
    scope: &str,
    parent_request_id: Option<u64>,
    from_cache: bool,
    trace_id: String,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: scope.to_string(),
        kind: RequestKind::TypeResolution,
        parent_request_id: parent_request_id.map(|id| id.to_string()),
        from_cache,
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

/// Inner synthesis + resolution logic shared by the audit /
/// non-audit entry-points.
///
/// Returns `(outcome, from_cache)`. `from_cache = true` means the
/// scratch URI was found in the host's cache and resolution did not
/// re-synthesise / re-upsert. `outcome` is `Err(fault)` on a genuine
/// dispatch fault, `Ok(None)` on a non-fault miss (upsert failure,
/// missing scratch shallow state, or a dispatch miss classified by
/// `TypeResolutionRequestError::from_query_error` — `Miss` /
/// `RecursiveRef` / `DeclPlaceholder` or a typed semantic sentinel),
/// and `Ok(Some(node))` on success.
#[allow(clippy::type_complexity)]
fn evaluate_inner(
    host: &VerterHost,
    req: &EvaluateTypeExpressionRequest,
    scratch_uri: &str,
) -> (
    Result<Option<SemanticNodeId>, TypeResolutionRequestError>,
    bool,
) {
    // Cache fast-path. Only consulted when the caller asked for
    // caching — otherwise the cache is bypassed in both directions.
    if req.cacheable {
        let mut guard = host.scratch_cache().lock();
        if let Some(node_id) = guard.get(scratch_uri) {
            return (Ok(Some(node_id)), true);
        }
    }

    // Synthesise the scratch source. Each import in `extra_imports`
    // produces one `import` declaration; the expression body is
    // wrapped in a single trailing
    // `type __VerterScratch = <expression>;`. When the scope's
    // eval-source is available it is inlined as a prelude so the
    // scratch's lookup environment carries every top-level binding
    // the scope publishes — including the SFC-synthesised `default`
    // for `.vue` scopes (see `vue_default_synth`).
    let scope_eval_source = host
        .ensure_indexed_ready_serve(&req.scope)
        .map(|serve| Arc::clone(&serve.indexed.eval_source));
    let source = synthesise_source(
        &req.expression,
        &req.extra_imports,
        scope_eval_source.as_deref(),
    );

    // Upsert the scratch file. The canonical id is the URI; aliases
    // remain empty — typeinfo URIs do not appear in user-visible
    // alias maps. A `.ts` URI classifies as a plain script.
    let upsert_result = host.upsert(UpsertRequest {
        canonical_id: Some(scratch_uri.to_string()),
        input_id: scratch_uri.to_string(),
        source: Arc::from(source.as_str()),
        file_language: host.language_classifier().classify(scratch_uri),
        aliases: Vec::new(),
    });
    if upsert_result.is_err() {
        return (Ok(None), false);
    }

    // Resolve the synthesised alias by dispatching through
    // `Instantiate { base, args: [], context: InstantiateContext {
    // projection_reduction, resolve_env_hash } }` in the requested mode
    // (`context.projection_reduction.mode`). The scratch alias has no
    // declaration-site type parameters so `args = []` is correct; the
    // dispatch lifts the `DeclPlaceholder` into a concrete body in the
    // requested mode.
    //
    // This is a query-RETURNER (it returns the resolved node, and on a
    // cacheable request it warms `scratch_cache`), so it MUST resolve
    // against a PROVEN-CURRENT snapshot — read AFTER the scratch upsert
    // above so the snapshot reflects the synthesised scratch content. A
    // known-stale (`ReturnOnly`) read would resolve against superseded
    // dependency state; on sustained churn surface a miss and drop the
    // scratch WITHOUT warming `scratch_cache` (a non-current execution
    // must never populate the cache). The bounded retry terminates.
    //
    // The scratch is now upserted but NOT yet in `scratch_cache` (the
    // cache is warmed only on the success path below), so THIS request
    // holds no LRU owner. Any miss between here and the cache write must
    // drop the orphan regardless of `req.cacheable` — the scratch never
    // entered the cache on this path, so nothing reclaims it otherwise.
    // `remove_scratch` is ownership-aware: it skips removal only if a
    // concurrent sibling request for the same content-addressed URI
    // already owns it in `scratch_cache`.
    let Some(current_view) = crate::typeinfo::current_store_view_for_query(host) else {
        // A non-current settle is a non-fault miss (`Ok(None)`); drop the
        // orphan (ownership-aware) without warming `scratch_cache`.
        remove_scratch(host, scratch_uri);
        return (Ok(None), false);
    };
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::from_current(host, &current_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let scratch_canonical: Arc<str> = Arc::from(scratch_uri);
    let Some(shallow) = host.shallow_file_state(scratch_uri) else {
        // Same orphan contract as the non-current branch above: this
        // request's scratch is upserted but unowned, so drop it
        // (ownership-aware; a non-fault miss rides `Ok(None)`).
        remove_scratch(host, scratch_uri);
        return (Ok(None), false);
    };
    let scope_node = crate::semantic_query::NodeScopeId::File {
        canonical_id: Arc::clone(&scratch_canonical),
        whole_hash: shallow.whole_hash,
        local_scope: None,
    };
    let _ = &scope_node;
    let base = dispatch.type_slot_for(
        Arc::clone(&scratch_canonical),
        Arc::from(SCRATCH_ALIAS_NAME),
    );
    let instantiate_key =
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            base,
            Arc::from(Vec::new().into_boxed_slice()),
            dispatch.instantiate_context_for(
                &scratch_canonical,
                crate::semantic_query::ProjectionReductionContext::published(req.mode),
            ),
        ));
    let resolved_alias_node = match dispatch.execute_type_node(instantiate_key) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(err) => {
            // A genuine dispatch fault is a request fault — surface it.
            // A non-fault miss falls back to the bare-decl path so the
            // caller sees a node id even when the body could not
            // materialise (the audit record still emits with the
            // chosen mode).
            if let Some(fault) = TypeResolutionRequestError::from_query_error(&err) {
                // Fault before the cache write: this request's upserted
                // scratch is unowned — drop it (ownership-aware).
                remove_scratch(host, scratch_uri);
                return (Err(fault), false);
            }
            let resolve_decl_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: ScopeId {
                    canonical_id: Arc::clone(&scratch_canonical),
                    local_scope: None,
                },
                name: Arc::from(SCRATCH_ALIAS_NAME),
            });
            match dispatch.execute_type_node(resolve_decl_key) {
                QueryResult::Value(SemanticQueryOutput { value: n, .. }) => n,
                QueryResult::Recursive(n) => n,
                QueryResult::Error(err) => {
                    // Resolution failed before the cache write, so this
                    // request's upserted scratch is unowned — drop it
                    // (ownership-aware).
                    remove_scratch(host, scratch_uri);
                    // A genuine dispatch fault rides `Err`; a non-fault
                    // miss rides `Ok(None)`.
                    return (
                        super::resolve_named_symbol::classify_dispatch_error(&err, None),
                        false,
                    );
                }
            }
        }
    };

    // Apply the requested terminal mode. `Identity` returns the
    // alias node verbatim. Everything else routes through the SINGLE
    // canonical materializer in `resolve_named_symbol` — the same helper
    // the named-resolve path uses — so an operator-bodied alias
    // (`type X = Y[K]` / `keyof Y`) reduces IDENTICALLY on this FFI path
    // and the named-resolve path (the one-resolver mandate). There is no
    // evaluate-local materialiser fork.
    let final_node = match req.mode {
        ProjectionMode::Identity => resolved_alias_node,
        _ => match super::resolve_named_symbol::materialize_through_aliases(
            host,
            &dispatch,
            resolved_alias_node,
            req.mode,
        ) {
            Ok(materialized) => materialized,
            // A hard dispatch fault during nested materialization
            // propagates as `Err` rather than silently degrading to the
            // un-materialised placeholder.
            Err(fault) => {
                // Fault before the cache write: this request's upserted
                // scratch is unowned — drop it (ownership-aware).
                remove_scratch(host, scratch_uri);
                return (Err(fault), false);
            }
        },
    };

    // Publish to cache if asked. The cached node id is the one we
    // return so a later cache hit and a fresh request produce the
    // same value.
    if req.cacheable {
        let mut guard = host.scratch_cache().lock();
        let evicted = guard.insert(scratch_uri.to_string(), final_node);
        drop(guard);
        if let Some(evicted_uri) = evicted {
            // The LRU dropped an older scratch entry — fully remove its
            // host file so memory does not grow unbounded. Uses the same
            // synthetic-file removal as the unowned paths (a scratch has
            // no workspace backing, so `evict`'s reload-pending semantics
            // are wrong here). The removal is ownership-aware: between
            // dropping the guard above and re-locking inside
            // `remove_scratch`, a concurrent request could cold-resolve
            // and re-insert the SAME evicted URI (it re-upserts its host
            // file first); `remove_scratch` re-checks ownership under the
            // lock and skips removal in that case, so the re-inserted
            // entry keeps backing a live file.
            remove_scratch(host, &evicted_uri);
        }
    } else {
        // Non-cacheable success: this request deliberately kept its
        // scratch out of `scratch_cache`, so it holds no LRU owner.
        // EVICT (not full-remove) on this path: a successful resolution
        // admitted scratch-rooted entries into the semantic-graph memo,
        // and the typeinfo mode contract pins cross-mode materialized-
        // point satisfaction over them — a repeat evaluate of the SAME
        // expression in another publication mode must warm-satisfy from
        // this resolution's recorded materializations (the
        // `operator_reduction` evaluate-parity tests pin it). A full
        // `host.remove` purges the scratch's semantic-graph state and
        // forces every repeat cold, breaking that pinned equivalence.
        // `evict` reclaims the compile/derived state while preserving
        // the memo; the miss/fault terminals below keep FULL removal
        // (a failed resolution admitted nothing worth preserving —
        // `cache_suppress` covers the memo side).
        // Ownership-aware like the removal paths: a concurrent CACHEABLE
        // request for the same content-addressed URI may own it in
        // `scratch_cache`; evicting under it would clear that owner's
        // live compile state.
        evict_scratch(host, scratch_uri);
    }

    (Ok(Some(final_node)), false)
}

/// Drop a scratch file iff no concurrent request owns it in
/// `scratch_cache`.
///
/// A scratch gains an LRU owner only when a cacheable request reaches
/// the `scratch_cache` write below. Every miss/failure terminal between
/// the upsert and that write, plus the non-cacheable success path that
/// deliberately bypasses the cache, leaves *this* request's scratch with
/// no cache entry to reclaim it — so it must be removed irrespective of
/// `req.cacheable`. Skipping removal there would accumulate orphaned
/// host/scheduler state.
///
/// The removal is nonetheless **ownership-aware**: the scratch URI is
/// content-addressed, so a *different* request for the same
/// `(scope, expression, extra_imports)` triple can be running
/// concurrently. If that sibling reached the success path it upserted the
/// same URI and inserted it into `scratch_cache` — it now OWNS a live
/// host file the cache fast-path will hand back. An unconditional
/// `host.remove` here would then delete a cache-owned scratch, leaving
/// the cache able to return a `SemanticNodeId` for a removed host file.
/// To prevent that, the presence check and the removal are made atomic
/// with respect to the success-path ownership insert by holding the
/// `scratch_cache` lock — the SAME lock `evaluate_inner`'s success path
/// takes to `insert`/own the URI — across "is `uri` present? if NOT,
/// `host.remove(uri)`". The two operations therefore serialise:
/// - if the sibling is taking ownership, cleanup sees the URI present
///   and SKIPS removal (the sibling upserts the host file *before* it
///   inserts, so a present cache entry always backs a live file);
/// - if cleanup removes first, the sibling's insert happens-after on an
///   absent URI. This ordering is NOT fully exclusive: the sibling
///   upserts its scratch BEFORE taking the lock to insert, so cleanup's
///   `host.remove` can land in the window between the sibling's upsert
///   and its cache insert — the sibling's entry then points at a removed
///   host file. The actual bound is a stale cache ENTRY, never wrong
///   served content. A later cache hit performs NO liveness check: the
///   fast-path in `evaluate_inner` hands the cached `SemanticNodeId`
///   straight back, and the semantic-graph node arena is append-only, so
///   the id still dereferences to its immutable resolved node after the
///   removal — the hit itself does NOT degrade to a miss. That is benign
///   for what is served: the entry's node is exactly what a live-backed
///   hit on the same content-addressed URI would return. What
///   `host.remove` does guarantee is that the removed FILE's state can
///   no longer be read as current: it drops the scratch's scheduler node
///   and compile caches, drains every resolver / memo entry scoped to
///   the canonical, and bumps the store-view epoch, so any path that
///   re-reads the scratch file (shallow state, file facts, read-set-
///   signature validation, cross-mode warm satisfaction) misses and
///   re-synthesises instead of consuming removed-file state. The
///   residual window is therefore an orphaned cache entry whose
///   file-needing consumers pay a recompute — until LRU eviction or a
///   fresh request re-upserts the file — not a stale-content hazard.
///
/// **Lock order (no deadlock):** `scratch_cache` is acquired here, then
/// `host.remove` runs while it is held. `host.remove` takes the alias /
/// workspace / scheduler / resolver / project-store locks and bumps the
/// store-view epoch, but none of those paths ever acquire the
/// `scratch_cache` lock (the only `scratch_cache` lock sites are this
/// module's fast-path get, success insert, and this cleanup). The lock
/// order `scratch_cache → {host.remove internals}` is therefore strictly
/// one-directional and cannot invert.
///
/// This uses `host.remove` (full deletion), NOT `host.evict`: a scratch
/// URI is synthetic and has no workspace backing, so `evict`'s
/// "invisible until `ensure_loaded` reloads from disk" semantics would
/// leave a zombie the next identical re-`upsert` cannot re-integrate
/// (the no-op-reload short-circuit skips re-integration when the
/// re-upserted content hashes identically to the evicted content).
/// `remove` drops the scheduler node and resolver caches outright, so a
/// later identical request re-synthesises the scratch cleanly.
/// Evict (not remove) a scratch file iff no concurrent request owns it
/// in `scratch_cache`. The non-cacheable SUCCESS terminal uses this:
/// eviction reclaims compile/derived state but PRESERVES the scratch's
/// semantic-graph memo, which the typeinfo cross-mode satisfaction
/// contract depends on (see the call site). Ownership-check rationale is
/// identical to [`remove_scratch`].
fn evict_scratch(host: &VerterHost, uri: &str) {
    let guard = host.scratch_cache().lock();
    if guard.contains(uri) {
        return;
    }
    host.evict(uri);
    drop(guard);
}

fn remove_scratch(host: &VerterHost, uri: &str) {
    #[cfg(test)]
    test_interleave::fire(uri);
    let guard = host.scratch_cache().lock();
    if guard.contains(uri) {
        // A concurrent request owns this URI — its cache entry backs a
        // live host file. Removing it now would strand that entry.
        return;
    }
    // Still unowned under the lock: the check-and-remove is atomic with
    // respect to a concurrent ownership insert (that insert serialises
    // behind this guard). It is NOT exclusive with the sibling's UPSERT,
    // which happens before the sibling takes this lock: a sibling that
    // upserted before this remove will insert an entry pointing at the
    // just-removed host file — the stale-entry-never-wrong-content bound
    // documented above.
    let _ = host.remove(uri);
    drop(guard);
}

/// Compute the scratch URI for the evaluate-type-expression
/// substrate.
///
/// Hash inputs: `scope_canonical || \0 || expression || \0 ||
/// serialize(extra_imports)`. The serialised form for imports is a
/// stable text encoding so two structurally identical
/// `Vec<ImportSpec>`s always hash the same — ordering of bindings
/// and imports is preserved verbatim.
pub(crate) fn compute_scratch_uri(
    scope: &str,
    expression: &str,
    extra_imports: &[ImportSpec],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update(b"\0");
    hasher.update(expression.as_bytes());
    hasher.update(b"\0");
    for imp in extra_imports {
        hasher.update(b"i:");
        hasher.update(imp.specifier.as_bytes());
        hasher.update(b"\0");
        for binding in &imp.bindings {
            match binding {
                NamedImport::Default { local_name } => {
                    hasher.update(b"d:");
                    hasher.update(local_name.as_bytes());
                }
                NamedImport::Named {
                    exported_name,
                    local_alias,
                    type_only,
                } => {
                    hasher.update(b"n:");
                    if *type_only {
                        hasher.update(b"t:");
                    }
                    hasher.update(exported_name.as_bytes());
                    if let Some(alias) = local_alias {
                        hasher.update(b"=");
                        hasher.update(alias.as_bytes());
                    }
                }
                NamedImport::Namespace { local_name } => {
                    hasher.update(b"s:");
                    hasher.update(local_name.as_bytes());
                }
            }
            hasher.update(b"\n");
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex_buf = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write;
        let _ = write!(&mut hex_buf, "{byte:02x}");
    }
    format!("{SCRATCH_URI_PREFIX}{hex_buf}.ts")
}

/// Synthesise the scratch TS source. Layout:
///
/// ```text
/// // optional scope eval-source prelude
/// import <imports>...;
/// type __VerterScratch = <expression>;
/// ```
///
/// `scope_eval_source`, when provided, is the textual eval-source
/// for `req.scope` — the same TS body the host parses to build that
/// scope's shallow inventory. Inlining it here makes the scratch's
/// name resolution truly "rooted at the scope" per the
/// `EvaluateTypeExpressionRequest::scope` contract: every top-level
/// declaration that exists in the scope (types, value bindings,
/// imports, the SFC-synthesised `default`) is visible to the
/// trailing `__VerterScratch` alias without forcing the caller to
/// enumerate them through `extra_imports`. Without this prelude,
/// expressions like `InstanceType<typeof default>['$props']`
/// evaluated against a `.vue` scope would have no `default` in
/// their lookup environment and never reduce.
///
/// Comments / blank lines are emitted at file head as needed so
/// downstream parsers see a clean source.
fn synthesise_source(
    expression: &str,
    extra_imports: &[ImportSpec],
    scope_eval_source: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by VerterHost::evaluate_type_expression\n");
    if let Some(prelude) = scope_eval_source {
        if !prelude.is_empty() {
            out.push_str("// --- begin scope eval-source prelude ---\n");
            out.push_str(prelude);
            if !prelude.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("// --- end scope eval-source prelude ---\n\n");
        }
    }
    for imp in extra_imports {
        push_import(&mut out, imp);
    }
    if !extra_imports.is_empty() {
        out.push('\n');
    }
    out.push_str("type ");
    out.push_str(SCRATCH_ALIAS_NAME);
    out.push_str(" = ");
    out.push_str(expression);
    out.push_str(";\n");
    out
}

/// Render one [`ImportSpec`] as a TypeScript import declaration.
///
/// Splits `Default` / `Namespace` from `Named` because TS allows at
/// most one default + at most one namespace + a single named-binding
/// list per import. Namespace imports are emitted on their own line
/// when mixed with named bindings; default imports merge with named
/// bindings on the same import declaration.
fn push_import(out: &mut String, imp: &ImportSpec) {
    let mut default_name: Option<&str> = None;
    let mut namespace_name: Option<&str> = None;
    let mut named: Vec<&NamedImport> = Vec::new();
    for b in &imp.bindings {
        match b {
            NamedImport::Default { local_name } => default_name = Some(local_name.as_str()),
            NamedImport::Namespace { local_name } => namespace_name = Some(local_name.as_str()),
            NamedImport::Named { .. } => named.push(b),
        }
    }
    // Namespace imports cannot mix with named/default — emit them
    // on a separate line.
    if let Some(ns) = namespace_name {
        out.push_str("import * as ");
        out.push_str(ns);
        out.push_str(" from \"");
        out.push_str(&imp.specifier);
        out.push_str("\";\n");
    }
    if default_name.is_some() || !named.is_empty() {
        out.push_str("import ");
        if let Some(default) = default_name {
            out.push_str(default);
            if !named.is_empty() {
                out.push_str(", ");
            }
        }
        if !named.is_empty() {
            out.push_str("{ ");
            for (idx, b) in named.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                if let NamedImport::Named {
                    exported_name,
                    local_alias,
                    type_only,
                } = b
                {
                    if *type_only {
                        out.push_str("type ");
                    }
                    out.push_str(exported_name);
                    if let Some(alias) = local_alias {
                        out.push_str(" as ");
                        out.push_str(alias);
                    }
                }
            }
            out.push_str(" }");
        }
        out.push_str(" from \"");
        out.push_str(&imp.specifier);
        out.push_str("\";\n");
    }
}

/// Deterministic interleaving hook for the cleanup ownership-race test.
///
/// Fires at the very top of [`remove_scratch`], before the
/// `scratch_cache` lock is taken, so a test can rendezvous a cleaning
/// request with a concurrent owning request at the exact race window. The
/// installed closure receives the scratch URI under cleanup; the test
/// gates on its own URI so unrelated `remove_scratch` calls (LRU
/// eviction, the owner's own paths, sibling tests) do not block. No-op
/// when no closure is installed, and compiled out entirely in non-test
/// builds.
#[cfg(test)]
pub(crate) mod test_interleave {
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn(&str) + Send + Sync>;

    static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

    /// Install the cleanup-window rendezvous closure.
    pub(crate) fn install(hook: impl Fn(&str) + Send + Sync + 'static) {
        *HOOK.lock().expect("test interleave lock poisoned") = Some(Arc::new(hook));
    }

    /// Remove any installed closure.
    pub(crate) fn clear() {
        *HOOK.lock().expect("test interleave lock poisoned") = None;
    }

    /// Invoke the installed closure (if any) for `uri`. Called from
    /// [`super::remove_scratch`] before the `scratch_cache` lock is taken.
    pub(crate) fn fire(uri: &str) {
        // Clone the `Arc` out from under the lock so the rendezvous — which
        // may block the calling thread — runs WITHOUT holding the static
        // `HOOK` lock. Holding it across a blocking closure would deadlock a
        // concurrent `install`/`clear` from the test driver.
        let hook = HOOK
            .lock()
            .expect("test interleave lock poisoned")
            .as_ref()
            .map(Arc::clone);
        if let Some(hook) = hook {
            hook(uri);
        }
    }
}
