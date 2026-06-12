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
//! `component_meta_entry` (`cold_seed_view_and_fence`,
//! `publish_if_admissible`, `publish_component_meta_cache_entry`).
//! Public surface remains rooted at `crate::host_manage::*`; this file
//! contributes a continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::VerterHost;

use super::{
    component_meta_options_fingerprint, extract_component_meta_from_resolved, ComponentMetaOptions,
};

impl VerterHost {
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

        // Build a `RequestContext` first; the registration consumes
        // the same `Arc` so the active-request entry is keyed by the
        // request id and the kind comes from the context.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let accumulator = if footprint_capture {
            // Wire `HostConfig::audit_caps` through to the
            // accumulator so per-host cap overrides take effect on
            // every raw push lane (structured_events, vfs_reads,
            // materializations, etc.), not just the post-mining
            // caps that `mine_footprint` applies. Using `::new()`
            // here would hardcode `AuditCaps::default()` (10_000
            // per category) regardless of `host.config.audit_caps`.
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
            std::sync::Arc::<str>::from(canonical.as_str()),
            verter_audit::RequestKind::ComponentMeta,
            footprint_capture,
            self.config.audit_timing_capture && self.config.audit_enabled,
            accumulator.clone(),
            self.config.projection_op_budget,
        );

        // Construct the audit registration BEFORE installing the TLS
        // guard. The `Active` arm enters the host's active-request
        // registry; the `Noop` arm is returned when the consumer
        // filter rejects the kind (no record will be produced
        // downstream). Plant the registration on the request context
        // so the inner resolver path finalises through it instead of
        // routing the record through a direct host insert.
        let registration =
            std::sync::Arc::new(crate::host_audit_runtime::AuditRequestRegistration::new(
                self,
                std::sync::Arc::clone(&ctx),
            ));
        // The OnceLock returns Err only on a re-entrant install,
        // which the production entry-point cannot trigger because
        // the context is freshly constructed.
        debug_assert!(
            ctx.audit_registration.get().is_none(),
            "freshly-constructed RequestContext must have no audit_registration",
        );
        let _ = ctx.install_audit_registration(std::sync::Arc::clone(&registration));

        // Register a per-request `SessionVfsSink` with the workspace
        // so VFS reads populate the accumulator's `vfs_reads`. The
        // registration must outlive the `RequestContextGuard` below
        // so late events still route correctly; it is dropped FIRST
        // at scope exit (field order: `_sink_registration` above
        // `_ctx_guard` would drop registration LAST, which we want).
        //
        // Rust drops locals in REVERSE declaration order, so we
        // declare the guard FIRST and the registration SECOND: at
        // scope exit, the registration drops first (deregistering
        // the sink — no more fan-out events arrive), then the
        // context guard drops, then the accumulator Arc drops.
        let _ctx_guard = crate::request_context::RequestContextGuard::install(ctx);
        let _sink_registration = accumulator.as_ref().and_then(|acc| {
            let sink = crate::component_meta_audit::session_vfs_sink::SessionVfsSink::new(
                request_id,
                std::sync::Arc::clone(acc),
            );
            self.workspace().register_audit_sink(sink).ok()
        });

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
        // tracer covers BOTH `resolve_component_meta` and
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
        // Publish fence + cold-seed view in ONE read — see
        // `get_component_meta` for the divergence-free derivation and the
        // request-bound-ctx construction rationale. A non-current seed
        // declines promotion.
        let (store_view, seed_fence) = self.cold_seed_view_and_fence();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &store_view, overlay);
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let (maybe_resolved_analysis, read_set) = self.with_fact_tracer(|| {
            let mut resolved = match self
                .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
            {
                Some(r) => r,
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
            let analysis = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
                host_ctx_ref,
            );
            Some((analysis, resolved))
        });
        let (analysis, resolved) = maybe_resolved_analysis?;

        // Seal + admission decision — `publish_if_admissible` (by-value
        // fenced-serve consult + R20 finalise). An admitted write lets
        // subsequent identical calls short-circuit through
        // `try_with_resolution_cache_hit`; suppression is enforced inside
        // `publish_component_meta_cache_entry` via
        // `resolved.synthesis_should_suppress`.
        self.publish_if_admissible(
            canonical.as_str(),
            "with-resolution path",
            read_set,
            |sig| {
                self.publish_component_meta_cache_entry(
                    canonical.as_str(),
                    &resolved,
                    analysis.clone(),
                    sig,
                    validated_at_generation,
                    &seed_fence,
                );
            },
        );

        Some((analysis, resolved))
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

        // Cold compute through the view-bearing path so the view's
        // fingerprint discriminates the singleflight slot.
        let mut resolved = self.resolve_component_meta_with_view(
            canonical.as_str(),
            crate::types::ProjectionMode::Expanded,
            view,
        )?;
        resolved.request_id = self.next_request_id();
        // Build a HostResolverContext before extract so engine
        // constructions inside the policy / fallthrough path bind to the
        // request-bound ctx rather than a bare-host. This is a post-fence
        // extraction binder — `resolve_component_meta_with_view` already ran
        // under its own publish fence — so it threads a COLD-SEED view: the
        // resolve owns currentness, and a non-current seed fails the
        // ctx's nested warm-cache probes closed rather than validating
        // against a stale snapshot.
        let store_view = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &store_view, overlay);
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let analysis = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true,
            host_ctx_ref,
        );
        Some((analysis, resolved))
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
        let shallow = self.shallow_file_state(canonical)?;
        let owner_whole_hash = shallow.whole_hash;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        // Fact-precise validation is the sole cache oracle:
        // `ComponentMetaResultDb::get_with_view` validates the entry's
        // `read_set_signature.facts` against the resolver-tier
        // `HostStoreView` and counts a warm hit only when validation
        // passes and the value is returned. Accepts ONLY a
        // `CurrentHostStoreView`: a known-stale `ReturnOnly` snapshot
        // misses to the cold recompute path rather than validating an
        // entry against an already-superseded view.
        let results = self.project_type_store.component_meta_results();
        let Some(current_view) = self.resolver_store_view_read().current() else {
            results.record_non_current_view_miss(self);
            return None;
        };
        let entry = results.get_with_view(self, &current_view, &key, owner_whole_hash)?;

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
