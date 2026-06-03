#![deny(missing_docs)]
//! `VerterHost::analyze_with_audit` — public audited entry-point for
//! `RequestKind::SemanticAnalysis` requests.
//!
//! Drives the host's existing semantic analysis path
//! ([`VerterHost::ensure_indexed_ready`]) inside the same
//! audit-registration / TLS-observer machinery the component-meta and
//! type-resolution producers use. Returns the materialised
//! [`AnalysisReady`] artifact (when the canonical exists in the
//! workspace) plus the per-request
//! [`verter_audit::RequestAuditRecord`].
//!
//! Boundary contract:
//!
//! 1. Construct an [`AuditRequestRegistration`] with
//!    `RequestKind::SemanticAnalysis` BEFORE installing any TLS guard.
//! 2. Branch on the registration's `Active` / `Noop` arm to install
//!    the matching observer:
//!    - `Active` → real [`RequestContextGuard`].
//!    - `Noop` → [`verter_audit::install_noop_observer`].
//! 3. Detect "fresh build vs warm-cache reuse" with a content-pinned
//!    probe ([`VerterHost::current_content_pinned_indexed`]) BEFORE
//!    invoking [`VerterHost::ensure_indexed_ready`]. The probe is
//!    cheap and provides a discriminating signal independent of the
//!    producer's internal state machine — a regression that always
//!    rebuilt would still surface a real `indexed_ready_built = true`
//!    here. The pin matters: a permissive `get_any` probe would match
//!    a stale lingering artifact and misreport the request as
//!    cache-served.
//! 4. Materialise the [`AnalysisReady`] from the canonical's cached
//!    `IndexedReady` artifact + `FileAnalysisSnapshot`. The numeric
//!    payload counters are sourced from this snapshot so the audit
//!    metric describes the file's real semantic footprint.
//! 5. Build the [`verter_audit::RequestAuditRecord`] with
//!    [`verter_audit::RequestKindPayload::SemanticAnalysis`].
//! 6. Finalise through the registration and return an
//!    [`verter_audit::AuditedResult<Option<AnalysisReady>, Infallible>`]
//!    carrier. A resolved canonical rides `Ok(Some(analysis))`; a
//!    missing canonical rides `Ok(None)`. The carrier's `audit` field
//!    is mandatory — filtered / disabled paths return the cheap
//!    default-filled record marked
//!    [`verter_audit::AuditCaptureState::FilteredNoop`] /
//!    [`verter_audit::AuditCaptureState::AuditDisabled`].

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use std::convert::Infallible;

