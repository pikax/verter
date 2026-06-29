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
pub(super) struct ColdSeedFence {
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
            let (meta, fallthrough_completeness) = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
                ctx,
            );
            (Some(resolved), Some((meta, fallthrough_completeness)))
        });
        let resolved = resolved_opt?;
        let (meta, fallthrough_completeness) = meta_opt?;

        // Seal + admission decision (the by-value fenced-serve consult
        // + the R20 finalise) live in ONE place — `publish_if_admissible`.
        self.publish_if_admissible(canonical.as_str(), "base path", read_set, |sig| {
            self.publish_component_meta_cache_entry(
                canonical.as_str(),
                &resolved,
                meta.clone(),
                sig,
                validated_at_generation,
                &seed_fence,
                fallthrough_completeness,
            );
        });

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
            let (meta, fallthrough_completeness) = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
                ctx,
            );
            (Some(resolved), Some((meta, fallthrough_completeness)))
        });
        let resolved = resolved_opt?;
        let (meta, fallthrough_completeness) = meta_opt?;

        // Seal + admission decision — `publish_if_admissible` (by-value
        // fenced-serve consult + R20 finalise).
        self.publish_if_admissible(canonical.as_str(), "view-aware path", read_set, |sig| {
            self.publish_component_meta_cache_entry_with_view(
                canonical.as_str(),
                view,
                &resolved,
                meta.clone(),
                sig,
                validated_at_generation,
                &seed_fence,
                fallthrough_completeness,
            );
        });

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta_via_view owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
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
    pub(super) fn cold_seed_view_and_fence(
        &self,
    ) -> (crate::resolver_store::ColdSeedHostStoreView, ColdSeedFence) {
        let read = self.resolver_store_view_read();
        let current = read.is_current_for_promotion();
        let view = read.into_cold_seed_view();
        let token = view.view().validation_token();
        (view, ColdSeedFence { token, current })
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
        if read_set.fenced_serve_observed() {
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
        fallthrough_completeness: crate::semantic_query::ResultCompleteness,
    ) {
        // Refuse a partial result: `resolved.synthesis_should_suppress` covers
        // the resolve step, and `fallthrough_completeness` covers the
        // fallthrough cold compute (which runs AFTER `resolved` was produced, so
        // a fallthrough-only partial would otherwise escape the resolve-time
        // signal). This is COMPUTE completeness — a `LowerBound` accepted-surface
        // SHAPE with a complete compute stays cacheable.
        if resolved.synthesis_should_suppress || fallthrough_completeness.is_partial() {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion (view-aware path): partial result (resolve or fallthrough)",
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
        fallthrough_completeness: crate::semantic_query::ResultCompleteness,
    ) {
        // Refuse a partial result: `resolved.synthesis_should_suppress` covers
        // the resolve step, and `fallthrough_completeness` covers the
        // fallthrough cold compute (which runs AFTER `resolved` was produced, so
        // a fallthrough-only partial would otherwise escape the resolve-time
        // signal). This is COMPUTE completeness — a `LowerBound` accepted-surface
        // SHAPE with a complete compute stays cacheable.
        if resolved.synthesis_should_suppress || fallthrough_completeness.is_partial() {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion: partial result (resolve or fallthrough)",
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
