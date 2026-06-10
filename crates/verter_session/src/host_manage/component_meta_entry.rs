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

/// The publish fence a cold component-meta compute must pass before its
/// result may warm the shared cache.
///
/// Both fields are derived from ONE store-view read (see
/// [`VerterHost::cold_seed_view_and_fence`]): `token` is reconstructed
/// from the seed view itself and `current` is the
/// [`crate::resolver_store::StoreViewRead`] arm, so neither can describe
/// a different snapshot than the one the cold compute ran under.
#[derive(Debug, Clone, Copy)]
struct ColdSeedFence {
    /// The validation token of the exact view the cold compute used.
    token: crate::resolver_store::StoreViewValidationToken,
    /// Whether the manager proved that view current. A non-current
    /// (`ReturnOnly`) seed is never promoted.
    current: bool,
}

/// Strip the OWNER's own `DerivedFactHash { kind: Route }` fact from a
/// `ComponentMetaResultEntry` signature before cache admission.
///
/// **Why exactly this one fact.** The owner's `Route` hash is the only
/// fact in the tracer-owned signature that does NOT round-trip through
/// warm validation. `HostStoreView::build` populates
/// `view.derived_hashes[(owner, Route)]` from TWO sources — the
/// owner's `IndexedReady.shallow_state` AND the
/// `route_owned_shallow_cache` — and the route-owned source overwrites
/// the indexed source when both are present (see
/// `resolver_store.rs` `HostStoreView::build`). When the owner already
/// has a `route_owned_shallow` entry from an earlier route-only read,
/// the cold component-meta compute's route walk observes the owner's
/// Route fact with the *indexed* hash, but a later warm-hit validation
/// reads the *route-owned* hash. The two disagree even with no edit,
/// so the warm hit misses and the query cold-recomputes every time —
/// a steady-state warm-cache miss / perf regression.
///
/// The filter is deliberately narrow:
///
/// - Only `kind == Route` is dropped. `ImportRoute` and `DirectSource`
///   derived facts round-trip and stay.
/// - Only the OWNER's own Route fact is dropped (`canonical_id ==
///   owner_canonical`). Cross-file route facts — Route facts for the
///   route DEPS the cold compute walked — round-trip correctly (a dep
///   does not race a route-owned-shallow build during the owner's cold
///   compute) and MUST stay so an edit to a route dep still
///   invalidates the owner's warm hit.
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

        let _ctx_guard = if crate::request_context::current_request_context().is_none() {
            Some(crate::request_context::RequestContextGuard::install(
                crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
                    self.next_request_id(),
                    std::sync::Arc::<str>::from(canonical.as_str()),
                    verter_audit::RequestKind::ComponentMeta,
                    false,
                    false,
                    None,
                    self.config.projection_op_budget,
                ),
            ))
        } else {
            None
        };

        // Cold build under the existing `with_fact_tracer` scope.
        // The tracer continues to fan observations into any outer
        // scope (R24 fan-out). The FINALISED tracer read set is the
        // authoritative `fact_dep_signature` source: it records the
        // exact, deduplicated union of every cross-file fact the cold
        // compute observed — dispatch dual-emit `FileWholeHash` facts,
        // resolver-tier `Parse` / `ResolveImports` / `RouteSurface`
        // facts, and every sub-cache's bubbled signature. The curated
        // `resolved.fact_versions` is NOT consulted for the published
        // signature. The single fact the tracer-owned signature CAN
        // carry that does not round-trip on warm validation is the
        // owner's OWN `DerivedFactHash{Route}` — the cold compute's
        // macro-root route walk observes it whenever the owner is a
        // route participant — so `publish_component_meta_cache_entry`
        // drops exactly that fact before cache admission (see
        // `strip_owner_route_fact`). Cross-file route facts round-trip
        // and are retained.
        //
        // R24 contract: the tracer is installed on COLD paths only.
        // The warm-hit fast path above returned before reaching
        // here, so no tracer is installed for hot reads (zero
        // allocation per hit).
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
        // Publish fence + cold-seed view in ONE read. The fence token is
        // derived from the seed view itself and the currentness is the
        // `StoreViewRead` arm, so the snapshot the publish fence rechecks
        // and the snapshot the cold compute runs under cannot diverge. A
        // mismatch against the live host token at promotion time (epoch /
        // generation / env / identity / overlay shifted mid-flight), OR a
        // non-current seed, routes to return-only — the result is handed
        // to the caller but never promoted, so a superseded or
        // unprovable snapshot can never warm the shared cache. The
        // `validated_at_generation` gate above is a SUBSET of the token
        // recheck.
        //
        // The seed view is also threaded into the `HostResolverContext`
        // BEFORE `with_fact_tracer` opens so the extract step (which runs
        // under the tracer) binds its engine constructions to the same
        // overlay-aware ctx the inner `resolve_component_meta_with_view`
        // builds — without this the extract path would construct bare-host
        // engines under the tracer, inflating `store_merge_ms` +
        // `prepared_type_decls` per request.
        let (store_view, seed_fence) = self.cold_seed_view_and_fence();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &store_view, overlay);
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let ((resolved_opt, meta_opt), read_set) = self.with_fact_tracer(|| {
            let resolved = match self
                .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
            {
                Some(r) => r,
                None => return (None, None),
            };
            let meta = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
                ctx,
            );
            (Some(resolved), Some(meta))
        });
        let resolved = resolved_opt?;
        let meta = meta_opt?;

        // Finalise the tracer (R20). On `Ok` the returned
        // `Arc<[FactVersionRef]>` is the tracer-owned signature; on
        // `Overflow` the signature exceeded the cap and cache
        // admission is refused.
        match read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                self.publish_component_meta_cache_entry(
                    canonical.as_str(),
                    &resolved,
                    meta.clone(),
                    fact_dep_signature,
                    validated_at_generation,
                    &seed_fence,
                );
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    "skipping component-meta cache promotion: fact-signature overflowed cap",
                );
            }
        };

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
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
        if fixed.is_none() {
            crate::host_manage::overlay_priority::prewarm_view_overlays(self, view);
        }

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
        // Publish fence + cold-seed view from ONE read — see
        // `get_component_meta` above for the divergence-free derivation.
        // The base token recheck covers any epoch / generation / env /
        // identity shift landing during this cold window; the session
        // overlay identity is fixed for the request (`view` does not
        // change mid-request), so the base token is the sound recheck
        // rail here too. A non-current base seed declines promotion.
        //
        // With a caller-captured fixed view, the cold-seed + fence come
        // from that ONE batch capture (its captured token + currentness)
        // instead of a fresh per-job read — the same single-snapshot the
        // executor's promotion fence gates on. Without one, take a fresh
        // read here.
        let (store_view, seed_fence) = match fixed {
            Some(fixed) => (
                fixed.cold_seed().clone(),
                ColdSeedFence {
                    token: fixed.captured_validation_token(),
                    current: fixed.is_current(),
                },
            ),
            None => self.cold_seed_view_and_fence(),
        };
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &store_view, overlay);
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        // Pin the request executor to the same fixed view (FENCED) when one
        // was supplied, so the resolved-meta promotion shares the batch
        // snapshot rather than re-reading per job.
        let executor_fixed = fixed.map(|fixed| {
            let (view, fp) = fixed.executor_fixed_view();
            (view, fp, fixed.is_current())
        });
        let ((resolved_opt, meta_opt), read_set) = self.with_fact_tracer(|| {
            let resolved = match self.resolve_component_meta_with_view_and_fixed(
                canonical.as_str(),
                crate::types::ProjectionMode::Expanded,
                view,
                executor_fixed,
            ) {
                Some(r) => r,
                None => return (None, None),
            };
            let meta = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
                ctx,
            );
            (Some(resolved), Some(meta))
        });
        let resolved = resolved_opt?;
        let meta = meta_opt?;

        // Finalise the tracer (R20). The `Ok` payload is the
        // tracer-owned signature — the authoritative cross-file
        // dependency set (see the base `get_component_meta` path
        // above for the source rationale).
        match read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                self.publish_component_meta_cache_entry_with_view(
                    canonical.as_str(),
                    view,
                    &resolved,
                    meta.clone(),
                    fact_dep_signature,
                    validated_at_generation,
                    &seed_fence,
                );
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    "skipping component-meta cache promotion (view-aware path): fact-signature overflowed cap",
                );
            }
        };

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta_via_view owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
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
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
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
    /// Like [`try_component_meta_cache_hit`] but derives the cache key
    /// from `view.content_hash_for(canonical)` instead of the base
    /// host's `shallow_file_state(canonical).whole_hash`. This is the
    /// R17 + R18 wiring: sessions construct an
    /// [`crate::session_view::SessionView`] over their overlay state
    /// and the consumer path consults it for cache-key derivation, so
    /// two sessions with conflicting overlays admit distinct cache
    /// slots in the multi-candidate substrate.
    ///
    /// The `view.content_hash_for(canonical)` lookup increments
    /// `provenance.view_aware_cache_key_lookups`. A `None` return
    /// from the view falls through to the base host's
    /// `shallow_file_state` — but the increment fires either way so
    /// callers observe that the consumer path consulted the view.
    /// View-aware warm-cache probe with an optional caller-captured fixed
    /// view.
    ///
    /// When `fixed` is `Some`, the warm probe validates against the fixed
    /// view's PROVEN-CURRENT base snapshot (overlay-re-rooted for `view`)
    /// instead of taking a fresh `resolver_store_view_read()` — the
    /// per-job read elimination for the analysis path. A fixed view whose
    /// capture was non-current (`current_view() == None`) misses to cold,
    /// exactly as a fresh non-current read would. `None` takes a fresh
    /// read (the direct, non-batch caller).
    fn try_component_meta_cache_hit_with_view_inner(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
        fixed: Option<&crate::resolver_store::BatchFixedView>,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
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
        // The proven-current OVERLAY-AWARE view for validation. The batch
        // path's `fixed.current_view()` is ALREADY overlaid once by
        // `capture_batch_fixed_view` and shared across jobs, so it is used
        // directly with NO per-job copy-on-write (re-applying the overlay per
        // job was the O(N²) regression); the direct non-batch caller has no
        // shared capture, so its fresh read is overlaid HERE, once. A
        // non-current capture / fresh `ReturnOnly` read exposes no current
        // view and misses to cold, so the overlay can never LAUNDER a
        // non-current base into a validating view (the `current()` miss runs
        // FIRST).
        let current_view = match fixed {
            Some(fixed) => match fixed.current_view() {
                Some(current) => current.clone(),
                None => {
                    results.record_non_current_view_miss(self);
                    return None;
                }
            },
            None => match self.resolver_store_view_read().current() {
                Some(current) => current.with_session_overlay(self, view),
                None => {
                    results.record_non_current_view_miss(self);
                    return None;
                }
            },
        };
        let entry = results.get_with_view(self, &current_view, &key, owner_whole_hash)?;
        Some(entry.payload.analysis.clone())
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
    /// artifact / route-owned / load generations: the cold compute
    /// legitimately publishes indexed / route-owned artifacts and loads
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

    /// Read the host base store view ONCE for a cold component-meta
    /// compute and bundle the seed view with the publish fence it must be
    /// validated under.
    ///
    /// The fence token is derived from the returned view itself
    /// ([`crate::resolver_store::HostStoreView::validation_token`]), and
    /// the currentness is the [`crate::resolver_store::StoreViewRead`]
    /// arm — so the token the publish fence rechecks and the view the
    /// cold compute runs under CANNOT diverge (they are the same read).
    /// This is the structural fix for the prior pattern that sampled a
    /// live token BEFORE the seed-view read: a mutation landing between
    /// the two reads could either refuse a fresh result or, worse, let a
    /// stale `ReturnOnly` seed promote against a still-matching pre-read
    /// sample.
    ///
    /// A non-current (`ReturnOnly`) read is still returned as a usable
    /// cold-seed view (the cold compute's own coherence still runs), but
    /// `ColdSeedFence::current` is `false` so
    /// [`Self::validation_token_still_live`] declines promotion.
    fn cold_seed_view_and_fence(
        &self,
    ) -> (crate::resolver_store::ColdSeedHostStoreView, ColdSeedFence) {
        let read = self.resolver_store_view_read();
        let current = read.is_current_for_promotion();
        let view = read.into_cold_seed_view();
        let token = view.view().validation_token();
        (view, ColdSeedFence { token, current })
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
    ) {
        if resolved.synthesis_should_suppress {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion (view-aware path): synthesis_should_suppress=true",
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
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
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
    fn publish_component_meta_cache_entry(
        &self,
        canonical: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        meta: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>,
        validated_at_generation: u64,
        seed_fence: &ColdSeedFence,
    ) {
        if resolved.synthesis_should_suppress {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion: synthesis_should_suppress=true",
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
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
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

        // Finalise the tracer (R20). The `Ok` payload is the
        // tracer-owned signature — the authoritative cross-file
        // dependency set captured during the cold compute.
        match read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                // Cache-write so subsequent identical calls
                // short-circuit through `try_with_resolution_cache_hit`.
                // Suppression is enforced inside `publish_component_meta_cache_entry`
                // via `resolved.synthesis_should_suppress`.
                self.publish_component_meta_cache_entry(
                    canonical.as_str(),
                    &resolved,
                    analysis.clone(),
                    fact_dep_signature,
                    validated_at_generation,
                    &seed_fence,
                );
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    "skipping component-meta cache promotion (with-resolution path): fact-signature overflowed cap",
                );
            }
        };

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