use verter_audit::{
    AuditedResult, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit, SemanticAnalysisPayload, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::project_type_store::{AnalysisArtifactKey, AnalysisReady};
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::types::FileAnalysisSnapshot;
use crate::VerterHost;

impl VerterHost {
    /// Run a semantic-analysis request through the host's shared
    /// `IndexedReady` materialisation path and return the resulting
    /// [`AnalysisReady`] alongside the per-request
    /// [`RequestAuditRecord`], packaged in a single
    /// [`crate::AuditedResult`] carrier.
    ///
    /// The error type is [`Infallible`]: semantic analysis has no
    /// request-fault path — a missing canonical rides the success arm
    /// as `Ok(None)`.
    ///
    /// Outcome mapping:
    /// - `Ok(Some(analysis))` — the canonical resolved through the
    ///   workspace and analysis materialised.
    /// - `Ok(None)` — the canonical does not exist in the workspace.
    ///
    /// The carrier's `audit` field is always populated:
    /// [`verter_audit::AuditCaptureState::ActiveStored`] on the
    /// full-capture path, or the cheap default-filled record marked
    /// [`verter_audit::AuditCaptureState::FilteredNoop`] /
    /// [`verter_audit::AuditCaptureState::AuditDisabled`] on the
    /// filtered / disabled paths.
    #[must_use]
    pub fn analyze_with_audit(
        self: &Arc<Self>,
        canonical_id: &str,
    ) -> AuditedResult<Option<AnalysisReady>, Infallible> {
        // Probe the IndexedReady cache BEFORE constructing the
        // registration so the cache state we observe is unaffected by
        // any work we are about to perform. The probe is
        // content-pinned (`current_content_pinned_indexed`): a `Some`
        // here means a *current-content* warm artifact exists and the
        // request will be served by warm state. A permissive `get_any`
        // probe would also match a STALE lingering artifact for an
        // older content hash once the own-canonical drain is retired —
        // reporting `from_cache: true` (and `indexed_ready_built: false`)
        // even though `materialize_analysis_ready` below rematerialises
        // the current content. The content pin keeps the audit
        // `from_cache` / `indexed_ready_built` flags faithful to what
        // the request actually did.
        let pre_call_cache_hit = self.current_content_pinned_indexed(canonical_id).is_some();

        // Audit-disabled fast path: drive the analysis with NO
        // `RequestContextGuard` installed. Producer-side
        // `current_observer()` returns `None`, the instrumentation
        // short-circuits at the TLS check, and nothing is published.
        // The carrier still carries a cheap default-filled record
        // marked `AuditDisabled`.
        if !self.config.audit_enabled {
            let request_id = self.next_request_id();
            let analysis = self.materialize_analysis_ready(canonical_id);
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            let record = noop_semantic_analysis_record(
                request_id,
                canonical_id,
                parent_request_id,
                pre_call_cache_hit,
                verter_audit::AuditCaptureState::AuditDisabled,
            );
            return AuditedResult::ok(analysis, record);
        }

        // Stamp request id and increment the created-counter so the
        // `AuditedRequest` harness's multi-request guard surfaces
        // correctly when a closure issues both a component-meta and
        // analyze call inside the same `run` window.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // Build the per-request context. Footprint capture follows
        // the host config; semantic-analysis requests do NOT install
        // a footprint accumulator because the analysis path does not
        // emit footprint events the way the cold component-meta
        // resolver does.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::SemanticAnalysis,
            footprint_capture,
            timing_capture,
            None,
        );

        // BEFORE installing the TLS guard: construct the registration.
        // The `Noop` arm short-circuits when the consumer filter
        // rejects `SemanticAnalysis`; the `Active` arm captures a slot
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
        let request_start = Instant::now();
        let analysis = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                self.materialize_analysis_ready(canonical_id)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                self.materialize_analysis_ready(canonical_id)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // Filtered kinds: return the cheap default-filled record. The
        // analysis still ran (consumers asked for it), but no payload
        // is collected and nothing is published.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let record = noop_semantic_analysis_record(
                request_id,
                canonical_id,
                ctx.parent_request_id.map(|id| id.to_string()),
                pre_call_cache_hit,
                verter_audit::AuditCaptureState::FilteredNoop,
            );
            return AuditedResult::ok(analysis, record);
        }

        // Missing canonical on an active registration: there is no
        // analysis behind the request, but the carrier still returns a
        // populated record so the always-a-record contract holds. The
        // payload is the zero-valued default; the registration is
        // finalised so the active-request slot is released cleanly
        // rather than swept by the defensive `Drop`.
        let Some(analysis) = analysis else {
            let record = RequestAuditRecord {
                from_cache: pre_call_cache_hit,
                ..noop_semantic_analysis_record(
                    request_id,
                    canonical_id,
                    ctx.parent_request_id.map(|id| id.to_string()),
                    pre_call_cache_hit,
                    verter_audit::AuditCaptureState::ActiveStored,
                )
            };
            registration.finalize(record.clone());
            return AuditedResult::ok(None, record);
        };

        // Build the audit payload from the materialised AnalysisReady.
        // The numeric counters describe the file's actual semantic
        // footprint — they come from the same snapshot consumers see.
        let payload = SemanticAnalysisPayload {
            indexed_ready_built: !pre_call_cache_hit,
            ..build_payload_from_analysis(&analysis)
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
            kind: RequestKind::SemanticAnalysis,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            // Envelope `from_cache` mirrors the IndexedReady probe
            // result — when the cache already had a satisfying entry
            // BEFORE this request started, the request was served
            // entirely by warm state.
            from_cache: pre_call_cache_hit,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::SemanticAnalysis(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        AuditedResult::ok(Some(analysis), cloned)
    }

    /// Materialise an [`AnalysisReady`] for `canonical_id` by routing
    /// through the shared semantic analysis paths. Returns `None`
    /// when the canonical does not exist in the workspace.
    ///
    /// Drives both [`Self::ensure_indexed_ready`] (for the canonical
    /// `whole_hash` plus the cached `script_analysis` /
    /// `export_signatures` arcs) and [`Self::get_analysis`] (for the
    /// fully-finalised [`FileAnalysisSnapshot`], which includes
    /// template analysis when the host's effective scope requests
    /// it). The artifact is shaped so the returned
    /// `AnalysisReady`'s `(canonical, whole_hash, scope)` triple
    /// matches what [`crate::project_type_store::AnalysisReadyDb`]
    /// would key for the same caller. The cache itself is
    /// intentionally not populated here — the wider host already
    /// populates `AnalysisReadyDb` when a consumer routes through
    /// the scope-aware `find_satisfying` path; this entry-point's
    /// responsibility is to expose the ready artifact for the
    /// audited window, not to compete with that population path.
    fn materialize_analysis_ready(&self, canonical_id: &str) -> Option<AnalysisReady> {
        let indexed = self.ensure_indexed_ready(canonical_id)?;
        let snapshot = self.get_analysis(canonical_id)?;
        let scope = self.config.effective_scope();
        let _key = AnalysisArtifactKey {
            canonical_id: Arc::<str>::from(canonical_id),
            whole_hash: indexed.whole_hash,
            scope,
        };
        Some(AnalysisReady {
            whole_hash: indexed.whole_hash,
            scope,
            script_analysis: indexed.script_analysis.clone(),
            export_signatures: indexed.export_signatures.clone(),
            snapshot: Arc::new(snapshot),
        })
    }
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled / missing-canonical semantic-analysis path.
/// No per-request counters are collected — the payload is the
/// zero-valued default and `capture_state` records why the full path
/// was skipped.
fn noop_semantic_analysis_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<String>,
    from_cache: bool,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::SemanticAnalysis,
        parent_request_id,
        from_cache,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::SemanticAnalysis(SemanticAnalysisPayload::default()),
        capture_state,
        trace_id: String::new(),
    }
}

