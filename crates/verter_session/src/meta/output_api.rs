//! `meta::output_api` — the output-envelope surfaces of [`MetaSession`]
//! (`super::MetaSession`): the fixed-view scalar and batch entries producing
//! the session-owned [`crate::meta_resolve::ComponentMetaOutput`], plus the
//! test-only payload completeness probe. Continuation `impl MetaSession`
//! block; split out of `meta.rs` to keep the parent module within the
//! production file-size gate.

#[cfg(test)]
use super::PAYLOAD_ITEM_COMPLETENESS_PROBE;
use super::{component_meta_resolution_budget_error, MetaError, MetaSession};

impl MetaSession {
    /// Output-envelope scalar through this session's overlay view: the
    /// session-owned [`crate::meta_resolve::ComponentMetaOutput`] with ALL
    /// 11 materialized wire type lanes and no resolution sidecar (the
    /// type lanes are fully resolved; only the sidecar/registry overlay
    /// is omitted — the payload entries and the audited entry seed it).
    /// Shares the fixed-view fast path with the batch surface (N=1):
    /// pre-warm overlays once, capture ONE fixed view, warm-probe +
    /// materialize against that single capture.
    pub fn get_component_meta_output(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::meta_resolve::ComponentMetaOutput>, MetaError> {
        self.check_alive()?;
        let host = self.project.host();
        let output = self.with_overlay_view(|view| {
            crate::host_manage::overlay_priority::prewarm_view_overlays(host, view);
            let fixed = host.capture_batch_fixed_view(view);
            host.get_component_meta_output_via_view_with_fixed_store_view(
                canonical_or_alias,
                view,
                &fixed,
                false,
            )
        });
        output.map_err(MetaError::from)
    }

    /// Batch surface for [`Self::get_component_meta_output`]: one shared
    /// overlay view, ONE captured fixed view threaded into every per-job
    /// call (no extra per-item store-view reads), one host-coordinated
    /// batch submission. Per-id failures (including typed
    /// output-materialization failures) surface in the per-result slot;
    /// the batch does not abort.
    pub fn get_component_meta_output_batch(
        &self,
        canonical_or_aliases: &[String],
    ) -> Result<Vec<Result<Option<crate::meta_resolve::ComponentMetaOutput>, MetaError>>, MetaError>
    {
        use std::sync::Arc;
        self.check_alive()?;
        let scheduler = self.project.host().scheduler();
        let host = self.project.host();
        let jobs: Vec<verter_scheduler::stage::SchedulerJobKind> = canonical_or_aliases
            .iter()
            .map(
                |canonical| verter_scheduler::stage::SchedulerJobKind::ComponentMeta {
                    canonical_id: Arc::from(canonical.as_str()),
                },
            )
            .collect();
        let on_item_panic = |panic: crate::host_batch_coordinator::BatchItemPanic<
            '_,
            verter_scheduler::stage::SchedulerJobKind,
        >| {
            let verter_scheduler::stage::SchedulerJobKind::ComponentMeta { canonical_id } =
                panic.item;
            Err(MetaError::Host(format!(
                "component-meta output batch job for `{}` panicked: {}",
                canonical_id,
                panic.message()
            )))
        };
        let policy = crate::host_batch_coordinator::BatchPolicy {
            scheduler: Some(scheduler.as_ref()),
            label: "component_meta_output_batch",
            on_item_panic: &on_item_panic,
        };
        let results = self.with_overlay_view(|view| {
            // Pre-warm overlays ONCE for the whole batch BEFORE deriving the
            // fixed view, then capture ONE fixed view and thread it into
            // every per-job call — the same single-capture discipline as the
            // analysis and payload batch surfaces.
            crate::host_manage::overlay_priority::prewarm_view_overlays(host, view);
            let fixed = host.capture_batch_fixed_view(view);
            host.batch_coordinator().run_batch(&jobs, &policy, |job| {
                let verter_scheduler::stage::SchedulerJobKind::ComponentMeta { canonical_id } = job;
                host.get_component_meta_output_via_view_with_fixed_store_view(
                    canonical_id.as_ref(),
                    view,
                    &fixed,
                    false,
                )
                .map_err(MetaError::from)
            })
        });
        Ok(results)
    }

    /// Test-only observation of the per-item TYPED completeness at the
    /// payload boundary. The wire envelope deliberately carries no compute
    /// completeness (it is session bookkeeping, not wire data), so the
    /// batch-partiality tests arm this probe to observe
    /// `(final_completeness.is_partial(), synthesis_should_suppress)` per
    /// canonical on the ACTUAL fixed-view batch path. A `Mutex`-guarded map
    /// (not a thread-local) because batch jobs run on pool worker threads.
    #[cfg(test)]
    pub(crate) fn arm_payload_completeness_probe() {
        *PAYLOAD_ITEM_COMPLETENESS_PROBE.lock().unwrap() = Some(rustc_hash::FxHashMap::default());
    }

    /// Drain the armed test probe (see
    /// [`Self::arm_payload_completeness_probe`]).
    #[cfg(test)]
    pub(crate) fn take_payload_completeness_probe() -> rustc_hash::FxHashMap<String, (bool, bool)> {
        PAYLOAD_ITEM_COMPLETENESS_PROBE
            .lock()
            .unwrap()
            .take()
            .unwrap_or_default()
    }
}

impl MetaSession {
    /// Resolve ONE encoded component-meta payload against a
    /// caller-captured [`crate::resolver_store::BatchFixedView`] and the
    /// shared session `view`.
    ///
    /// This is the single per-item body shared by the batch
    /// ([`Self::get_component_meta_batch_payloads`]) and scalar
    /// ([`Self::get_component_meta_payload`]) payload paths, so the two
    /// surfaces stay byte-identical (no dual path). It performs, in order:
    ///
    /// 1. **Warm probe** against the fixed view's proven-current view
    ///    (when the capture was current). A non-current capture skips the
    ///    probe (miss to cold) — it must never validate a cache entry
    ///    against a stale snapshot.
    /// 2. **Cold resolve** via
    ///    [`crate::VerterHost::resolve_component_meta_with_view_and_fixed`],
    ///    pinning the request executor to the fixed view (the O(N)→O(1)
    ///    win) with the FENCED promotion gate.
    /// 3. **Extraction** under a `HostResolverContext` seeded from the
    ///    SHARED batch cold-seed (`fixed.cold_seed()`) — not a fresh
    ///    per-item `resolver_store_view_read()`.
    /// 4. **Payload-write fence**: the encoded payload is
    ///    promoted into the per-file payload cache ONLY when
    ///    [`crate::resolver_store::BatchFixedView::payload_promotion_admissible`]
    ///    holds (the capture was current AND no external mutation landed
    ///    since capture). On a decline the payload is still RETURNED to the
    ///    caller; only the cache write is dropped — so a mid-batch
    ///    invalidation cannot admit a stale payload.
    ///
    /// Returns `Ok(Some(bytes))` on success, `Ok(None)` when the canonical
    /// does not resolve to a component, and `Err(_)` on a per-id failure (a
    /// budget overrun or a typed output-materialization failure). The
    /// scalar caller propagates `Err` (interactive); the batch caller keeps
    /// it as a typed per-item `Err` slot (never the missing sentinel) —
    /// both consume the SAME body so the two surfaces stay byte-identical.
    pub(super) fn resolve_one_payload_item(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
        encode_fn: impl FnOnce(crate::meta_resolve::ComponentMetaOutput) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, MetaError> {
        use std::sync::atomic::Ordering::Relaxed;
        let host = self.project.host();
        let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

        // (1) Warm probe — only against a PROVEN-CURRENT fixed view. A
        // non-current capture (`current_view() == None`) misses to cold so
        // it never validates a cached payload against a stale snapshot.
        //
        // SOUNDNESS: the fixed view's current view is ALREADY overlay-aware —
        // `capture_batch_fixed_view` applies the session overlay ONCE at
        // capture and shares it across every job. Validating against this
        // shared overlaid view (rather than an un-overlaid base view) means a
        // session that mutates a DEPENDENCY of an owner whose own whole-hash
        // is unchanged sees the overlaid dep fact MISS, falls to the
        // overlay-aware cold resolve, and returns the overlay surface — never
        // false-positiving the cached BASE payload. The overlay is NOT
        // re-applied here per job: that per-job copy-on-write was the O(N²)
        // regression; the batch applies it once and shares it. For a base
        // (empty-overlay) session the shared view IS the base snapshot, so
        // validation is identical to the base — no behavior change.
        if let Some(current_view) = fixed.current_view() {
            if let Some(cached) =
                host.try_get_cached_meta_payload_with_store_view(current_view, canonical.as_str())
            {
                host.provenance().payload_cache_hits.fetch_add(1, Relaxed);
                return Ok(Some(cached));
            }
        }
        host.provenance().payload_cache_misses.fetch_add(1, Relaxed);

        // Install ONE request context (with `config.projection_op_budget`)
        // spanning the cold resolve AND the fallthrough extract, so the
        // projection-op budget fuse and the no-poison completeness gate are
        // uniformly LIVE on the payload surface exactly as on the analysis
        // surface (the Shared Optimized Codebase rule — a budget partial must
        // be observable here, not only through the analysis entry). Held to
        // function end so it covers step-(2) resolve AND step-(3) extract; the
        // step-(2) inner install-if-none-active reuses this outer context. This
        // is the SHARED body for BOTH the scalar and the batch per-job payload
        // paths, so this single install covers both (a per-job install on a
        // batch pool thread is correct — `RequestContext` is thread-local RAII).
        let _payload_request_ctx_guard = host.install_request_budget_context_if_none(
            crate::meta_resolve::next_component_meta_audit_request_id(),
            canonical.as_str(),
            host.config.audit_timing_capture && host.config.audit_enabled,
        );

        // (2) Cold resolve pinned to the fixed view (FENCED promotion).
        let Some((mut resolved, admission)) = ({
            let (executor_view, captured_fp) = fixed.executor_fixed_view();
            host.resolve_component_meta_with_view_and_fixed_admission(
                canonical.as_str(),
                crate::types::ProjectionMode::Expanded,
                view,
                Some((executor_view, captured_fp, fixed.current_view().is_some())),
            )
        }) else {
            return Ok(None);
        };

        // (3) Extraction context seeded from the SHARED batch cold-seed —
        // no fresh per-item store-view read. The cold-seed carries the
        // capture's currentness, so a non-current seed fails nested warm
        // probes closed.
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            host,
            fixed.cold_seed(),
            std::sync::Arc::clone(&overlay),
        );
        let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let crate::host_manage::ComponentMetaExtractOutcome {
            analysis,
            fallthrough_fact_versions,
            completeness: extract_completeness,
        } = crate::host_manage::extract_component_meta_from_resolved_with_facts(
            host,
            canonical.as_str(),
            &resolved,
            host_ctx_ref,
        );
        host.merge_extraction_facts_into_admitted_resolved_meta(
            canonical.as_str(),
            crate::types::ProjectionMode::Expanded,
            view.fingerprint(),
            &mut resolved,
            fallthrough_fact_versions.as_deref(),
            admission.as_ref(),
        );
        // ONE merged admission signal: the resolve-phase completeness merged
        // with the whole-extract scope (macro-DTO read + fallthrough compute).
        let final_completeness = resolved.completeness.merge(extract_completeness);
        #[cfg(test)]
        if let Some(map) = PAYLOAD_ITEM_COMPLETENESS_PROBE.lock().unwrap().as_mut() {
            map.insert(
                canonical.to_string(),
                (
                    final_completeness.is_partial(),
                    resolved.synthesis_should_suppress,
                ),
            );
        }

        if let Some(err) =
            component_meta_resolution_budget_error(canonical.as_str(), Some(&analysis), &resolved)
        {
            return Err(err);
        }

        // (3b) Materialize the session-owned OUTPUT envelope under the SAME
        // request-bound context the extract ran under (the batch cold-seed —
        // the analysis and the output observe one snapshot), in its OWN
        // fact-tracer scope so the output-materialization dependencies are
        // traced SEPARATELY and folded into the ENCODED-payload cache rail
        // below (never into any analysis-entry signature). A typed failure
        // propagates as `MetaError::OutputMaterialization` — the encoded
        // payload is refused, while the analysis/resolved caches published
        // by step (2) stay warm (output failure suppresses ONLY
        // output/encoded-payload admission).
        let seed = crate::meta_resolve::output::ComponentMetaResolutionSeed::from_resolved_state(
            &resolved,
        );
        // SESSION-BOUND output context (the session-bound counterpart of
        // `HostResolverContext::from_cold_seed`, SAME capture, same fence):
        // an output-time raise that replays a producing route
        // (`ensure_indexed_ready_serve` on the owning SFC — the macro hot
        // mirror, the member-path / callable-params replays) must observe
        // the session view the analysis was served under; the base-bound
        // context cannot serve an overlay-only canonical and fails those
        // raises typed. Scoped to the OUTPUT materialization only — the
        // extract above keeps the established base-bound binder. A base
        // (empty-overlay) session's view falls through to the host's
        // standard reads — identical behavior.
        let output_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
            host,
            view,
            fixed.cold_seed(),
            overlay,
        );
        let output_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext =
            &output_ctx;
        let (output_result, output_read_set) = host.with_fact_tracer(|| {
            crate::meta_resolve::projectors::build_component_meta_output(
                output_ctx_ref,
                canonical.as_str(),
                analysis,
                Some(seed),
            )
        });
        let output = output_result?;
        // Snapshot the output-materialization tracer's non-cacheability bit
        // BEFORE `finalise` consumes the read-set below. A fenced (ReturnOnly,
        // `store_published == false`) `IndexedReady` serve consumed inside
        // `build_component_meta_output` (an output-time raise replaying a
        // producing route — the macro hot mirror, member-path / callable-params
        // replays) fans onto this tracer: the encoded payload was computed from
        // a served-without-publication (superseded) artifact while its fact
        // stamps validate against the live view, so it MUST NOT warm the payload
        // cache. Orthogonal to completeness — the payload is still returned to
        // this caller; only the shared-cache write is refused.
        let output_non_cacheable = output_read_set.non_cacheable_read_observed();

