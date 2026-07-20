//! `host_manage::component_meta_entry_resolution` — the
//! resolution-bearing component-meta entry points.
//!
//! Domain H continuation of
//! [`component_meta_entry`](super::component_meta_entry): holds the
//! `get_component_meta_with_resolution` /
//! `get_component_meta_with_resolution_via_view` public entry points
//! and their warm-cache hit path (`try_with_resolution_cache_hit`).
//! The publish fence, admission, and cache-entry helpers they share
//! with the plain `get_component_meta` lane stay in
//! `component_meta_entry` (`ColdSeedFence`,
//! `publish_if_admissible`, `publish_component_meta_cache_entry`).
//! Public surface remains rooted at `crate::host_manage::*`; this file
//! contributes a continuation `impl VerterHost { … }` block.

use crate::VerterHost;

use super::{extract_component_meta_from_resolved, ComponentMetaOptions};

/// RAII bundle for the audited component-meta request scope: the TLS
/// request-context guard plus the per-request VFS audit-sink handle. The
/// sink holds a `Weak` to the accumulator, so once the context guard (and
/// its accumulator `Arc`) drops, late fan-out events no-op — matching the
/// inline preamble this helper was extracted from.
struct ComponentMetaAuditScope {
    _sink_handle: Option<verter_workspace::audit_sink::SinkHandle>,
    _ctx_guard: crate::request_context::RequestContextGuard,
}

impl VerterHost {
    /// Install the audited component-meta request scope shared by the
    /// resolution-bearing entries: the `RequestContext` (kind
    /// `ComponentMeta`, projection budget, optional footprint accumulator),
    /// the `AuditRequestRegistration` planted on it, and the per-request
    /// `SessionVfsSink`. Returns the RAII bundle the caller holds for the
    /// duration of the request.
    fn install_component_meta_audit_scope(
        &self,
        canonical: &str,
        request_id: u64,
    ) -> ComponentMetaAuditScope {
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let accumulator = if footprint_capture {
            // Wire `HostConfig::audit_caps` through to the accumulator so
            // per-host cap overrides take effect on every raw push lane.
            Some(std::sync::Arc::new(
                crate::component_meta_audit::RequestFootprintAccumulator::with_caps(
                    self.config.audit_caps.clone(),
                ),
            ))
        } else {
            None
        };
        let ctx = crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
            request_id,
            std::sync::Arc::<str>::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            footprint_capture,
            self.config.audit_timing_capture && self.config.audit_enabled,
            accumulator.clone(),
            self.config.projection_op_budget,
        );

        // Construct the audit registration BEFORE installing the TLS
        // guard. The `Active` arm enters the host's active-request
        // registry; the `Noop` arm is returned when the consumer filter
        // rejects the kind. Plant the registration on the request context
        // so the inner resolver path finalises through it.
        let registration =
            std::sync::Arc::new(crate::host_audit_runtime::AuditRequestRegistration::new(
                self,
                std::sync::Arc::clone(&ctx),
            ));
        debug_assert!(
            ctx.audit_registration.get().is_none(),
            "freshly-constructed RequestContext must have no audit_registration",
        );
        let _ = ctx.install_audit_registration(std::sync::Arc::clone(&registration));