/// Build a [`SemanticAnalysisPayload`] from the materialised
/// [`AnalysisReady`] artifact. The `indexed_ready_built` flag is left
/// at its default (`false`) — the caller decides that based on
/// pre-call cache state.
fn build_payload_from_analysis(analysis: &AnalysisReady) -> SemanticAnalysisPayload {
    let snapshot: &FileAnalysisSnapshot = &analysis.snapshot;
    let num_imports = u32_from_usize_clamped(snapshot.imports.len());
    let num_exports = u32_from_usize_clamped(snapshot.export_signatures.len());

    // Type / value declaration split. The shallow processor produces
    // `declaration_entries` on `ScriptAnalysisSnapshot` already
    // tagged with `LocalDeclarationKind::{Type, Value, TypeAndValue}`;
    // the audit count is the per-kind tally over that list.
    // `TypeAndValue` (e.g. a class declaration) counts toward both
    // type and value sides because the symbol occupies both
    // namespaces. The script_analysis arc may be absent for non-SFC
    // files where shallow processing populated the snapshot through a
    // different path; in that case the counts stay at 0.
    let mut num_type_decls: u32 = 0;
    let mut num_value_decls: u32 = 0;
    if let Some(script) = analysis.script_analysis.as_ref() {
        for entry in &script.declaration_entries {
            match entry.kind {
                verter_semantic::analysis::LocalDeclarationKind::Type => {
                    num_type_decls = num_type_decls.saturating_add(1);
                }
                verter_semantic::analysis::LocalDeclarationKind::Value => {
                    num_value_decls = num_value_decls.saturating_add(1);
                }
                verter_semantic::analysis::LocalDeclarationKind::TypeAndValue => {
                    num_type_decls = num_type_decls.saturating_add(1);
                    num_value_decls = num_value_decls.saturating_add(1);
                }
            }
        }
    }

    let num_macro_calls = u32_from_usize_clamped(snapshot.macros.len());

    // Root-reachability edges: the count of root-level template
    // elements (those whose `parent_index` is `None`). This is the
    // production-path metric `extract_root_reachability` consumes
    // when computing fallthrough surfaces — surfacing it here keeps
    // the audit count aligned with the resolver's own facts.
    let num_root_reachability_edges = snapshot
        .template
        .as_ref()
        .map(|tpl| {
            u32_from_usize_clamped(
                tpl.elements
                    .iter()
                    .filter(|el| el.parent_index.is_none())
                    .count(),
            )
        })
        .unwrap_or(0);

    SemanticAnalysisPayload {
        num_imports,
        num_exports,
        num_type_decls,
        num_value_decls,
        num_macro_calls,
        num_root_reachability_edges,
        indexed_ready_built: false,
    }
}

#[inline]
fn u32_from_usize_clamped(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
