//! `host_manage::component_meta_entry` — public component-meta query
//! entry points + audit-record dispatch.
//!
//! Domain H. Holds the `evaluate_types`,
//! `get_component_meta`, and `get_component_meta_with_resolution`
//! public entry points along with the
//! [`ComponentMetaResultDb`](crate::component_meta_result_db::ComponentMetaResultDb)
//! cache hit / publish / dep-signature helpers and the audit-record
//! intake. Public surface remains rooted at `crate::host_manage::*`;
//! this file contributes a continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;

use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_options_fingerprint,
    extract_component_meta_from_resolved, ComponentMetaOptions,
};

#[cfg(test)]
thread_local! {
    /// Test-only knob: when `true`, the next
    /// [`VerterHost::validation_token_still_live`] call returns `false`
    /// (consuming the flag), forcing the publish fence to skip promotion
    /// exactly as if the snapshot had been superseded mid-flight. Lets
    /// the discriminating publish-fence test assert the cold result is
    /// NOT promoted under a superseded token without a racing thread.
    pub(crate) static PUBLISH_FENCE_FORCE_SUPERSEDE: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
thread_local! {
    /// Test-only hook: runs ONCE (consuming itself) at the top of the two
    /// SHARED cold bodies (`component_meta_via_view_cold` /
    /// `component_meta_with_resolution_cold`) — AFTER the entry captured its
    /// fixed store view, BEFORE the pinned resolve dispatches. Lets a
    /// barrier-controlled view-consistency test land a dependency mutation
    /// in exactly the capture→resolve window and assert the response is
    /// view-CONSISTENT (fully the captured view's world — never a
    /// fresh-view analysis paired with capture-bound materialization).
    pub(crate) static COLD_BODY_PRE_RESOLVE_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Run (and consume) the test-only pre-resolve hook. No-op when disarmed.
#[cfg(test)]
pub(crate) fn run_cold_body_pre_resolve_hook() {
    if let Some(hook) = COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
}

#[cfg(test)]
thread_local! {
    /// Test-only hook: runs ONCE (consuming itself) on the WARM arm of the
    /// output-bearing view entry — AFTER the warm cache entry validated
    /// against the captured view, BEFORE the output materializes. Lets a
    /// view-consistency test land a dependency mutation in exactly that
    /// window and assert the materialization runs under the SAME capture
    /// the validation used (an old-analysis/fresh-view implementation
    /// materializes the mutated world and tears).
    pub(crate) static WARM_OUTPUT_PRE_MATERIALIZE_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Run (and consume) the test-only warm pre-materialize hook. No-op when
/// disarmed.
#[cfg(test)]
pub(crate) fn run_warm_output_pre_materialize_hook() {
    if let Some(hook) = WARM_OUTPUT_PRE_MATERIALIZE_HOOK.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
}

/// The publish fence a cold component-meta compute must pass before its
/// result may warm the shared cache.
///
/// Both fields are derived from ONE captured
/// [`crate::resolver_store::BatchFixedView`]: `token` is the capture's
/// validation token and `current` is the capture's
/// [`crate::resolver_store::StoreViewRead`] arm, so neither can describe
/// a different snapshot than the one the cold compute ran under.
#[derive(Debug, Clone, Copy)]
pub(super) struct ColdSeedFence {
    /// The validation token of the exact view the cold compute used.
    token: crate::resolver_store::StoreViewValidationToken,
    /// Whether the manager proved that view current. A non-current
    /// (`ReturnOnly`) seed is never promoted.
    current: bool,
}

impl ColdSeedFence {
    /// Assemble a fence from a token + currentness pair that provably came
    /// from ONE store-view read (entries derive the pair from a shared
    /// [`crate::resolver_store::BatchFixedView`] capture).
    pub(super) fn new(
        token: crate::resolver_store::StoreViewValidationToken,
        current: bool,
    ) -> Self {
        Self { token, current }
    }
}

/// Strip the OWNER's own `DerivedFactHash { kind: Route }` fact from a
/// `ComponentMetaResultEntry` signature before cache admission.
///
/// **Why exactly this one fact.** The owner's `Route` hash is the only
/// fact in the tracer-owned signature that does NOT reliably
/// round-trip through warm validation: the owner's own `IndexedReady`
/// can lazily (re-)materialise mid-request — the cold compute's route
/// walk observes the owner's Route fact at one point of that
/// lifecycle while a later warm-hit validation reads the refreshed
/// surface. The two can disagree even with no edit, so the warm hit
/// would miss and the query cold-recompute every time — a
/// steady-state warm-cache miss / perf regression.
///
/// The filter is deliberately narrow:
///
/// - Only `kind == Route` is dropped. `ImportRoute` and `DirectSource`
///   derived facts round-trip and stay.
/// - Only the OWNER's own Route fact is dropped (`canonical_id ==
///   owner_canonical`). Cross-file route facts — Route facts for the
///   route DEPS the cold compute walked — round-trip correctly and
///   MUST stay so an edit to a route dep still invalidates the
///   owner's warm hit.
/// - The owner's `FileWholeHash` fact is untouched, so owner-content
///   edits still invalidate the warm hit.
///
/// Returns the input unchanged (cloned into a fresh `Arc`) when no
/// owner-Route fact is present.
///
/// `pub(crate)` so the `resolve_component_meta` warm-cache publish
/// boundaries (`component_meta_methods.rs`) apply the identical filter —
/// the two cache paths must reach the same warm-validation outcome for
/// the same owner.
pub(crate) fn strip_owner_route_fact(
    owner_canonical: &str,
    facts: &[crate::resolver_core::FactVersionRef],
) -> Arc<[crate::resolver_core::FactVersionRef]> {
    let filtered: Vec<crate::resolver_core::FactVersionRef> = facts
        .iter()
        .filter(|fact| {
            !matches!(
                fact,
                crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id,
                    kind: crate::resolver_core::DerivedFactKind::Route,
                    ..
                } if canonical_id == owner_canonical
            )
        })
        .cloned()
        .collect();
    Arc::from(filtered.into_boxed_slice())
}

impl VerterHost {
    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved = self
            .resolve_component_meta(canonical_or_alias, crate::types::ProjectionMode::Expanded)?;
        resolved.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Uses `resolve_component_meta(Expanded)` as the single enrichment owner,
    /// then projects the result through the analysis-owned `extract_component_meta`.
    ///
    /// Wires this through
    /// [`ComponentMetaResultDb`](crate::component_meta_result_db::ComponentMetaResultDb):
    /// the method consults the project-global result cache first, revalidates
    /// the cached entry's dep-signature against the live host, and only falls
    /// back to the cold resolver path on miss or stale signature. The cold
    /// build publishes through the cooperative-admission completion-fence
    /// path: the published entry's dep-signature is revalidated against the
    /// live host before it warms the shared cache, so a result torn by a
    /// mid-flight change is discarded rather than published as torn state.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Try the final-result cache before installing a request
        // view. A warm hit with a valid dep-signature returns with zero
        // resolver work.
        if let Some(warm) = self.try_component_meta_cache_hit(canonical.as_str()) {
            if let Some(started) = started {
                component_meta_debug(format!(
                    "get_component_meta owner={} warm-cache hit took {:?}",
                    canonical,
                    started.elapsed(),
                ));
            }
            return Some(warm);
        }

        let _ctx_guard = self.install_request_budget_context_if_none(
            self.next_request_id(),
            canonical.as_str(),
            false,
        );

        // Cold build through the SHARED pinned cold body
        // (`component_meta_via_view_cold`) with a base `HostViewRef` view
        // and ONE captured `BatchFixedView`: the publish fence, the
        // extraction context, AND the resolve executor all derive from that
        // single capture, so the resolve can never open a second unrelated
        // store view and pair a fresh-view analysis with the capture-bound
        // extraction context (the torn-result race). The cold body installs
        // the `with_fact_tracer` scope (R24: cold-path only — the warm-hit
        // fast path above returned before reaching here) and routes the
        // seal + admission decision through `publish_if_admissible`.
        //
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work. The carrier
        // (`fact_dep_signature`) validates only file-content
        // whole-hashes; a `ProjectGeneration` reset (tsconfig /
        // path-alias / SDK / workspace-folder change) bumps no file
        // content, so without this snapshot a
        // `bump_project_generation_and_evict` racing this cold publish
        // could strand a stale-by-project-generation entry whose
        // carrier still validates. The published entry stamps the
        // snapshotted generation; `ComponentMetaResultDb::get_with_view`
        // rejects on warm read when the live generation differs.
        let validated_at_generation = self.project_type_store.current_project_generation();
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        let seed_fence = ColdSeedFence::new(fixed.captured_validation_token(), fixed.is_current());
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            overlay,
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let (_resolved, meta) = self.component_meta_via_view_cold(
            canonical.as_str(),
            &view,
            &fixed,
            ctx,
            &seed_fence,
            validated_at_generation,
        )?;

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
    }

    /// Output-bearing BASE-HOST component-meta entry: produce the
    /// session-owned [`crate::meta_resolve::ComponentMetaOutput`] envelope
    /// (all 11 materialized wire type lanes), warm-cache-aware, with the
    /// materialization ALWAYS driven inside the same validated view the
    /// analysis was served under. The envelope carries no resolution
    /// sidecar (the audited
    /// [`Self::get_component_meta_output_with_resolution`] entry and the
    /// payload entries carry it); the materialized type lanes are fully
    /// resolved either way.
    ///
    /// Wraps the shared view-core with a base [`crate::session_view::HostViewRef`]
    /// and ONE captured [`crate::resolver_store::BatchFixedView`] — the warm
    /// probe validates against that capture's proven-current view and the
    /// materialization context seeds from the SAME capture, so a warm
    /// analysis can never be paired with a different view's dispatch.
    ///
    /// A materialization failure is the typed
    /// [`crate::meta_resolve::ComponentMetaOutputError`] (fail-closed; a
    /// present-but-unraisable source never silently materializes as
    /// `Unknown`). `Ok(None)` = the component does not resolve. An output
    /// failure never suppresses the independently-complete ANALYSIS cache
    /// entry the cold path publishes.
    pub fn get_component_meta_output(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<crate::meta_resolve::ComponentMetaOutput>,
        crate::meta_resolve::ComponentMetaOutputError,
    > {
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        self.get_component_meta_output_via_view_with_fixed_store_view(
            canonical_or_alias,
            &view,
            &fixed,
            false,
        )
    }

    /// Output-bearing VIEW entry pinned to a caller-captured
    /// [`crate::resolver_store::BatchFixedView`]: the overlay/session,
    /// fixed-view scalar, and fixed-view batch surfaces all route here (the
    /// batch threads ONE capture into every per-item call — no extra
    /// per-item store-view reads).
    ///
    /// View fence (both arms):
    ///
    /// - **Warm.** The warm probe validates against the capture's
    ///   proven-current OVERLAID view; on a hit the output materializes
    ///   under a request context seeded from the SAME capture's cold seed —
    ///   the analysis and the materialization dispatch observe one snapshot.
    /// - **Cold.** The shared cold body resolves + extracts + publishes the
    ///   analysis under the capture's cold seed, then the output
    ///   materializes under that SAME context BEFORE it drops — never
    ///   "resolve, return, then materialize under a second unrelated view".
    ///
    /// Cache rails: the ANALYSIS cache publish happens inside the cold body
    /// BEFORE materialization and is never suppressed by an output failure;
    /// the output materialization runs in its OWN fact-tracer scope so its
    /// dependencies never fold into the analysis entry's fact signature
    /// (output dependencies are traced separately by the encoded-payload
    /// lane that admits encoded results).
    ///
    /// `with_resolution` selects whether the envelope carries the narrowed
    /// resolution sidecar (and the session-owned registry name-overlay
    /// finalize that comes with it).
    pub(crate) fn get_component_meta_output_via_view_with_fixed_store_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
        with_resolution: bool,
    ) -> Result<
        Option<crate::meta_resolve::ComponentMetaOutput>,
        crate::meta_resolve::ComponentMetaOutputError,
    > {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        if view.is_tombstoned(canonical.as_str()) {
            return Ok(None);
        }

        // Budget context spans the warm-path materialization AND the cold
        // resolve+extract+materialize — the projection-op fuse is armed for
        // every output raise. Install-if-none so a batch / outer caller
        // keeps its context.
        let _budget_ctx_guard = self.install_request_budget_context_if_none(
            crate::meta_resolve::next_component_meta_audit_request_id(),
            canonical.as_str(),
            self.config.audit_timing_capture && self.config.audit_enabled,
        );

        // Warm probe against the capture's proven-current overlaid view.
        if let Some(cached) =
            self.try_component_meta_cache_entry_with_view(canonical.as_str(), view, fixed)
        {
            #[cfg(test)]
            run_warm_output_pre_materialize_hook();
            let overlay =
                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            // SESSION-BOUND materialization context (the session-bound
            // counterpart of `HostResolverContext::from_cold_seed`, same
            // fence: the overlaid cold seed + its currentness): an
            // output-time raise that replays a producing route
            // (`ensure_indexed_ready_serve` on the owning SFC — the macro
            // hot mirror, the member-path / callable-params replays) must
            // observe the SAME session view the analysis was served under;
            // the base-bound context cannot serve an overlay-only
            // canonical, failing those raises typed. A base-passthrough
            // view falls through to the host's standard reads — identical
            // behavior for non-overlay callers.
            let host_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
                self,
                view,
                fixed.cold_seed(),
                overlay,
            );
            let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
            let seed = with_resolution.then(|| {
                crate::meta_resolve::output::ComponentMetaResolutionSeed::from_template(
                    &cached.resolution_template,
                )
            });
            // Output-dependency tracing stays SEPARATE from any outer
            // analysis tracer scope (none is active on the warm path).
            let (output, _output_read_set) = self.with_fact_tracer(|| {
                crate::meta_resolve::projectors::build_component_meta_output(
                    ctx,
                    canonical.as_str(),
                    cached.analysis.clone(),
                    seed,
                )
            });
            return output.map(Some);
        }

        // Cold: shared cold body (publishes the analysis cache entry
        // independently of the output result), then materialize under the
        // SAME capture — the OUTPUT context is the session-bound wrapper
        // (see the warm arm) while the extract keeps the established
        // base-bound binder.
        let validated_at_generation = self.project_type_store.current_project_generation();
        let seed_fence = ColdSeedFence::new(fixed.captured_validation_token(), fixed.is_current());
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            std::sync::Arc::clone(&overlay),
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let Some((resolved, meta)) = self.component_meta_via_view_cold(
            canonical.as_str(),
            view,
            fixed,
            ctx,
            &seed_fence,
            validated_at_generation,
        ) else {
            return Ok(None);
        };
        let seed = with_resolution.then(|| {
            crate::meta_resolve::output::ComponentMetaResolutionSeed::from_resolved_state(&resolved)
        });
        // SEPARATE tracer scope: output-materialization dependencies never
        // fold into the analysis entry's fact signature (the cold body's
        // tracer already sealed + published above). SESSION-BOUND output
        // context (same capture, same fence — see the warm arm).
        let output_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
            self,
            view,
            fixed.cold_seed(),
            overlay,
        );
        let output_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext =
            &output_ctx;
        let (output, _output_read_set) = self.with_fact_tracer(|| {
            crate::meta_resolve::projectors::build_component_meta_output(
                output_ctx_ref,
                canonical.as_str(),
                meta,
                seed,
            )
        });
        output.map(Some)
    }

    /// View-aware variant of [`get_component_meta`].
    ///
    /// The supplied [`crate::session_view::SessionView`] is consulted
    /// for cache-key derivation (R17) and dep-signature revalidation
    /// (R19). This is the entry point sessions use to thread their
    /// per-overlay view into the consumer path so two sessions with
    /// conflicting overlays admit distinct multi-candidate slots in
    /// `ComponentMetaResultDb`.
    ///
    /// **Tombstone semantics.** If `view.is_tombstoned(canonical)` is
    /// `true`, the canonical is treated as deleted from the session's
    /// perspective and the call returns `None` without consulting the
    /// base host's cache. Base-only views (`HostView`,
    /// `HostViewRef`) never tombstone.
    pub fn get_component_meta_via_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        // No caller-captured fixed view: the body takes its own store-view
        // reads and runs the per-call overlay pre-warm.
        self.get_component_meta_via_view_inner(canonical_or_alias, view, None)
    }

    /// [`Self::get_component_meta_via_view`] pinned to a caller-captured
    /// [`crate::resolver_store::BatchFixedView`].
    ///
    /// The batch coordinator captures ONE fixed view (after pre-warming the
    /// overlays ONCE) and threads it into every per-job call, so the warm
    /// probe, the cold-seed extraction context, and the request executor
    /// all share that single snapshot instead of each per-job call taking
    /// its own `resolver_store_view_read()` — the O(N)→O(1) warm-batch read
    /// collapse for the analysis (struct-returning) path, matching the
    /// payload path. The caller is responsible for having run
    /// `prewarm_view_overlays` once before capturing the fixed view; this
    /// variant does NOT pre-warm per call. Promotion stays FENCED: the
    /// fixed view's captured token gates the publish.
    pub(crate) fn get_component_meta_via_view_with_fixed_store_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.get_component_meta_via_view_inner(canonical_or_alias, view, Some(fixed))
    }

    fn get_component_meta_via_view_inner(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: Option<&crate::resolver_store::BatchFixedView>,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Tombstone detection (R17): a session's overlay-Delete is the
        // explicit signal — never inferred from `source().is_none()`,
        // which fires for unloaded canonicals too.
        if view.is_tombstoned(canonical.as_str()) {
            return None;
        }

        // Overlay-priority pre-warm: thread the view through a
        // `SessionResolverContext` and pre-warm IndexedReady for the
        // owner AND every canonical the view carries an overlay for
        // (R20 multi-candidate isolation). The pre-warm publishes
        // overlay candidates under their content hashes so cross-file
        // resolver-tier reads inside the cold compute observe the
        // overlay for deps, not just the owner.
        //
        // When a caller-captured fixed view is supplied (the batch path),
        // the caller already ran this pre-warm ONCE before capturing the
        // fixed view — re-running it per job would defeat the O(1) goal AND
        // could publish overlay artifacts the captured fixed view cannot
        // observe (it was snapshotted before the per-job pre-warm). Skip it.
        //
        // The direct (non-batch) caller pre-warms, then captures its OWN
        // fixed view (AFTER the pre-warm so the capture observes the
        // pre-warmed overlay candidates): downstream, the warm probe, the
        // extraction context, the publish fence, AND the pinned resolve
        // executor all derive from ONE capture — never a per-stage fresh
        // read that a concurrent mutation could tear apart.
        let captured_fixed;
        let fixed = match fixed {
            Some(fixed) => fixed,
            None => {
                crate::host_manage::overlay_priority::prewarm_view_overlays(self, view);
                captured_fixed = self.capture_batch_fixed_view(view);
                &captured_fixed
            }
        };

        // Try the view-aware warm cache fast path.
        if let Some(warm) =
            self.try_component_meta_cache_hit_with_view_inner(canonical.as_str(), view, fixed)
        {
            if let Some(started) = started {
                component_meta_debug(format!(
                    "get_component_meta_via_view owner={} warm-cache hit took {:?}",
                    canonical,
                    started.elapsed(),
                ));
            }
            return Some(warm);
        }

        // Arm the projection-budget fuse + the per-cold-compute completeness
        // rail across the FULL cold body (resolve AND the fallthrough
        // extract). Without this the view-aware path ran the fallthrough
        // extract context-free — the inner `resolve_component_meta_with_*`
        // install-if-none dropped before the extract, so the `[P0]` op-budget
        // fuse was inert here. Install-if-none (never install-always), so a
        // batch / outer-context caller keeps its context and the inner
        // resolve install no-ops. Matches the inner audited id source so the
        // resolve-phase audit is unchanged.
        let _view_budget_ctx_guard = self.install_request_budget_context_if_none(
            crate::meta_resolve::next_component_meta_audit_request_id(),
            canonical.as_str(),
            self.config.audit_timing_capture && self.config.audit_enabled,
        );

        // Cold build. The view's overlay content (when present) has
        // been pre-warmed into `FileArtifactStore` under the overlay's
        // content hash via `materialize_overlay_indexed_ready` above,
        // so resolver-tier reads through
        // [`SessionResolverContext`](crate::resolver_core::SessionResolverContext)
        // see the overlay. The view's hash is used to publish the
        // result so the cache slot is keyed under the overlay hash —
        // R20 multi-candidate isolation: two sessions with different
        // overlays admit distinct candidate slots in the resolved-
        // meta cache.
        //
        // Install `with_fact_tracer` outer scope so the materialiser
        // `observe` wiring accumulates a real `FactReadSet` that
        // becomes the candidate's `fact_dep_signature`. R24: tracer
        // installs on cold-path only; warm-hits returned above.
        //
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work. See `get_component_meta` above for the
        // race rationale — a `bump_project_generation_and_evict`
        // landing while this cold publish is in flight would otherwise
        // strand a stale-by-project-generation entry whose carrier
        // still validates on file-content terms.
        let validated_at_generation = self.project_type_store.current_project_generation();
        // Publish fence + cold-seed view from the ONE capture (batch-
        // supplied or the direct caller's own, above): its captured token +
        // currentness — the same single-snapshot the executor's promotion
        // fence gates on. The base token recheck covers any epoch /
        // generation / env / identity shift landing during this cold
        // window; the session overlay identity is fixed for the request
        // (`view` does not change mid-request), so the base token is the
        // sound recheck rail here too. A non-current capture declines
        // promotion.
        let seed_fence = ColdSeedFence {
            token: fixed.captured_validation_token(),
            current: fixed.is_current(),
        };
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
            self,
            fixed.cold_seed(),
            overlay,
        );
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let (_resolved, meta) = self.component_meta_via_view_cold(
            canonical.as_str(),
            view,
            fixed,
            ctx,
            &seed_fence,
            validated_at_generation,
        )?;

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta_via_view owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
    }

    /// The SHARED view-path cold body: resolve PINNED to the caller's
    /// captured fixed view, extract under the caller's request-bound
    /// `ctx`, and publish the analysis result to the shared cache under the
    /// caller's fence — returning BOTH the resolved state and the analysis
    /// so an output-bearing caller can materialize the envelope under the
    /// SAME still-alive `ctx` (invariant: the analysis-cache publish is
    /// INDEPENDENT of any later output-materialization outcome).
    ///
    /// `fixed` is REQUIRED: every caller derives `ctx` / `seed_fence` from
    /// ONE captured [`crate::resolver_store::BatchFixedView`] and the
    /// resolve executor pins to that same capture — an unpinned resolve
    /// here would open its own store view and could pair a fresh-view
    /// analysis with the capture-bound extraction/materialization context
    /// (the torn-result race).
    fn component_meta_via_view_cold(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        seed_fence: &ColdSeedFence,
        validated_at_generation: u64,
    ) -> Option<(
        crate::meta_resolve::ResolvedComponentMetaState,
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    )> {
        #[cfg(test)]
        run_cold_body_pre_resolve_hook();
        // Pin the request executor to the caller's fixed view (FENCED), so
        // the resolve shares the capture's snapshot rather than re-reading.
        let executor_fixed = {
            let (executor_view, executor_fp) = fixed.executor_fixed_view();
            Some((executor_view, executor_fp, fixed.is_current()))
        };
        let ((resolved_opt, meta_opt), read_set) = self.with_fact_tracer(|| {
            let resolved = match self.resolve_component_meta_with_view_and_fixed(
                canonical,
                crate::types::ProjectionMode::Expanded,
                view,
                executor_fixed,
            ) {
                Some(r) => r,
                None => return (None, None),
            };
            let extract = extract_component_meta_from_resolved(
                self, canonical, &resolved, true, // include_fallthrough
                ctx,
            );
            (
                Some(resolved),
                Some((extract.analysis, extract.completeness)),
            )
        });
        let resolved = resolved_opt?;
        let (meta, extract_completeness) = meta_opt?;
        // ONE merged admission signal: the resolve-phase completeness merged
        // with the whole-extract scope (macro-DTO read + fallthrough compute).
        let final_completeness = resolved.completeness.merge(extract_completeness);

        // Seal + admission decision — `publish_if_admissible` (by-value
        // fenced-serve consult + R20 finalise).
        self.publish_if_admissible(canonical, "view-aware path", read_set, |sig| {
            self.publish_component_meta_cache_entry_with_view(
                canonical,
                view,
                &resolved,
                meta.clone(),
                sig,
                validated_at_generation,
                seed_fence,
                final_completeness,
            );
        });
        Some((resolved, meta))
    }

    /// Canonical builder for the
    /// [`crate::component_meta_result_db::ComponentMetaResultKey`] slot
    /// key. Every warm-lookup and cold-publish site MUST route through
    /// this one builder so the R21 split env axes are sourced identically
    /// on both sides — the env hashes
    /// (`host_view_env_hashes_for(canonical)`) and
    /// `host_view_project_identity_for(canonical)` are view-independent
    /// (they key on the owning project, not file content), so a lookup
    /// and the publish that warmed it compute the same key and a
    /// content-unchanged owner warm-hits. The owner content version stays
    /// the value-side candidate discriminant (NOT a key field, R6).
    pub(crate) fn component_meta_result_key(
        &self,
        canonical: &str,
        options: &ComponentMetaOptions,
    ) -> crate::component_meta_result_db::ComponentMetaResultKey {
        let env = self.host_view_env_hashes_for(canonical);
        crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            options_fingerprint: component_meta_options_fingerprint(options),
            project_identity: self.host_view_project_identity_for(canonical),
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
        }
    }

    /// Look up the project-global final-result cache for the
    /// owner and return the warm payload only when its recorded fact
    /// signature revalidates against the live store view. Returns
    /// `None` on any miss, stale entry, or missing shallow state.
    fn try_component_meta_cache_hit(
        &self,
        canonical: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        let shallow = self.shallow_file_state(canonical)?;
        let owner_whole_hash = shallow.whole_hash;
        let key = self.component_meta_result_key(canonical, &ComponentMetaOptions::default());
        // Fact-precise validation is the sole cache oracle:
        // `ComponentMetaResultDb::get_with_view` validates the entry's
        // `read_set_signature.facts` against the live `HostStoreView`
        // and counts a warm hit only when validation passes and the
        // value is returned. The DB stores
        // `CachedComponentMetaResult { analysis, resolution_template, ... }`
        // so the with_resolution path can rehydrate without re-running
        // the cold resolver; the plain `get_component_meta` warm path
        // returns only the analysis projection.
        //
        // The warm validator accepts ONLY a `CurrentHostStoreView`: a
        // known-stale `StoreViewRead::ReturnOnly` (the manager could not
        // prove the snapshot current under sustained churn) is a cache
        // MISS — accounting it as one and returning `None` falls the caller
        // to the cold recompute path, which never false-validates a
        // superseded entry against an already-mutated dependency.
        let results = self.project_type_store.component_meta_results();
        let Some(current_view) = self.resolver_store_view_read().current() else {
            results.record_non_current_view_miss(self);
            return None;
        };
        let entry = results.get_with_view(self, &current_view, &key, owner_whole_hash)?;
        Some(entry.payload.analysis.clone())
    }

    /// View-aware warm-cache fast path for component-meta queries.
    ///
    /// Like [`try_component_meta_cache_hit`] but derives the owner
    /// content version (the candidate DISCRIMINANT — NOT the cache key,
    /// which is the content-free [`crate::component_meta_result_db::ComponentMetaResultKey`])
    /// from `view.content_hash_for(canonical)` instead of the base
    /// host's `shallow_file_state(canonical).whole_hash`. This is the
    /// R17 + R18 wiring: sessions construct an
    /// [`crate::session_view::SessionView`] over their overlay state
    /// and the consumer path consults it for the candidate discriminant,
    /// so two sessions with conflicting overlays admit distinct
    /// CANDIDATES in ONE content-free slot of the multi-candidate
    /// substrate.
    ///
    /// The `view.content_hash_for(canonical)` lookup increments
    /// `provenance.view_aware_cache_key_lookups`. A `None` return
    /// from the view falls through to the base host's
    /// `shallow_file_state` — but the increment fires either way so
    /// callers observe that the consumer path consulted the view.
    /// View-aware warm-cache probe against a caller-captured fixed view.
    ///
    /// The warm probe validates against the fixed view's PROVEN-CURRENT
    /// overlaid snapshot instead of taking a fresh
    /// `resolver_store_view_read()` — the per-job read elimination for the
    /// analysis path (the direct, non-batch caller captures its own fixed
    /// view once per call). A fixed view whose capture was non-current
    /// (`current_view() == None`) misses to cold, exactly as a fresh
    /// non-current read would.
    fn try_component_meta_cache_hit_with_view_inner(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.try_component_meta_cache_entry_with_view(canonical, view, fixed)
            .map(|cached| cached.analysis.clone())
    }

    /// Entry-returning core of the view-aware warm probe: the FULL cached
    /// payload (analysis + resolution template), for consumers that need
    /// more than the analysis projection (the output-bearing entries seed
    /// their resolution sidecar from the cached template without a
    /// rehydrate). Validation semantics identical to
    /// [`Self::try_component_meta_cache_hit_with_view_inner`].
    fn try_component_meta_cache_entry_with_view(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
    ) -> Option<std::sync::Arc<crate::component_meta_result_db::CachedComponentMetaResult>> {
        // Owner-canonical tombstone short-circuit: a canonical the
        // session deleted has no meaningful component-meta result and
        // must NOT collapse onto a base cache slot. `content_hash_for`
        // returns `None` for a tombstone, but the `or_else` fallback
        // below would then derive an `owner_whole_hash` from the base
        // host's `shallow_file_state` (still reporting pre-delete
        // content — a session delete is an overlay, it never mutates
        // the base host), keying the warm lookup at the base slot.
        // Reject before the fallback. The sole caller
        // (`get_component_meta_via_view`) already guards the owner
        // tombstone; this is defence-in-depth so the method honours
        // its own contract independent of the caller.
        if view.is_tombstoned(canonical) {
            return None;
        }
        self.provenance
            .view_aware_cache_key_lookups
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let owner_whole_hash = view.content_hash_for(canonical).or_else(|| {
            // View did not know about the canonical — fall back to
            // the base host's shallow file state. This branch covers
            // canonicals the session never touched.
            self.shallow_file_state(canonical).map(|s| s.whole_hash)
        })?;
        let key = self.component_meta_result_key(canonical, &ComponentMetaOptions::default());
        // Fact-precise validation is the sole cache oracle:
        // `ComponentMetaResultDb::get_with_view` validates the entry's
        // `read_set_signature.facts` against the resolver-tier
        // `HostStoreView` and counts a warm hit only when validation
        // passes and the value is returned.
        //
        // The validation view is OVERLAY-AWARE so a candidate whose
        // cross-file dep facts are pinned to BASE content is rejected
        // when the session overlays or tombstones a dep — the
        // overlay-aware per-canonical snapshots re-root parse /
        // derived-fact validators at the session's CURRENT content
        // identity. Without the overlay, a session that mutates a
        // dependency of an owner whose own whole-hash is unchanged
        // would return the stale base candidate.
        //
        // Currentness is preserved across the overlay (Q4): the overlay
        // re-roots per-canonical snapshots on a base view the manager
        // already proved current, so the overlaid view is current too. A
        // non-current base never reaches the overlay — the `current()`
        // miss-to-cold runs FIRST, so the overlay can never LAUNDER a
        // `ReturnOnly` base into a validating view.
        let results = self.project_type_store.component_meta_results();
        // The proven-current OVERLAY-AWARE view for validation. The capture
        // (`fixed.current_view()`) is ALREADY overlaid once by
        // `capture_batch_fixed_view` and shared across jobs, so it is used
        // directly with NO per-job copy-on-write (re-applying the overlay per
        // job was the O(N²) regression); the direct non-batch caller
        // captures its own fixed view once per call, so the same single-COW
        // discipline holds there. A non-current capture exposes no current
        // view and misses to cold, so the overlay can never LAUNDER a
        // non-current base into a validating view (the `current()` miss runs
        // FIRST).
        let current_view = match fixed.current_view() {
            Some(current) => current,
            None => {
                results.record_non_current_view_miss(self);
                return None;
            }
        };
        let entry = results.get_with_view(self, current_view, &key, owner_whole_hash)?;
        Some(std::sync::Arc::clone(&entry.payload))
    }

    /// Publish-fence token recheck.
    ///
    /// Returns `true` iff promotion is admissible, gating on TWO
    /// conditions:
    ///
    /// 1. **Seed currentness.** `seed.current` is the
    ///    [`crate::resolver_store::StoreViewRead`] currentness of the view
    ///    the cold compute actually ran under. A non-current
    ///    (`StoreViewRead::ReturnOnly`) seed means the manager could not
    ///    prove the snapshot current under sustained churn; its result is
    ///    return-only and MUST NOT warm the shared cache regardless of the
    ///    token recheck.
    /// 2. **No mid-flight external supersession.** `seed.token` is the
    ///    validation token DERIVED FROM the seed view itself
    ///    ([`crate::resolver_store::HostStoreView::validation_token`]) —
    ///    not a separately-sampled live token — so the fence rechecks the
    ///    exact snapshot the compute used. If an external content /
    ///    project / env / identity / overlay mutation landed mid-flight
    ///    the live token now externally supersedes it and the result is
    ///    return-only.
    ///
    /// The EXTERNAL-supersession check deliberately excludes the additive
    /// artifact / load generations: the cold compute
    /// legitimately publishes `IndexedReady` artifacts and loads
    /// its deps as its OWN work, so folding those would self-fence
    /// promotion. The `validated_at_generation` gate is a subset of this
    /// token; the token recheck additionally covers `store_view_epoch` +
    /// env + identity.
    ///
    /// On any decline the result is still returned to the caller (the
    /// promotion alone is dropped).
    fn validation_token_still_live(
        &self,
        seed: &ColdSeedFence,
        canonical: &str,
        path_label: &str,
    ) -> bool {
        // Test-only: force the fence to observe a superseded token once,
        // exercising the skip branch deterministically without a racing
        // mid-flight mutation.
        #[cfg(test)]
        if PUBLISH_FENCE_FORCE_SUPERSEDE.with(|c| {
            if c.get() {
                c.set(false);
                true
            } else {
                false
            }
        }) {
            return false;
        }
        // A non-current seed is never promotable: the manager could not
        // prove the snapshot the compute ran under was current, so the
        // result is return-only by the StoreViewRead contract — it must
        // not warm the shared cache even if the external token happens to
        // still match.
        if !seed.current {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                path = %path_label,
                "skipping component-meta cache promotion: cold compute seeded by a non-current store view",
            );
            return false;
        }
        // EXTERNAL-supersession check only — see the doc above for the
        // excluded additive dimensions. The token is the one derived from
        // the seed view, so this compares the exact snapshot the compute
        // used against the live host.
        if !seed
            .token
            .externally_superseded_by(&self.current_validation_token())
        {
            return true;
        }
        tracing::debug!(
            target: "verter::audit::record",
            file = %canonical,
            path = %path_label,
            "skipping component-meta cache promotion: validation token superseded mid-flight",
        );
        false
    }

    /// Seal a cold producer's tracer and run `publish` only when the
    /// result is admissible to the shared cache — the single admission
    /// decision point for the three component-meta cold producers.
    ///
    /// Two by-value refusals, consistent with every other admission
    /// point:
    ///
    /// - **Fenced serve.** A traced compute that consumed a FENCED
    ///   (ReturnOnly, `store_published == false`) `IndexedReady` serve
    ///   derived its payload from a superseded artifact while its fact
    ///   stamps are read from the LIVE state — an entry the read-side
    ///   fact rail cannot reject. The seed-fence token recheck inside
    ///   the publish only covers mutations landing AFTER the seed
    ///   capture; this consult refuses on the serve's own publication
    ///   status even when the token still matches.
    /// - **Signature overflow (R20).** A finalised signature exceeding
    ///   the cap is refused admission.
    ///
    /// On either refusal the producer still returns the freshly
    /// computed value to its caller (return-only semantics) — only the
    /// cache publish is skipped.
    pub(super) fn publish_if_admissible(
        &self,
        canonical: &str,
        path_label: &str,
        read_set: crate::resolver_core::FactReadSet,
        publish: impl FnOnce(Arc<[crate::resolver_core::FactVersionRef]>),
    ) {
        if read_set.non_cacheable_read_observed() {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                path = %path_label,
                "skipping component-meta cache promotion: cold compute consumed a fenced (ReturnOnly) IndexedReady serve",
            );
            return;
        }
        match read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                publish(fact_dep_signature);
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: fact-signature overflowed cap",
                );
            }
        }
    }

    /// Publish the cold-build result into the project-global
    /// final-result cache, keyed under the view's content hash for the
    /// owner.
    ///
    /// Mirror of [`publish_component_meta_cache_entry`] that consults
    /// the supplied [`crate::session_view::SessionView`] for the
    /// owner's content hash so sessions with conflicting overlays
    /// admit distinct multi-candidate slots. Falls through to the
    /// base host's `shallow_file_state` if the view does not know
    /// about the canonical.
    fn publish_component_meta_cache_entry_with_view(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        meta: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>,
        validated_at_generation: u64,
        seed_fence: &ColdSeedFence,
        final_completeness: crate::semantic_query::ResultCompleteness,
    ) {
        // Refuse a partial result on ONE merged signal. `final_completeness` is
        // `resolved.completeness.merge(extract_scope_completeness)`: the
        // resolve-phase completeness merged with the WHOLE-extract scope (the
        // pre-choke macro-DTO read + the fallthrough cold compute). It replaces
        // the former source-enumerated gate
        // (`resolved.synthesis_should_suppress || fallthrough_completeness`),
        // so no partiality source can escape by construction. The dropped
        // `synthesis_should_suppress` term is SUBSUMED: it is the bool
        // projection of `resolved.completeness` (`= completeness.is_partial()`,
        // `component_meta_result_db.rs`), already a merge operand here. This is
        // COMPUTE completeness — a `LowerBound` accepted-surface SHAPE with a
        // complete compute stays cacheable.
        if final_completeness.is_partial() {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion (view-aware path): partial result (merged extract+resolve completeness)",
            );
            return;
        }
        // Publish fence: a superseded or non-current snapshot never warms
        // the shared cache.
        if !self.validation_token_still_live(seed_fence, canonical, "view-aware path") {
            return;
        }
        let Some(whole_hash) = view
            .content_hash_for(canonical)
            .or_else(|| self.shallow_file_state(canonical).map(|s| s.whole_hash))
        else {
            return;
        };
        let key = self.component_meta_result_key(canonical, &ComponentMetaOptions::default());
        let resolution_template =
            crate::component_meta_result_db::ResolutionTemplate::from_resolved_state(resolved);
        let cached = crate::component_meta_result_db::CachedComponentMetaResult {
            analysis: meta,
            resolution_template,
            canonical_id: Arc::from(canonical),
            whole_hash,
        };
        // Drop the owner's own non-round-tripping `DerivedFactHash{Route}`
        // fact before admission (see `strip_owner_route_fact`). Cross-file
        // route facts and the owner `FileWholeHash` fact are retained.
        let admitted_signature = strip_owner_route_fact(canonical, &fact_dep_signature);
        self.project_type_store.component_meta_results().insert(
            key,
            whole_hash,
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(cached),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                    admitted_signature,
                ),
                validated_at_generation,
            },
        );
    }

    /// Publish the cold-build result into the project-global
    /// final-result cache. The recorded fact signature carries every
    /// transitive file fact the resolver observed while producing the
    /// result. A later lookup revalidates the full fact signature
    /// against the live store view so an edit to *any* file the
    /// resolver touched invalidates the cached payload — not just edits
    /// to the owner itself.
    ///
    /// **Suppression gate.** When graph-native slot-binding synthesis
    /// observed a fatal `QueryError` (`BudgetExceeded`,
    /// `UnstableState`, walker `cache_suppress`),
    /// `resolved.synthesis_should_suppress` is `true` and the
    /// final-result cache write is skipped. Subsequent requests
    /// cold-recompute. The synthesis output remains available to the
    /// caller so partial diagnostics still surface — only the cache
    /// promotion is gated.
    pub(super) fn publish_component_meta_cache_entry(
        &self,
        canonical: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        meta: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>,
        validated_at_generation: u64,
        seed_fence: &ColdSeedFence,
        final_completeness: crate::semantic_query::ResultCompleteness,
    ) {
        // Refuse a partial result on ONE merged signal. `final_completeness` is
        // `resolved.completeness.merge(extract_scope_completeness)`: the
        // resolve-phase completeness merged with the WHOLE-extract scope (the
        // pre-choke macro-DTO read + the fallthrough cold compute). It replaces
        // the former source-enumerated gate
        // (`resolved.synthesis_should_suppress || fallthrough_completeness`),
        // so no partiality source can escape by construction. The dropped
        // `synthesis_should_suppress` term is SUBSUMED: it is the bool
        // projection of `resolved.completeness` (`= completeness.is_partial()`,
        // `component_meta_result_db.rs`), already a merge operand here. This is
        // COMPUTE completeness — a `LowerBound` accepted-surface SHAPE with a
        // complete compute stays cacheable.
        if final_completeness.is_partial() {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion: partial result (merged extract+resolve completeness)",
            );
            return;
        }
        // Publish fence: recheck the seed view's validation token against
        // the live host immediately before promotion, AND decline a
        // non-current seed. A token mismatch means the snapshot the cold
        // compute ran under was superseded mid-flight (epoch / generation
        // / env / identity changed); a non-current seed means the manager
        // never proved the snapshot current. Either skips the promotion so
        // a torn-from-a-stale-or-unprovable-snapshot result never warms
        // the shared cache. The result is still returned to the caller
        // (return-only semantics).
        if !self.validation_token_still_live(seed_fence, canonical, "base path") {
            return;
        }
        let Some(shallow) = self.shallow_file_state(canonical) else {
            return;
        };
        let whole_hash = shallow.whole_hash;
        let key = self.component_meta_result_key(canonical, &ComponentMetaOptions::default());
        let resolution_template =
            crate::component_meta_result_db::ResolutionTemplate::from_resolved_state(resolved);
        let cached = crate::component_meta_result_db::CachedComponentMetaResult {
            analysis: meta,
            resolution_template,
            canonical_id: Arc::from(canonical),
            whole_hash,
        };
        // Drop the owner's own non-round-tripping `DerivedFactHash{Route}`
        // fact before admission (see `strip_owner_route_fact`). Cross-file
        // route facts and the owner `FileWholeHash` fact are retained.
        let admitted_signature = strip_owner_route_fact(canonical, &fact_dep_signature);
        self.project_type_store.component_meta_results().insert(
            key,
            whole_hash,
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(cached),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                    admitted_signature,
                ),
                validated_at_generation,
            },
        );
    }

    /// Monotonic request-id generator. Starts at 1; zero is reserved
    /// for "not populated" (see `ResolvedComponentMetaState::request_id`).
    pub(crate) fn next_request_id(&self) -> u64 {
        use std::sync::atomic::Ordering;
        self.request_id_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Drain the `RequestAuditRecord` matching `request_id` from the host's
    /// bounded audit-record store. Returns `None` when the record was
    /// never inserted (capture disabled) or already drained by a prior
    /// `take_audit_record` call.
    pub fn take_audit_record(
        &self,
        request_id: u64,
    ) -> Option<crate::component_meta_audit::RequestAuditRecord> {
        self.audit_records.take(request_id)
    }

    /// Finalise a finished audit record through the
    /// [`crate::host_audit_runtime::AuditRequestRegistration`] planted
    /// on the active [`crate::request_context::RequestContext`]. The
    /// registration removes the in-flight slot from the host's
    /// active-request registry and inserts the record into the
    /// records store.
    ///
    /// When no registration is installed (the active context predates
    /// the audited entry-point or no context is in scope at all), the
    /// record is inserted directly so the host-wide store stays
    /// consistent. This branch covers code paths that bypass the
    /// public audited entry-point — e.g. tests that drive
    /// `resolve_component_meta` without first installing a
    /// registration, or callers that go through the lower-level
    /// `ComponentMetaSession::get_component_meta` API on an
    /// audit-enabled host. The fallback never touches the
    /// active-request registry; only the records store is
    /// populated.
    pub fn finalize_request_audit_record(
        &self,
        record: crate::component_meta_audit::RequestAuditRecord,
    ) {
        if let Some(ctx) = crate::request_context::current_request_context() {
            if let Some(registration) = ctx.audit_registration.get() {
                registration.finalize(record);
                return;
            }
        }
        self.audit_records.insert(record);
    }

    /// Selective surface API (D32 / D102) — host-level entry point.
    ///
    /// Convenience wrapper that combines [`Self::get_component_meta_with_resolution`]
    /// with [`crate::component_meta_payload::assemble_surface_from_analysis`] so
    /// host-only consumers (LSP, MCP, bundler) can request the surface
    /// envelope without holding a `MetaSession`. Returns `None` when the
    /// canonical does not resolve to a component.
    pub fn get_component_meta_surface(
        &self,
        canonical_or_alias: &str,
    ) -> Option<crate::component_meta_payload::ComponentMetaSurface> {
        let (analysis, _resolution) =
            self.get_component_meta_with_resolution(canonical_or_alias)?;
        Some(crate::component_meta_payload::assemble_surface_from_analysis(&analysis))
    }

    /// Selective type-expansion API (D32 / D104) — host-level entry point.
    ///
    /// Resolves a `TypeHandle` to a one-layer `TypeExpansion`. Errors are
    /// typed (D104 + D114): `ProjectMismatch` when the handle's project_id
    /// does not match the host's project; `StaleHandle` when the canonical
    /// file is no longer readable.
    pub fn get_component_meta_type_expansion(
        &self,
        handle: crate::component_meta_payload::TypeHandle,
        depth: Option<usize>,
    ) -> Result<
        crate::component_meta_payload::TypeExpansion,
        crate::component_meta_payload::TypeHandleError,
    > {
        crate::component_meta_payload::resolve_type_expansion(self, handle, depth)
    }
}

#[cfg(test)]
#[path = "component_meta_entry_tests.rs"]
mod component_meta_entry_tests;