        let payload = encode_fn(output);
        host.provenance().payload_encodes.fetch_add(1, Relaxed);

        // (4) Payload-write fence. Promote the encoded payload
        // into the per-file payload cache ONLY when the fixed view is still
        // promotable (current + not externally superseded since capture).
        // Otherwise return the payload but do NOT warm the cache with a
        // result computed against a now-stale snapshot.
        let mut facts = fallthrough_fact_versions.unwrap_or_else(|| resolved.fact_versions.clone());
        // Output-materialization dependencies join the ENCODED-payload
        // validation rail: an edit to any file the materialization observed
        // misses the warm payload read. A signature overflow refuses payload
        // admission (ReturnOnly) — the payload is still returned.
        let output_facts_admissible = match output_read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(output_facts) => {
                let mut seen: rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef> =
                    facts.iter().cloned().collect();
                for fact in output_facts.iter() {
                    if seen.insert(fact.clone()) {
                        facts.push(fact.clone());
                    }
                }
                true
            }
            crate::resolver_core::FactReadSetFinalise::NonCacheable(_) => false,
            crate::resolver_core::FactReadSetFinalise::Overflow => false,
        };
        // Conjunctive rails: the token fence (external supersession /
        // currentness) AND the output-materialization non-cacheability rail
        // (`output_non_cacheable` — a fenced/lease-miss serve consumed while
        // building the output) AND the per-result completeness rail — a partial
        // (budget-fail-closed / carrier-stopped) payload is returned but
        // never admitted, so a transient trip cannot warm-replay as a
        // sticky degraded payload. The completeness rail is ONE merged signal:
        // `final_completeness = resolved.completeness.merge(extract_completeness)`
        // — the resolve-phase completeness merged with the WHOLE-extract scope
        // (the pre-choke macro-DTO read + the fallthrough cold compute). The
        // former `synthesis_should_suppress` term is SUBSUMED (it is the bool
        // projection of `resolved.completeness`, already a merge operand).
        if output_facts_admissible
            && !output_non_cacheable
            && fixed.payload_promotion_admissible(host)
            && !final_completeness.is_partial()
        {
            // Stamp from the FLIGHT-CAPTURED generation (the fixed
            // view's captured token), never the live counter: a project
            // bump landing between the fence above and this store must
            // leave the payload stamped under the graph it was computed
            // from, so the warm read's generation backstop rejects it.
            host.store_meta_payload(
                canonical.as_str(),
                &facts,
                payload.clone(),
                fixed.captured_validation_token().project_generation,
            );
        }
        Ok(Some(payload))
    }
}