        let ctx_guard = crate::request_context::RequestContextGuard::install(ctx);
        let sink_registration = accumulator.as_ref().and_then(|acc| {
            let sink = crate::component_meta_audit::session_vfs_sink::SessionVfsSink::new(
                request_id,
                std::sync::Arc::clone(acc),
            );
            self.workspace().register_audit_sink(sink).ok()
        });
        ComponentMetaAuditScope {
            _sink_handle: sink_registration,
            _ctx_guard: ctx_guard,
        }
    }
    /// Combined query: resolves component-meta once and returns both the
    /// analysis projection and the resolved-meta sidecar. Avoids the
    /// double `resolve_component_meta(Expanded)` that happens if callers
    /// invoke `get_component_meta()` + `resolve_component_meta()` separately.
    ///
    /// **Audit lifecycle.** Constructs an
    /// [`crate::host_audit_runtime::AuditRequestRegistration`] before
    /// the per-request TLS guard installs. The `Active` arm captures a
    /// slot in [`crate::host_audit_runtime::HostAuditRuntime`]'s
    /// active-request map; the `Noop` arm is returned when the
    /// configured consumer filter rejects the request's kind, in which
    /// case no audit record will be produced. Either way the
    /// substrate's `current_observer()` TLS slot stays populated for
    /// the duration of the request.
    ///
    /// **Warm-cache fast path.** Consults the `ComponentMetaResultDb`
    /// warm cache before falling through to the cold resolver. On a
    /// cache hit with a valid `dep_signature`, the cached
    /// `ResolutionTemplate` rehydrates a per-request
    /// `ResolvedComponentMetaState` (snapshot reloaded from
    /// `FileArtifactStore`) and a synthesized `RequestAuditRecord` with
    /// `from_cache = true`, `total_ms = 0.0` is finalised through the
    /// registration so audit consumers via
    /// `take_audit_record(resolution.request_id)` work uniformly.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Stamp a request id for this call. The `AuditedRequest`
        // harness tracks this via `REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN`
        // so multi-request closures inside `run_custom` can be rejected.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Install the audited request scope (RequestContext + audit
        // registration + per-request SessionVfsSink) — shared with the
        // output-bearing resolution entry.
        let _audit_scope = self.install_component_meta_audit_scope(canonical.as_str(), request_id);

        // Warm-cache short-circuit AFTER request-context
        // install (so `current_request_id()` returns the fresh id even
        // on the warm path). Validates `dep_signature` against current
        // host state; on success, rehydrates the resolution template
        // and synthesizes a `from_cache: true` audit record.
        if let Some((analysis, resolution)) =
            self.try_with_resolution_cache_hit(canonical.as_str(), request_id)
        {
            return Some((analysis, resolution));
        }

        // Cold compute under a `with_fact_tracer` outer scope so the
        // resolver's `observe` calls accumulate into a real
        // `FactReadSet`. The finalised signature becomes the
        // candidate's `fact_dep_signature` at publish time. The
        // tracer covers BOTH the pinned resolve and
        // `extract_component_meta_from_resolved` so cross-file
        // observations from the extractor are captured. R24: tracer
        // installs on cold-path only; the warm-hit short-circuit
        // above returns before this block runs.
        //
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `get_component_meta` for the
        // race rationale. The published entry stamps this snapshot;
        // `ComponentMetaResultDb::get_with_view` rejects on warm read
        // when the live generation differs.
        let validated_at_generation = self.project_type_store.current_project_generation();
        // ONE captured view serves the publish fence, the extraction
        // context, AND the pinned resolve executor — the resolve can never
        // open a second unrelated store view and pair a fresh-analysis
        // result with this capture's extraction/materialization context.
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        let seed_fence = super::component_meta_entry::ColdSeedFence::new(
            fixed.captured_validation_token(),
            fixed.is_current(),
        );
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            overlay,
        );
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        self.component_meta_with_resolution_cold(
            canonical.as_str(),
            request_id,
            &view,
            &fixed,
            host_ctx_ref,
            &seed_fence,
            validated_at_generation,
        )
    }

    /// The SHARED audited cold body: resolve PINNED to the caller's captured
    /// fixed view, extract under the caller's request-bound `ctx`, publish
    /// the analysis result to the shared cache under the caller's fence, and
    /// return both halves — so an output-bearing caller can materialize the
    /// envelope under the SAME still-alive `ctx` (the analysis-cache publish
    /// is INDEPENDENT of any later output-materialization outcome).
    ///
    /// View fence: `view` / `fixed` / `ctx` / `seed_fence` all derive from
    /// the caller's ONE captured [`crate::resolver_store::BatchFixedView`],
    /// and the resolve executor pins to that same capture
    /// (`resolve_component_meta_with_view_and_fixed`) — it never opens its
    /// own store-view read, so a concurrent mutation landing after the
    /// capture cannot pair a fresh-view analysis with the capture-bound
    /// extraction/materialization context (the torn-result race).
    #[allow(clippy::too_many_arguments)]
    fn component_meta_with_resolution_cold(
        &self,
        canonical: &str,
        request_id: u64,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
        host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext,
        seed_fence: &super::component_meta_entry::ColdSeedFence,
        validated_at_generation: u64,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        #[cfg(test)]
        super::component_meta_entry::run_cold_body_pre_resolve_hook();
        let canonical = canonical.to_string();
        let (executor_view, executor_fp) = fixed.executor_fixed_view();
        let executor_fixed = Some((executor_view, executor_fp, fixed.is_current()));
        let results = self.project_type_store().component_meta_results();
        let maybe_resolved_analysis = results.compute_and_admit(
            self,
            canonical.as_str(),
            "with-resolution path",
            || {
                let (mut resolved, admission) = match self
                    .resolve_component_meta_with_view_and_fixed_admission(
                        canonical.as_str(),
                        crate::types::ProjectionMode::Expanded,
                        view,
                        executor_fixed,
                    ) {
                    Some(pair) => pair,
                    None => return None,
                };
                resolved.request_id = request_id;
                // Open the publication-boundary tracing span. Carries the
                // per-request `trace_id` (from `RequestContext`) so audit
                // consumers can join `RequestAuditRecord.trace_id` to
                // captured tracing logs by string match. The
                // `suppress` field surfaces the synthesis suppression
                // decision in spans for the same reason.
                let publish_trace_id = crate::request_context::current_request_context()
                    .map(|ctx| ctx.trace_id.clone())
                    .unwrap_or_default();
                let publish_span = tracing::info_span!(
                    "publish_component_meta",
                    file = %canonical,
                    trace_id = %publish_trace_id,
                    suppress = resolved.synthesis_should_suppress,
                );
                let _publish_enter = publish_span.enter();
                tracing::info!(
                    trace_id = %publish_trace_id,
                    suppress = resolved.synthesis_should_suppress,
                    "publish_component_meta",
                );
                // Always include fallthrough — the solver path does not use walker
                // overflow as a gating signal.
                let extract = extract_component_meta_from_resolved(
                    self,
                    canonical.as_str(),
                    &resolved,
                    true, // include_fallthrough
                    host_ctx_ref,
                );
                self.merge_extraction_facts_into_admitted_resolved_meta(
                    canonical.as_str(),
                    crate::types::ProjectionMode::Expanded,
                    view.fingerprint(),
                    &mut resolved,
                    extract.fallthrough_fact_versions.as_deref(),
                    admission.as_ref(),
                );
                Some((extract.analysis, resolved, extract.completeness))
            },
            |computed| {
                let Some((analysis, resolved, extract_completeness)) = computed else {
                    return crate::component_meta_result_db::ComponentMetaPublishDecision::no_value(
                    );
                };
                let final_completeness = resolved.completeness.merge(*extract_completeness);
                self.component_meta_publish_decision(
                    canonical.as_str(),
                    resolved,
                    analysis.clone(),
                    validated_at_generation,
                    seed_fence,
                    final_completeness,
                )
            },
        );
        let (analysis, resolved, _extract_completeness) = maybe_resolved_analysis?;
        // ONE merged admission signal: the resolve-phase completeness merged
        // with the whole-extract scope (macro-DTO read + fallthrough compute).

        // Seal + admission decision — `publish_if_admissible` (by-value
        // fenced-serve consult + R20 finalise). An admitted write lets
        // subsequent identical calls short-circuit through
        // `try_with_resolution_cache_hit`; suppression is enforced inside
        // `publish_component_meta_cache_entry` via the merged
        // `final_completeness` signal.
        Some((analysis, resolved))
    }

    /// Output-bearing AUDITED resolution entry: the wire consumers' (NAPI /
    /// WASM audit bundles, LSP custom method) counterpart of
    /// [`Self::get_component_meta_with_resolution`], producing the
    /// session-owned [`crate::meta_resolve::ComponentMetaOutput`] envelope
    /// with the narrowed resolution sidecar and ALL 11 materialized wire
    /// type lanes.
    ///
    /// Same audit lifecycle as the locator-based entry (request id, audit
    /// registration, per-request VFS sink; warm hits synthesize a
    /// `from_cache` record). EVERY terminal — success, the non-resolving
    /// `None`, and the typed output-materialization error — carries the
    /// stamped request id, so audit consumers retrieve the matching record
    /// via [`VerterHost::take_audit_record`] regardless of outcome: the
    /// resolution publishes its REAL record when the audit scope drops, and
    /// an error terminal that dropped the id would orphan that record while
    /// the consumer fabricated a zero-id stand-in.
    ///
    /// View fence: BOTH arms derive the warm-validation view and the
    /// materialization cold-seed from ONE `StoreViewRead`, so the analysis
    /// served and the view the output materializes under cannot describe
    /// different snapshots. Cache rails: the cold arm publishes the
    /// analysis cache entry BEFORE materialization (an output failure never
    /// suppresses it), and the output materializes in its OWN fact-tracer
    /// scope (its dependencies never fold into the analysis signature).
    pub fn get_component_meta_output_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        (Option<crate::meta_resolve::ComponentMetaOutput>, u64),
        (crate::meta_resolve::ComponentMetaOutputError, u64),
    > {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let _audit_scope = self.install_component_meta_audit_scope(canonical.as_str(), request_id);

        // ONE captured view serves the warm probe, the materialization /
        // cold-compute context, AND the pinned resolve executor.
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);

        // Warm probe against the capture's proven-current arm.
        if let Some(current_view) = fixed.current_view() {
            if let Some((analysis, resolution)) = self.try_with_resolution_cache_hit_in_view(
                canonical.as_str(),
                request_id,
                current_view,
            ) {
                let overlay =
                    std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
                    self,
                    fixed.cold_seed(),
                    overlay,
                );
                let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
                let seed =
                    crate::meta_resolve::output::ComponentMetaResolutionSeed::from_resolved_state(
                        &resolution,
                    );
                let (output, _output_read_set) = self.with_fact_tracer(|| {
                    crate::meta_resolve::projectors::build_component_meta_output(
                        ctx,
                        canonical.as_str(),
                        analysis,
                        Some(seed),
                    )
                });
                return output
                    .map(|output| (Some(output), request_id))
                    .map_err(|err| (err, request_id));
            }
        } else {
            self.project_type_store
                .component_meta_results()
                .record_non_current_view_miss(self);
        }

        // Cold: seed + fence from the SAME capture, shared audited cold body
        // (resolve PINNED to the capture; publishes the analysis
        // independently of the output result), then materialize under the
        // same still-alive ctx in a separate tracer scope.
        let validated_at_generation = self.project_type_store.current_project_generation();
        let seed_fence = super::component_meta_entry::ColdSeedFence::new(
            fixed.captured_validation_token(),
            fixed.is_current(),
        );
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            overlay,
        );
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let Some((analysis, resolved)) = self.component_meta_with_resolution_cold(
            canonical.as_str(),
            request_id,
            &view,
            &fixed,
            host_ctx_ref,
            &seed_fence,
            validated_at_generation,
        ) else {
            return Ok((None, request_id));
        };
        let seed = crate::meta_resolve::output::ComponentMetaResolutionSeed::from_resolved_state(
            &resolved,
        );
        let (output, _output_read_set) = self.with_fact_tracer(|| {
            crate::meta_resolve::projectors::build_component_meta_output(
                host_ctx_ref,
                canonical.as_str(),
                analysis,
                Some(seed),
            )
        });
        output
            .map(|output| (Some(output), request_id))
            .map_err(|err| (err, request_id))
    }

    /// View-aware variant of [`Self::get_component_meta_with_resolution`].
    ///
    /// R17 / R18 — Consults the supplied [`SessionView`] for tombstone
    /// detection and overlay-priority source. When the view carries
    /// an overlay for the owner canonical, the overlay's
    /// [`IndexedReady`](crate::project_type_store::IndexedReady) is
    /// pre-warmed into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
    /// via [`crate::resolver_core::SessionResolverContext`] so the
    /// cold compute reads from the overlay candidate.
    /// [`Self::resolve_component_meta_with_view`] threads the view
    /// fingerprint into the singleflight cache key so two sessions
    /// with different overlays admit distinct candidate slots.
    pub fn get_component_meta_with_resolution_via_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        if view.is_tombstoned(canonical.as_str()) {
            return None;
        }

        // Overlay-priority pre-warm for owner + every dep the view
        // carries an overlay for.
        {
            crate::host_manage::overlay_priority::prewarm_view_overlays(self, view);
        }

        // Arm the projection-budget fuse + the per-cold-compute completeness
        // rail across the FULL cold body (resolve AND the fallthrough
        // extract). Without this the session with-resolution view path ran
        // the fallthrough extract context-free — the inner resolve's
        // install-if-none dropped before the extract, so the op-budget fuse
        // was inert here. Install-if-none, so an outer context (the audited
        // `get_component_meta_with_resolution` entry) keeps its own.
        let _session_budget_ctx_guard = self.install_request_budget_context_if_none(
            crate::meta_resolve::next_component_meta_audit_request_id(),
            canonical.as_str(),
            self.config.audit_timing_capture && self.config.audit_enabled,
        );

        // ONE captured view (taken AFTER the overlay pre-warm so it observes
        // the pre-warmed overlay candidates) serves the pinned resolve
        // executor AND the extraction context — the resolve can never open a
        // second unrelated store view and pair a fresh-view analysis with
        // this capture's extraction context (the torn-result race).
        let fixed = self.capture_batch_fixed_view(view);
        let (mut resolved, admission) = {
            let (executor_view, executor_fp) = fixed.executor_fixed_view();
            self.resolve_component_meta_with_view_and_fixed_admission(
                canonical.as_str(),
                crate::types::ProjectionMode::Expanded,
                view,
                Some((executor_view, executor_fp, fixed.is_current())),
            )?
        };
        resolved.request_id = self.next_request_id();
        // Build a HostResolverContext before extract so engine
        // constructions inside the policy / fallthrough path bind to the
        // request-bound ctx rather than a bare-host. This is a post-fence
        // extraction binder — the pinned resolve already ran under its own
        // publish fence — and it seeds from the SAME capture's cold-seed: a
        // non-current capture fails the ctx's nested warm-cache probes
        // closed rather than validating against a stale snapshot.
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            overlay,
        );
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        // This view path does NOT publish to `ComponentMetaResultDb`, so the
        // extract completeness carrier is discarded here.
        let extract = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true,
            host_ctx_ref,
        );
        self.merge_extraction_facts_into_admitted_resolved_meta(
            canonical.as_str(),
            crate::types::ProjectionMode::Expanded,
            view.fingerprint(),
            &mut resolved,
            extract.fallthrough_fact_versions.as_deref(),
            admission.as_ref(),
        );
        Some((extract.analysis, resolved))
    }

    /// Cache-hit path. Returns `Some((analysis, resolution))` on a
    /// valid warm hit; `None` otherwise (miss, stale `dep_signature`,
    /// or eviction-race rehydrate failure). Caller falls through to
    /// the cold resolver on `None`.
    ///
    /// Synthesizes a `RequestAuditRecord` with `from_cache = true` and
    /// `total_ms = 0.0` and finalises it through the
    /// `AuditRequestRegistration` planted on the active
    /// `RequestContext` so audit consumers via
    /// `take_audit_record(resolution.request_id)` returns it
    /// uniformly with cold-resolver records.
    fn try_with_resolution_cache_hit(
        &self,
        canonical: &str,
        request_id: u64,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        // Fact-precise validation is the sole cache oracle. Accepts ONLY a
        // `CurrentHostStoreView`: a known-stale `ReturnOnly` snapshot
        // misses to the cold recompute path rather than validating an
        // entry against an already-superseded view.
        let Some(current_view) = self.resolver_store_view_read().current() else {
            self.project_type_store
                .component_meta_results()
                .record_non_current_view_miss(self);
            return None;
        };
        self.try_with_resolution_cache_hit_in_view(canonical, request_id, &current_view)
    }

    /// Warm-probe core validating against a CALLER-PROVIDED proven-current
    /// view — the output-bearing resolution entry derives this view and its
    /// materialization cold-seed from ONE `StoreViewRead`, so the analysis
    /// a warm hit serves and the view the output materializes under cannot
    /// describe different snapshots.
    fn try_with_resolution_cache_hit_in_view(
        &self,
        canonical: &str,
        request_id: u64,
        current_view: &crate::resolver_store::CurrentHostStoreView,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        let shallow = self.shallow_file_state(canonical)?;
        let owner_whole_hash = shallow.whole_hash;
        let key = self.component_meta_result_key(canonical, &ComponentMetaOptions::default());
        // Fact-precise validation is the sole cache oracle:
        // `ComponentMetaResultDb::get_with_view` validates the entry's
        // `read_set_signature.facts` against the resolver-tier
        // `HostStoreView` and counts a warm hit only when validation
        // passes and the value is returned.
        let results = self.project_type_store.component_meta_results();
        let entry = results.get_with_view(self, current_view, &key, owner_whole_hash)?;

        // Rehydrate the resolution template into a fresh per-request state.
        // Returns None on the bounded eviction race where the snapshot
        // was evicted between the warm-cache validation and reload.
        let cached = entry.payload.clone();
        let resolution = cached.resolution_template.rehydrate(
            self,
            &cached.canonical_id,
            cached.whole_hash,
            request_id,
        )?;

        // Synthesize a from_cache audit record so consumers via
        // `take_audit_record(resolution.request_id)` get uniform
        // observability. Snapshot per-request cache counters from
        // the active TLS context — the warm path consulted
        // `ComponentMetaResultDb::get` and `FileArtifactStore::get`
        // through `shallow_file_state`, both of which bumped
        // hits/misses on this request's `cache_counters`. The
        // joiner-accounting contract requires the snapshot to
        // attribute exactly to THIS request, not a host-global delta.
        // The peak-RSS slot is read from the active request context —
        // if the sampler thread ticked while the warm-cache path ran,
        // the peak surfaces here too.
        if self.config.audit_enabled {
            let store = crate::component_meta_audit::RequestStoreAudit {
                cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
                bypass_diagnostics:
                    crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
                ..Default::default()
            };
            // Warm-cache replay carries the same parent-request and
            // scheduler attribution the live request would have
            // observed had it run cold — read both off the active
            // request context (installed by the audited entry-point
            // a few lines above this branch). The same context lookup
            // also surfaces the per-request peak-RSS slot and the
            // per-request accumulator (used below to synthesise the
            // warm-path footprint).
            let mut memory = crate::component_meta_audit::RequestMemoryAudit::default();
            let (parent_request_id, scheduler_audit, waits, trace_id, footprint, files) =
                match crate::request_context::current_request_context() {
                    Some(ctx) if ctx.request_id == request_id => {
                        memory.process_rss_peak_bytes = ctx
                            .process_rss_peak_bytes
                            .load(std::sync::atomic::Ordering::Relaxed);
                        // Finalise the footprint through the SAME path
                        // the cold resolver uses
                        // (`compute_and_record_component_meta`): drain
                        // THIS request's accumulator, build the per-file
                        // audit vector off the drained state, then feed
                        // the rest through the deterministic miner. A
                        // warm hit performs little/no VFS work, so the
                        // mined footprint is typically empty — but it is
                        // always `Some(..)`, never `None`, so every
                        // audited request (warm or cold) carries a
                        // footprint. The accumulator is per-request and
                        // the `SessionVfsSink` filters by `request_id`,
                        // so the drained state attributes ONLY this
                        // request's reads — the strict per-request
                        // isolation the cold path relies on is
                        // preserved. Footprint is `Some` exactly when
                        // the cold path would produce one (i.e. when
                        // `footprint_capture` is on and an accumulator is
                        // attached); when capture is off it stays `None`,
                        // matching the cold contract.
                        let (footprint, files) = if ctx.footprint_capture {
                            if let Some(acc) = ctx.audit_accumulator.as_ref() {
                                let state = acc.drain();
                                let direct_imports: rustc_hash::FxHashSet<String> = self
                                    .shallow_file_state(ctx.canonical_id.as_ref())
                                    .map(|sfs| {
                                        sfs.import_targets
                                            .values()
                                            .map(|t| t.canonical_id.clone())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let files = crate::component_meta_audit::build_file_audit_vec(
                                    &state,
                                    ctx.canonical_id.as_ref(),
                                    &direct_imports,
                                    self.config.audit_timing_capture && self.config.audit_enabled,
                                );
                                let footprint = crate::component_meta_audit::mine_footprint(
                                    self.project_type_store().semantic_graph(),
                                    state,
                                    &ctx,
                                    self.config.max_derivation_edges,
                                    &self.config.audit_caps,
                                );
                                (Some(footprint), files)
                            } else {
                                (None, Vec::new())
                            }
                        } else {
                            (None, Vec::new())
                        };
                        // Surface `WaitAudit` only when the host's
                        // `audit_timing_capture` flag is on (mirrored on
                        // `RequestContext::timing_capture`). The warm
                        // path observed no locks of its own, but the
                        // aggregate state on the context is the source
                        // of truth — a stricter rule (always populate
                        // when context exists) would mask the flag-gate.
                        let waits = if ctx.timing_capture {
                            Some(verter_audit::WaitAudit {
                                lock_wait_ns: ctx
                                    .lock_wait_ns
                                    .load(std::sync::atomic::Ordering::Relaxed),
                                queue_wait_ns: ctx
                                    .queue_wait_ns
                                    .load(std::sync::atomic::Ordering::Relaxed),
                                lock_acquisitions: ctx
                                    .lock_acquisitions
                                    .load(std::sync::atomic::Ordering::Relaxed),
                            })
                        } else {
                            None
                        };
                        let parent_request_id = ctx.parent_request_id.map(|id| id.to_string());
                        let scheduler_audit = ctx.scheduler_audit.lock().clone();
                        let trace_id = ctx.trace_id.clone();
                        (
                            parent_request_id,
                            scheduler_audit,
                            waits,
                            trace_id,
                            footprint,
                            files,
                        )
                    }
                    _ => (None, None, None, String::new(), None, Vec::new()),
                };
            let synthesized = crate::component_meta_audit::RequestAuditRecord {
                request_id,
                canonical_id: canonical.to_string(),
                kind: crate::component_meta_audit::RequestKind::ComponentMeta,
                parent_request_id,
                timings: crate::component_meta_audit::RequestTimingAudit::default(),
                store,
                memory,
                footprint,
                scheduler: scheduler_audit,
                files,
                waits,
                from_cache: true,
                kind_payload: crate::component_meta_audit::RequestKindPayload::ComponentMeta(
                    crate::component_meta_audit::ComponentMetaPayload::default(),
                ),
                trace_id,
                capture_state: verter_audit::AuditCaptureState::ActiveStored,
            };
            debug_assert_eq!(synthesized.request_id, resolution.request_id);
            self.finalize_request_audit_record(synthesized);
        }

        Some((cached.analysis.clone(), resolution))
    }
}
