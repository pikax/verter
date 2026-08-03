//! `host_manage::prepared_decl` — fact-validated `PreparedDeclBundle`
//! materialisation, shallow-file-state lookup, and import-route resolution
//! used by the resolver / engine layers.
//!
//! Domain F. Owns the largest single block of
//! cache-discipline code in `host_manage`: the bundle materialiser, the
//! prepared-decl freshness gate, the imported-symbol dependency walker,
//! the indexed-ready upsert path, and the owner-direct-import surface.
//! Public surface remains rooted at `crate::host_manage::*`; this file
//! contributes a continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use verter_semantic::analysis::script_shallow_index::build_script_shallow_index_with_owners;

use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
    is_raw_import_specifier_id,
};

/// An `IndexedReady` serve plus its publication status — the value-flow
/// carrier for the ReturnOnly discriminant of
/// [`VerterHost::ensure_indexed_ready_serve`].
///
/// `store_published == true` means the artifact is the store-current
/// surface (a warm hit, or a flight whose pre-publish fence passed and
/// whose insert landed). `store_published == false` means the artifact
/// was served ReturnOnly from a FENCED flight: a workspace or
/// route-resolution mutation landed mid-flight, the artifact was built
/// against superseded state and published NOTHING. A ReturnOnly serve is
/// valid for the requesting caller's read, but any derived value that
/// would enter a SHARED cache (e.g. a `PreparedDeclBundle`) must consult
/// this flag and decline admission — the derived value's fact stamps are
/// computed from the LIVE post-mutation state while its payload was
/// computed FROM the superseded artifact, an entry the read-side fact
/// rail cannot reject. The flag flows by VALUE, so the gate works with
/// or without an installed `RequestContext`. A fenced serve ALSO fans the
/// generalized non-cacheability rail (`note_non_cacheable_read_fan_out`)
/// onto every active fact tracer, so enclosing traced cold computes (the
/// semantic-memo build, the component-meta admission gate) refuse warm
/// admission independently. The request-sticky `mark_request_result_partial`
/// channel is partiality-ONLY and no longer carries fences — see
/// `request_context::observe_component_meta_read_suppress` for why
/// shared-cache gates must key on the fact-tracer / `cache_suppress`
/// channel, never the request-coarse partial sticky.
#[derive(Clone)]
pub(crate) struct IndexedReadyServe {
    pub(crate) indexed: Arc<crate::project_type_store::IndexedReady>,
    pub(crate) store_published: bool,
}

/// Outcome of one prepared-decl-bundle cold producer
/// ([`VerterHost::materialize_prepared_decl_bundle_from_routed_shallow`] /
/// [`VerterHost::materialize_prepared_decl_bundle`]). Both arms carry the
/// consumed serve's publication status BY VALUE so the bundle flight
/// lane can decide retention: a fenced-derived value — a built bundle
/// served without admission, OR a miss concluded from a FENCED serve's
/// surface-emptiness — must never be retained as a joinable rendezvous.
enum BundleMaterialization {
    /// A bundle was built. `admitted == true` means it was inserted
    /// into the shared `prepared_decl_bundles` cache; `false` means it
    /// was built from a FENCED (ReturnOnly) serve and served WITHOUT
    /// admission.
    Built {
        bundle: Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>,
        admitted: bool,
    },
    /// The consumed serve's surface was empty — no bundle to build.
    /// `serve_published` is the serve's publication status: a FENCED
    /// serve's emptiness describes a superseded artifact, not live
    /// content, so the resulting miss is NOT reproducible.
    SurfaceEmpty { serve_published: bool },
}

impl VerterHost {
    // -----------------------------------------------------------------------
    // Fact-validated PreparedDeclBundle cache
    // -----------------------------------------------------------------------

    /// Look up (or materialize) the fact-validated prepared-decl bundle for a
    /// canonical file.  On a warm read the cost is O(facts.len()) — no
    /// dependency-resolution or route-refresh work is performed.
    ///
    /// Builds a fresh `HostStoreView` at every call. Production
    /// resolver-tier code on the per-component-meta hot path MUST use
    /// [`Self::prepared_decl_bundle_with_store_view`] instead so the view
    /// is built ONCE at the request boundary and threaded down (per the
    /// per-request hoist). This entry point survives for
    /// integration tests + the test-only arm on `impl ResolverContext
    /// for VerterHost::prepared_decl_bundle` — production callers go
    /// through `ctx.prepared_decl_bundle` (which routes through
    /// `_with_store_view`).
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        // Route through a cold-seed `HostResolverContext` so the warm-cache
        // probe reads through the cold-seed-aware `RequestStoreView`: a
        // known-stale (`ReturnOnly`) read fails the warm validation closed
        // and the bundle materialises cold, never validating a warm entry
        // against a superseded snapshot.
        let view = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx = crate::resolver_core::HostResolverContext::from_cold_seed(self, &view, overlay);
        self.prepared_decl_bundle_with_context(&ctx, canonical_id)
    }

    /// Attribute a prepared-decl bundle warm-read rejection to one of
    /// the five `PreparedDeclBundleReject*` audit counters.
    ///
    /// Inspects `rejected_fact` (the first fact that failed validation
    /// in the most-recent candidate, as returned by
    /// [`crate::resolver_core::ValidatedFactCache::get_if_valid_self_rooted_attributed`])
    /// and consults [`crate::resolver_core::StoreView::tracks_file`] for the
    /// self-root arm to determine WHICH check rejected. Fires
    /// exactly one audit event per call:
    ///
    /// * `PreparedDeclBundleRejectEntryMissing` — `rejected_fact ==
    ///   None && candidate_count == 0` (no cache entry at all).
    /// * `PreparedDeclBundleRejectSelfRootUntracked` — `FileWholeHash`
    ///   self-root, `view.tracks_file(canonical)` is `false`.
    /// * `PreparedDeclBundleRejectSelfRootHashMismatch` —
    ///   `FileWholeHash` self-root, tracked but stored hash differs.
    /// * `PreparedDeclBundleRejectImportRouteMismatch` — historical audit
    ///   event name retained for a moved path-precise resolution witness.
    /// * `PreparedDeclBundleRejectOther` — fallthrough; must stay 0
    ///   in steady state.
    fn attribute_prepared_decl_bundle_rejection(
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
        rejected_fact: Option<&crate::resolver_core::FactVersionRef>,
        candidate_count: usize,
    ) {
        let Some(obs) = verter_audit::current_observer() else {
            return;
        };
        let event = match rejected_fact {
            None if candidate_count == 0 => {
                verter_audit::AuditEvent::PreparedDeclBundleRejectEntryMissing
            }
            Some(crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: fact_canonical,
                ..
            }) if fact_canonical == canonical_id => {
                if view.tracks_file(fact_canonical) {
                    verter_audit::AuditEvent::PreparedDeclBundleRejectSelfRootHashMismatch
                } else {
                    verter_audit::AuditEvent::PreparedDeclBundleRejectSelfRootUntracked
                }
            }
            Some(crate::resolver_core::FactVersionRef::ResolveImports(fact))
                if fact.resolution_fact().is_some() =>
            {
                // The owner's import-route resolution witness moved: the
                // observed resolver input (a path probe, a realpath, an
                // exact-resolution row) no longer carries the version the
                // bundle recorded. The absent/mismatch distinction the
                // legacy digest drew does not exist on this rail — a
                // resolution fact either validates against the view's
                // captured world or it does not.
                let _ = view;
                verter_audit::AuditEvent::PreparedDeclBundleRejectImportRouteMismatch
            }
            _ => verter_audit::AuditEvent::PreparedDeclBundleRejectOther,
        };
        obs.record_event(event);
    }

    /// View-bound variant of [`Self::prepared_decl_bundle`].
    ///
    /// `view` is a borrow into the request-bound [`HostStoreView`] built
    /// at the request entry point. The warm-hit path validates against
    /// this view instead of requesting another root capture.
    ///
    /// Same strict self-root validation contract as
    /// [`Self::prepared_decl_bundle`]: a deleted (now-untracked) keyed
    /// canonical rejects the stale bundle.
    pub(crate) fn prepared_decl_bundle_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Fast path: fact-validated cache hit. The bundle's keyed
        // canonical is its self-root — validated **strictly** so a
        // deleted (now-untracked) keyed file rejects the stale bundle
        // instead of riding the lazy untracked-accept rule.
        //
        // On a rejection the attributed sibling returns the FIRST
        // rejected fact from the most-recent candidate; we feed it
        // to `attribute_prepared_decl_bundle_rejection` so the
        // matching per-cause audit counter fires (one of the five
        // `PreparedDeclBundleReject*` variants).
        let bundles = &self.resolver.runtime.prepared_decl_bundles;
        let key = canonical_id.to_string();
        match bundles.get_if_valid_self_rooted_attributed(&key, view, &[canonical_id]) {
            Ok(bundle) => {
                self.provenance
                    .bundle_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Per-request audit attribution: prepared-decl bundle
                // served from cache (no materialisation).
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(verter_audit::AuditEvent::PreparedDeclBundleWarm);
                }
                return Some(bundle);
            }
            Err((rejected_fact, candidate_count)) => {
                Self::attribute_prepared_decl_bundle_rejection(
                    view,
                    canonical_id,
                    rejected_fact.as_ref(),
                    candidate_count,
                );
            }
        }

        // Cold path with singleflight: coalesce concurrent materializations
        // for the same canonical_id + store-view compat token.
        let token = view.compat_token();
        let singleflight = bundles.singleflight();
        let flight_body = || {
            // Re-check cache inside the singleflight leader closure (another
            // thread may have populated it between our first check and winning
            // the flight). Strict self-root validation on the keyed canonical.
            // Re-check skips the rejection-attribution call: the per-cause
            // counter already fired on the outer fast-path miss; a recheck
            // miss attribution would double-count the same logical rejection.
            if let Some(bundle) = bundles.get_if_valid_self_rooted(&key, view, &[canonical_id]) {
                return Ok(crate::resolver_core::StableExecutionValue {
                    value: Some((*bundle).clone()),
                    stable: true,
                    // Served from the bundle cache inside the flight — no
                    // cold materialisation performed.
                    computed: false,
                    // Prepared-decl bundles gate on `stable`/`admitted`, not
                    // a partial-completeness lattice — a served bundle is
                    // complete.
                    completeness: crate::semantic_query::ResultCompleteness::Complete,
                    cache_refusal: None,
                });
            }
            // Per-request audit attribution: cold materialisation of
            // the prepared-decl bundle. Bumped once per cold run —
            // joiners that block on the leader do not count; a
            // fenced-retry re-run counts again (it genuinely re-runs
            // the cold build).
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::PreparedDeclBundleCold);
            }
            // Deterministic mirror of the audit event above: counts a
            // genuine cold flight-body run regardless of outcome (Built
            // OR surface-empty miss) — the observable that separates a
            // burst member ADOPTING a retained rendezvous (no run) from
            // one RE-RUNNING the cold build (run, even when every store
            // read warm-hits and no materialisation counter moves).
            self.provenance
                .bundle_cold_flight_runs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Producer chain: the routed-shallow producer first
            // (declaration files); on a non-`Built` outcome the
            // standard producer runs. `miss_serve_published`
            // accumulates the publication status of EVERY serve a
            // producer concluded surface-emptiness from — a miss is
            // reproducible only when no fenced serve contributed to it.
            let mut miss_serve_published = true;
            let mut note_empty = |serve_published: bool| {
                miss_serve_published &= serve_published;
            };
            let built = match self
                .materialize_prepared_decl_bundle_from_routed_shallow(view, canonical_id)
            {
                Some(BundleMaterialization::Built { bundle, admitted }) => Some((bundle, admitted)),
                routed_miss => {
                    if let Some(BundleMaterialization::SurfaceEmpty { serve_published }) =
                        routed_miss
                    {
                        note_empty(serve_published);
                    }
                    match self.materialize_prepared_decl_bundle(canonical_id) {
                        Some(BundleMaterialization::Built { bundle, admitted }) => {
                            Some((bundle, admitted))
                        }
                        Some(BundleMaterialization::SurfaceEmpty { serve_published }) => {
                            note_empty(serve_published);
                            None
                        }
                        None => None,
                    }
                }
            };
            // `stable` carries the retention decision BY VALUE. A built
            // bundle: a FENCED (ReturnOnly) serve was served WITHOUT
            // admission (`admitted == false`) and must NOT be retained
            // as a joinable rendezvous — a claimant that adopted it
            // would receive a superseded-state payload with no
            // ReturnOnly signal on its own request. A miss: it is
            // reproducible — and joinable — ONLY when every serve whose
            // surface-emptiness produced it was store-published; a
            // FENCED serve's emptiness describes the superseded
            // artifact, not live content, so a burst member adopting
            // that miss would treat a canonical with a live declaration
            // surface as bundle-less. A `None` with no serve consumed
            // (unloadable canonical) is a reproducible miss and stays
            // joinable.
            let (value, stable) = match built {
                Some((arc, admitted)) => ((Some((*arc).clone())), admitted),
                None => (None, miss_serve_published),
            };
            Ok(crate::resolver_core::StableExecutionValue {
                value,
                stable,
                // Reached the cold materialisation branch.
                computed: true,
                // Prepared-decl bundles gate on `stable`/`admitted`, not a
                // partial-completeness lattice.
                completeness: crate::semantic_query::ResultCompleteness::Complete,
                cache_refusal: None,
            })
        };
        // Bounded re-validation loop, mirroring the IndexedReady lane:
        // a STABLE (admitted or reproducible-miss) outcome is a joinable
        // rendezvous; a fenced-derived outcome serves only the LEADER
        // (ReturnOnly — its request consumed the fenced serve on its own
        // thread, so its suppression rails are already marked); a
        // FOLLOWER cannot prove its claim pre-dates the mutation and
        // re-runs against fresh state (the non-retained lane is gone, so
        // the re-run elects a fresh flight). Under sustained churn the
        // bounded fallback serves the last fenced-derived bundle
        // ReturnOnly, carrying the suppression status onto THIS thread's
        // request-sticky and traced-scope rails (the original fenced
        // ensure ran on the leader's thread, not this one).
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_unpublished: Option<crate::resolver_core::prepared_decl::PreparedDeclBundle> =
            None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result =
                match singleflight.run_retaining(key.clone(), token, flight_body, |sev| sev.stable)
                {
                    Ok(run_result) => run_result,
                    Err(()) => return None,
                };
            if run_result.value.stable {
                return run_result.value.value.clone().map(std::sync::Arc::new);
            }
            if matches!(
                run_result.role,
                crate::resolver_core::SingleflightRole::Leader
            ) {
                // Fenced-derived leader: serve its own caller. The inner
                // fenced `ensure_indexed_ready_serve` already marked this
                // thread's request-sticky and traced-scope rails.
                return run_result.value.value.clone().map(std::sync::Arc::new);
            }
            last_unpublished = run_result.value.value.clone();
        }
        if last_unpublished.is_some() {
            // Sustained-churn bounded fallback (FOLLOWER adoption): the
            // adopted bundle is fenced-derived and this thread never saw
            // the original fenced serve — carry the non-cacheability by
            // hand onto every enclosing traced compute's suppress rail. A
            // fenced-but-VALID serve is Complete, NOT partial: it refuses
            // shared-cache admission only, never marks request partiality.
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
            );
        }
        last_unpublished.map(std::sync::Arc::new)
    }

    /// View-aware prepared-decl bundle lookup.
    ///
    /// When the view carries an overlay source for the canonical
    /// (overlay-bearing session), the shared bundle cache (keyed by
    /// canonical alone) cannot store an overlay-specific bundle without
    /// colliding with the base. The session-tier resolver therefore
    /// bypasses the shared cache and materialises a per-call bundle
    /// rooted at `ctx.ensure_indexed_ready_serve(canonical)` — which
    /// routes through the overlay-priority
    /// `ensure_indexed_ready_serve_with_view` helper and returns the
    /// overlay's [`IndexedReady`] candidate.
    ///
    /// When the view carries no overlay for the canonical the call
    /// transparently delegates to [`Self::prepared_decl_bundle`] so the
    /// base session path keeps its warm-bundle reuse.
    pub(crate) fn prepared_decl_bundle_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        // Two-identity split. `canonical_id` is the RAW requested
        // canonical; the overlay-detection gate + tombstone check below
        // MUST run on it because the `SessionView` overlay maps +
        // tombstone set are raw-keyed — normalising first (the inverse
        // hazard) would fail to detect an overlay that exists only under
        // the raw id. The `OverlayArtifactIdentity` carries the raw
        // owner alongside `analysis_canonical` (the
        // `normalized_analysis_canonical` rewrite); the materialise step
        // drives `ensure_indexed_ready_serve` on the raw owner (its overlay
        // gate is raw-keyed), keys the BUNDLE identity on the raw owner
        // (so a `root_identity.canonical_id` resolves to the overlay
        // content hash under the session view's raw-keyed maps — see
        // `materialize_prepared_decl_bundle_via_ctx`), and keys
        // import-route resolution on the normalised analysis canonical.
        // The base path (`prepared_decl_bundle`) normalises internally,
        // so the raw id is forwarded unchanged.
        if let Some(view) = ctx.active_session_view() {
            let identity = self.overlay_artifact_identity(canonical_id);
            // If the active view tombstones the canonical, or carries an
            // overlay whose content hash differs from the base, the
            // host's shared bundle cache holds the base bundle (keyed by
            // canonical alone). Materialise a fresh bundle rooted at the
            // overlay's IndexedReady so the prepared-decl payload
            // reflects overlay content. Warm-cache reuse stays on the
            // base path when the view carries no overlay for the
            // canonical.
            if view.is_tombstoned(canonical_id) {
                return self.materialize_prepared_decl_bundle_via_ctx(ctx, &identity);
            }
            // An explicit overlay for the canonical means the host's
            // shared bundle cache (keyed by canonical alone) holds the
            // BASE bundle — materialise a fresh bundle rooted at the
            // overlay's IndexedReady instead.
            //
            // Overlay detection uses the **strict**
            // `overlay_content_hash_for`, NOT the permissive
            // `content_hash_for`. `content_hash_for` falls through to
            // the base host's `FileArtifactStore`-derived content hash
            // for an unmasked canonical — the same content-agnostic
            // scan as `get_any`, which can surface a STALE lingering
            // artifact's hash once the own-canonical drain is retired.
            // Comparing that stale hash against the scheduler's current
            // hash would read "overlay differs" for a canonical with NO
            // overlay and materialise the bundle from the stale
            // `IndexedReady` via the overlay path.
            // `overlay_content_hash_for` reports `Some` ONLY for an
            // actual overlay-Upsert, so an unmasked canonical keeps its
            // warm-bundle reuse on the base path.
            if let Some(overlay_hash) = view.overlay_content_hash_for(canonical_id) {
                // Request-scoped, SUCCESS-ONLY memo (the R17-compliant
                // reuse tier). R17 keeps this bundle OUT of the shared
                // `prepared_decl_bundles` cache, so pre-memo every touch
                // re-ran the full materialisation — including the
                // per-import re-export-chain walk
                // (`build_prepared_import_canonicalization`). The memo
                // lives on the request's `CanonicalCompletionOverlay`
                // (dies with the request; never a host/shared cache) and
                // keys on `(raw owner, overlay content hash, store-view
                // compat token)` — the token pins entries to ONE
                // externally-coherent world so a stability-retry attempt
                // under a fresh view misses and re-materialises. See the
                // memo field docs on `CanonicalCompletionOverlay`.
                let memo = ctx.request_completion_overlay();
                let token = ctx.store_view().compat_token();
                if let Some(memo) = memo {
                    if let Some(bundle) =
                        memo.overlay_bundle_memo_get(canonical_id, overlay_hash, token)
                    {
                        self.provenance
                            .overlay_bundle_memo_hits
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Some(bundle);
                    }
                }
                // Cacheability bracket around the materialisation: the
                // nested scope observes the same non-cacheable fan-out
                // every enclosing tracer receives (fan-out reaches ALL
                // active scopes, so enclosing verdicts are unchanged) and
                // gates memo admission. A materialisation that consumed a
                // FENCED overlay serve, an UNROOTABLE route, or a broken
                // decl-body lease is served to THIS caller but never
                // memoised — the per-call re-materialisation (and its
                // per-call fan-out into whatever tracer scopes enclose
                // each later touch) is load-bearing for that class.
                let (bundle, non_cacheable) =
                    crate::fact_signature_helpers::with_cacheability_scope(self, |_probe| {
                        self.materialize_prepared_decl_bundle_via_ctx(ctx, &identity)
                    });
                let bundle = bundle?;
                if !non_cacheable {
                    if let Some(memo) = memo {
                        memo.overlay_bundle_memo_insert(
                            canonical_id,
                            overlay_hash,
                            token,
                            std::sync::Arc::clone(&bundle),
                        );
                    }
                }
                return Some(bundle);
            }
        }
        // Per-request hoist: route the non-overlay fall-through
        // through the view-bound helper, threading `ctx.store_view()`
        // (the request-bound borrow) instead of building a fresh owned
        // snapshot via `self.prepared_decl_bundle(canonical_id)`.
        self.prepared_decl_bundle_with_store_view(ctx.store_view(), canonical_id)
    }

    /// Materialise a fresh prepared-decl bundle rooted at the overlay's
    /// `IndexedReady`. Used by the session-tier view-aware path when the
    /// view carries an overlay for (or tombstones) the canonical — the
    /// shared bundle cache is bypassed because its per-canonical slot
    /// already holds the base bundle.
    ///
    /// `identity` carries both canonical ids, and the two are NOT
    /// interchangeable here:
    ///
    /// * **Bundle identity** — the bundle, and therefore every
    ///   `PreparedTypeDecl::root_identity.canonical_id` it produces, is
    ///   keyed on the **RAW overlay owner**. The bundle's
    ///   `IndexedReady` and `owner_whole_hash` came from the raw
    ///   overlay (`ensure_indexed_ready_serve` is driven on the raw owner);
    ///   the bundle identity must stay tied to that raw owner. A
    ///   downstream prepared-member / prepared-target write-through
    ///   roots its shared-cache entry on `authoritative_current_content_hash`
    ///   of this canonical — and the session view's overlay maps are
    ///   raw-keyed, so only the raw owner resolves to the OVERLAY
    ///   content hash. Keying the bundle on the normalised companion
    ///   instead would root an overlay-derived member on the BASE
    ///   companion hash (the view carries no overlay for the
    ///   companion), admitting session-overlay data into the shared
    ///   cache under a base-valid signature where the base host — or an
    ///   unrelated session — would reuse it.
    /// * **Route-resolution identity** — import-route resolution keys
    ///   on the NORMALISED analysis canonical, matching how the overlay
    ///   `IndexedReady` itself resolved its routes
    ///   (`materialize_overlay_indexed_ready_with_view` resolves
    ///   imports against the analysis canonical) and the base bundle
    ///   path's route-dep cache identity.
    fn materialize_prepared_decl_bundle_via_ctx(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        identity: &crate::host_manage::overlay_materialize::OverlayArtifactIdentity,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        // Drive the overlay-aware `ensure_indexed_ready_serve` on the
        // RAW owner — the overlay-detection gate inside
        // `ensure_indexed_ready_serve_with_view` keys on the raw
        // canonical. Structurally read-only with respect to shared
        // admission: this per-call bundle is NEVER inserted into the
        // shared `prepared_decl_bundles` cache (R17 below), so the
        // serve status needs no local admission gate.
        let facts = ctx
            .ensure_indexed_ready_serve(identity.raw_overlay_owner())?
            .indexed;
        // The bundle identity is the RAW overlay owner — see the
        // doc-comment above. Every `root_identity.canonical_id` on a
        // decl built from this bundle is therefore the raw owner, so a
        // downstream write-through roots on the overlay content hash
        // (the raw owner is the only id the session view's raw-keyed
        // overlay maps mask) and never pollutes the base shared cache.
        let bundle_canonical_id = identity.raw_overlay_owner();
        // Import-route resolution keys on the NORMALISED analysis
        // canonical — directory-equivalent for the `.js`→`.d.ts`
        // rewrite and consistent with the overlay `IndexedReady`'s own
        // route resolution.
        let route_canonical_id = identity.analysis_canonical();
        let state = &facts.shallow_state;
        if !state.has_resolvable_surface()
            && state.import_targets.is_empty()
            && !state.has_augmentation_declarations()
        {
            return None;
        }
        let dep_edges = self.prepared_decl_bundle_route_dep_edges_with_context(
            ctx,
            route_canonical_id,
            state.as_ref(),
        )?;

        let script_setup_type_bindings = if bundle_canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(bundle_canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // Record the DIRECT-hop import identities; the final defining owner
        // resolves at decl-prepare demand through the shared route authority
        // (see `build_prepared_import_canonicalization`).
        let import_canonicalization =
            self.build_prepared_import_canonicalization(state.as_ref(), &dep_edges);

        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                bundle_canonical_id,
                std::sync::Arc::clone(state),
                dep_edges,
                script_setup_type_bindings,
                import_canonicalization,
                self.project_type_store().identity_interner(),
            ),
        );

        // R17: do NOT insert into the shared `prepared_decl_bundles`
        // cache from an overlay-bearing materialisation. The shared
        // slot is keyed by canonical alone and would alias the base
        // bundle, leaking overlay state to base-only consumers.
        component_meta_trace_custom!(
            "materialize_prepared_decl_bundle_via_ctx",
            format!(
                "owner={} type_decls={} value_decls={} dep_edges={} source=session_overlay",
                bundle_canonical_id,
                bundle.prepared_type_decls.len(),
                bundle.prepared_value_decls.len(),
                bundle.dep_edges.len(),
            ),
        );

        Some(bundle)
    }

    fn prepared_decl_bundle_route_dep_edges(
        &self,
        canonical_id: &str,
        state: &crate::resolver_core::ShallowFileState,
    ) -> Option<rustc_hash::FxHashMap<String, String>> {
        self.prepared_decl_bundle_route_dep_edges_with_context(self, canonical_id, state)
    }

    /// Resolve the owner's authored import specifiers into the bundle's
    /// dep-edge table.
    ///
    /// CONTEXT-BOUND, because the shallow surface bakes no target: every
    /// edge resolves here, so a session-overlay build must resolve
    /// through its OWN view or an overlay-only dependency silently
    /// disappears from the bundle. The base host is the ctx on the base
    /// path.
    fn prepared_decl_bundle_route_dep_edges_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        state: &crate::resolver_core::ShallowFileState,
    ) -> Option<rustc_hash::FxHashMap<String, String>> {
        let mut dep_edges = rustc_hash::FxHashMap::default();
        let mut seen_sources = rustc_hash::FxHashSet::default();

        // The OWNER-QUALIFIED table, not the ordinary-file one: a `.vue`
        // SFC's `<script setup>` bindings carry a module/instance
        // top-level owner and never land in `import_targets`, so an
        // ordinary-file-only walk would resolve none of a carrier's
        // imports. That was invisible while the shallow surface baked a
        // resolved target the consumer could fall back on.
        for target in state.owner_import_targets.values() {
            if !seen_sources.insert(target.source_specifier.clone()) {
                continue;
            }

            let cached_resolution =
                self.cached_import_route_resolution(canonical_id, target.source_specifier.as_str());
            let resolved: Option<String> = if let Some(resolution) = cached_resolution.as_ref() {
                let preferred = match self.prefer_type_dependency_target_from_resolution(
                    canonical_id,
                    target.source_specifier.as_str(),
                    resolution,
                ) {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        admitted.into_result()
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => return None,
                };
                preferred.or_else(|| {
                    if Self::import_route_is_known_miss(resolution) {
                        None
                    } else {
                        self.resolve_route_type_edge_with_context(
                            ctx,
                            canonical_id,
                            target.source_specifier.as_str(),
                        )
                    }
                })
            } else {
                self.resolve_route_type_edge_with_context(
                    ctx,
                    canonical_id,
                    target.source_specifier.as_str(),
                )
            };
            let Some(resolved) = resolved else {
                continue;
            };

            dep_edges.insert(target.source_specifier.clone(), resolved);
        }

        Some(dep_edges)
    }

    /// Record each import binding's DIRECT-hop identity for DEMAND-DRIVEN
    /// canonicalization: `(local owner, local name) → (direct target
    /// canonical, ordinary-file owner, imported name)`.
    ///
    /// Bundle build resolves NO import chain. The final defining identity is
    /// resolved at the FIRST decl-prepare / ref-head DEMAND through the shared
    /// route authority (`resolve_imported_type_root_with_facts*`, memoized
    /// host-side in `ImportedRootDb` under an R6 content-free query-identity
    /// key), and every demand site records the chain hops' route facts into
    /// the ACTIVE fact tracer AT DEMAND TIME — so the CONSUMING cache entry
    /// (the `LowerLocator` shape memo, an `Instantiate` memo, a
    /// component-meta proof) carries the barrel/re-export participants'
    /// `FileWholeHash` + `Route` facts in its OWN read-set and a retarget or
    /// leaf edit anywhere on the chain misses that warm read. The demand
    /// sites: the locator-shape ref-head re-canonicalization
    /// (`resolve_locator_ref_head`), the prepared-decl final-hop retry
    /// (`resolve_prepared_type_decl_via_host`), the bare-name import layers
    /// (`resolve_import_binding_from_facts`), and the imported-registry
    /// resolver (`resolve_imported_registry_symbol_with_budget`) — each is
    /// gated on the stored `ordinary_file()` PROVISIONAL owner, the marker
    /// that final-owner resolution is still owed.
    ///
    /// The eager whole-bundle chain resolution this replaces walked EVERY
    /// import at bundle build (loading files nothing demanded, lowering dead
    /// type-args for unresolvable heads) and pinned the chain facts on the
    /// BUNDLE's fact rail — where a downstream memo's read-set never saw
    /// them, so a barrel retarget with the owner unchanged false-warmed the
    /// memoized shape.
    ///
    /// An UNRESOLVABLE specifier (no dep-edge, no parse-time canonical)
    /// records NO entry, preserving the typed `MissingExternalOwner`
    /// preparation failure for declarations that reference it. A namespace
    /// import (`import * as NS`) records NO entry — a namespace alias is a
    /// module handle, not a declaration identity; qualified members resolve
    /// through the namespace-member facts path.
    fn build_prepared_import_canonicalization(
        &self,
        state: &crate::resolver_core::ShallowFileState,
        dep_edges: &rustc_hash::FxHashMap<String, String>,
    ) -> crate::resolver_core::prepared_decl::ImportCanonicalization {
        use verter_semantic::analysis::type_solver::ResolvedRootIdentity;

        let mut canonicalization =
            crate::resolver_core::prepared_decl::ImportCanonicalization::default();
        let interner = self.project_type_store().identity_interner();

        for (local, target) in state.owner_import_targets.iter() {
            if target.is_namespace {
                continue;
            }
            // The import's resolved direct canonical comes from the
            // owner's resolved dep-edge table (built through the shared
            // route-edge authority). The shallow inventory bakes no
            // target, so there is no artifact-derived fallback: an
            // unresolvable specifier has no authoritative target owner
            // to publish.
            let Some(direct_canonical) = dep_edges.get(&target.source_specifier).cloned() else {
                continue;
            };
            if direct_canonical.is_empty() {
                continue;
            }

            canonicalization.final_resolution.insert(
                local.clone(),
                ResolvedRootIdentity::new_in_owner(
                    interner.intern(&direct_canonical),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    interner.intern(&target.imported_name),
                ),
            );
        }

        canonicalization
    }

    /// Routed-shallow cold producer (declaration files only). Returns
    /// `None` when the lane does not apply (non-declaration extension)
    /// or the canonical is unloadable; otherwise a
    /// [`BundleMaterialization`] carrying the consumed serve's
    /// publication status BY VALUE — the flight lane consumes it: a
    /// non-admitted bundle, and a miss concluded from a FENCED serve's
    /// surface-emptiness, are never retained as a joinable rendezvous.
    fn materialize_prepared_decl_bundle_from_routed_shallow(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
    ) -> Option<BundleMaterialization> {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        if !declaration_file {
            return None;
        }

        // Wall-clock fence for the cold materialisation envelope. The
        // `materializations` lane is empty without this — the per-request
        // footprint accumulator carries no production-side producer
        // for the prepared-decl-bundle cold path unless this site
        // pushes a `MaterializationRecord` at the cold-build exit (see
        // also `materialize_prepared_decl_bundle` below for the
        // standard cold-build sibling).
        let materialize_started_at = crate::instant::Instant::now();

        // The serve carries the publication status BY VALUE — consumed by
        // the admission gate below: a FENCED (ReturnOnly) routed-shallow
        // serve may feed THIS caller's bundle, never a shared cache
        // admission.
        let serve = self.routed_shallow_state_serve(canonical_id)?;
        let state = serve.state;
        if !state.has_resolvable_surface()
            && state.import_targets.is_empty()
            && !state.has_augmentation_declarations()
        {
            // Surface-emptiness is a property of the SERVED artifact,
            // not necessarily of live content — carry the serve's
            // publication status so the flight lane can judge the
            // miss's reproducibility.
            return Some(BundleMaterialization::SurfaceEmpty {
                serve_published: serve.store_published,
            });
        }

        let dep_edges = self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref())?;
        // Record the DIRECT-hop import identities; final canonicalization is
        // DEMAND-DRIVEN (chain facts are observed into the CONSUMING query's
        // read-set at decl-prepare demand, never pinned on the bundle rail).
        let import_canonicalization =
            self.build_prepared_import_canonicalization(state.as_ref(), &dep_edges);
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(&state),
                dep_edges,
                rustc_hash::FxHashMap::default(),
                import_canonicalization,
                self.project_type_store().identity_interner(),
            ),
        );

        // The bundle roots its import-route dependency on the owner's
        // RESOLUTION WITNESS. Unlike the legacy digest — which producer
        // and validator had to re-derive identically from two different
        // route-table source orders — the witness IS the observation
        // set, so a store view validates it against the immutable
        // resolution world it captured with nothing to re-derive.
        //
        // FAIL CLOSED on an unrootable witness. `None` means a refused
        // resolution, an unreadable parse surface, or a union that
        // overflows `FACT_SIGNATURE_CAP` — the bundle's import-route
        // dependency cannot be expressed as facts, so nothing can
        // invalidate it. Admitting it rooted on `FileWholeHash` ALONE
        // would serve pre-appearance dependency edges forever: the
        // owner's bytes never move when a dependency appears or
        // retargets. `decline_import_route_witness` has already marked
        // the enclosing compute non-cacheable, but `insert_arc_with_kind`
        // does not consult that flag — a lone `FileWholeHash` is a
        // perfectly well-formed signature — so the producer must refuse
        // here, exactly as the sibling `resolved_import_facts_witness`
        // and `framework::script_facts` producers do.
        let import_route_witness = self.owner_import_route_witness(canonical_id);
        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: state.whole_hash,
        }];
        if let Some(witness) = import_route_witness.clone() {
            facts.extend(witness);
        }
        // No import-chain facts are pinned here: canonicalization is
        // demand-driven, so retarget invalidation rides the CONSUMING
        // query's read-set (recorded at decl-prepare demand), not the
        // bundle rail.

        // Promote the just-materialised canonical's facts into the request
        // overlay BEFORE the bundle insert. Without this promotion the
        // request-entry [`HostStoreView`] snapshot misses the
        // just-published canonical (the snapshot is built once at
        // request entry — entries published after that lookup are
        // invisible to the view), and every subsequent warm-validation of the bundle's
        // stored `FileWholeHash` fact falls through to
        // the base view's untracked-canonical reject. The next read
        // therefore triggers a fresh cold rebuild, and the loop
        // repeats every time the canonical is consulted. With the
        // promotion the overlay knows the canonical's authoritative
        // hashes and the next warm read matches.
        //
        // `route_hash` is `None` when the shallow state has no
        // resolvable surface — mirrors
        // `current_derived_fact_hash(Route)` (only
        // computes the hash when `has_resolvable_surface()` is true).
        // The host view's snapshot uses the same predicate, so the
        // overlay stays in sync with what the request-entry view
        // would have carried had the canonical been published before
        // snapshot time.
        //
        // The producer-side epoch guard would mirror
        // `complete_canonical_inner`'s
        // `host.current_store_view_epoch() != base.mutation_epoch()`
        // short-circuit — but the resolver-tier `StoreView` trait
        // cannot take `&VerterHost` (`no_concrete_verter_host_in_seal_scope`
        // architecture guard). The materialiser publishes
        // unconditionally; a superseded view will be detected by
        // the outer audited-request retry loop, which discards the
        // overlay before re-running the request.
        let route_hash = state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(state.as_ref()));
        view.promote_route_completion(canonical_id, state.whole_hash, route_hash);

        // Strict admission. Bundles always carry `FileWholeHash` — gated
        // on the routed-shallow serve's publication status (ReturnOnly
        // never publishes), the same gate as the standard cold producer
        // below (`materialize_prepared_decl_bundle`): a bundle rooted at a
        // FENCED `IndexedReady` is served to this caller WITHOUT
        // admission. The fenced artifact's route surface was resolved
        // against superseded state, while the fact versions above
        // (`state.whole_hash`; the LIVE import-route witness) validate
        // against a fresh view — so the read-side fact rail cannot reject the entry
        // and this gate is the only correct refusal point. The flag flows
        // BY VALUE through `routed_shallow_state_serve` (see
        // `RoutedShallowServe`), so the gate works with or without an
        // installed `RequestContext`. The `promote_route_completion` call
        // above stays ungated: the request overlay is request-scoped
        // (discarded with the request), not a shared publication.
        if serve.store_published && import_route_witness.is_some() {
            self.resolver
                .runtime
                .prepared_decl_bundles
                .insert_arc_with_kind(
                    canonical_id.to_string(),
                    std::sync::Arc::clone(&bundle),
                    facts,
                    "prepared_decl_bundles",
                );
        }

        self.provenance
            .bundle_materializations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        component_meta_trace_custom!(
            "materialize_prepared_decl_bundle",
            format!(
                "owner={} type_decls={} value_decls={} dep_edges={} source=route_shallow",
                canonical_id,
                bundle.prepared_type_decls.len(),
                bundle.prepared_value_decls.len(),
                bundle.dep_edges.len(),
            ),
        );

        // Push a `MaterializationSubject::PreparedDeclBundle`
        // record onto the per-request footprint accumulator. Wired
        // here (routed-shallow cold path) and at
        // `materialize_prepared_decl_bundle` below (standard cold
        // path) so both `prepared_decl_bundles` cold producers light
        // up the `materializations` lane.
        let duration_ms = materialize_started_at.elapsed().as_secs_f64() * 1000.0;
        crate::component_meta_audit::record_materialization(
            crate::component_meta_audit::MaterializationSubject::PreparedDeclBundle {
                canonical: std::sync::Arc::<str>::from(canonical_id),
                cold: true,
            },
            duration_ms,
        );

        Some(BundleMaterialization::Built {
            bundle,
            admitted: serve.store_published,
        })
    }

    /// Materialize a fresh `PreparedDeclBundle` for a canonical file,
    /// insert it into the stable cache with the appropriate fact
    /// versions, and return a [`BundleMaterialization`] carrying the
    /// consumed serve's publication status BY VALUE (see the
    /// routed-shallow sibling above for the contract). `None` means the
    /// canonical is unloadable (no serve at all).
    fn materialize_prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<BundleMaterialization> {
        // Wall-clock fence for the cold materialisation envelope —
        // see the routed-shallow sibling above for the
        // rationale (push one materialisation record per cold
        // bundle build so the footprint lane has a per-envelope
        // duration breakdown). Captured BEFORE the
        // `ensure_indexed_ready_serve` lookup so a cold IndexedReady build
        // that the materialiser triggers is part of the recorded
        // duration.
        let materialize_started_at = crate::instant::Instant::now();

        // 1. Ensure source/shallow data exists. The publication status is
        // consumed by the admission gate below: a FENCED (ReturnOnly)
        // IndexedReady serve may feed THIS caller's bundle, never a shared
        // cache admission.
        let serve = self.ensure_indexed_ready_serve(canonical_id)?;
        let facts = &serve.indexed;
        let state = &facts.shallow_state;
        if !state.has_resolvable_surface()
            && state.import_targets.is_empty()
            && !state.has_augmentation_declarations()
        {
            // Surface-emptiness is a property of the SERVED artifact,
            // not necessarily of live content — carry the serve's
            // publication status so the flight lane can judge the
            // miss's reproducibility.
            return Some(BundleMaterialization::SurfaceEmpty {
                serve_published: serve.store_published,
            });
        }
        let dep_edges = self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref())?;

        // 4. Build script-setup type bindings for Vue SFCs (once per bundle).
        // Non-Vue files get an empty map — zero cost.
        let script_setup_type_bindings = if canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // 4b. Record the DIRECT-hop import identities. Final canonicalization
        // is DEMAND-DRIVEN: the first decl-prepare demand resolves the chain
        // through the shared route authority and records the chain hops'
        // route facts into the CONSUMING query's read-set (never the bundle
        // rail), so a barrel retarget invalidates the consuming entries.
        let import_canonicalization =
            self.build_prepared_import_canonicalization(state.as_ref(), &dep_edges);

        // 5. Build the bundle atomically.
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(state),
                dep_edges,
                script_setup_type_bindings,
                import_canonicalization,
                self.project_type_store().identity_interner(),
            ),
        );

        // 6. Compute fact versions.
        // The bundle's import-route rooting is the owner's RESOLUTION
        // WITNESS: resolving the owner's authored specifiers through the
        // shared route-edge policy yields the sealed transactions'
        // observations, which the store view validates against its
        // captured immutable resolution world. Producer and validator
        // agree by construction — the producer records the observed fact
        // versions and the validator compares them to the world it
        // captured, with no digest for the two sides to re-derive
        // differently.
        //
        // FAIL CLOSED on an unrootable witness — see the sibling
        // materialiser above. A bundle admitted on `FileWholeHash` alone
        // keeps serving its pre-appearance resolved edges for as long as
        // the owner's bytes stay put, which is precisely the invalidation
        // this rail exists to provide.
        let import_route_witness = self.owner_import_route_witness(canonical_id);
        let whole_hash = facts.whole_hash;
        let mut fact_versions = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: whole_hash,
        }];
        if let Some(witness) = import_route_witness.clone() {
            fact_versions.extend(witness);
        }
        // No import-chain facts are pinned here: canonicalization is
        // demand-driven, so retarget invalidation rides the CONSUMING
        // query's read-set (recorded at decl-prepare demand), not the
        // bundle rail.

        // 7. Insert into the stable cache. Strict admission — bundles always
        // carry `FileWholeHash` — gated on the IndexedReady serve's
        // publication status (ReturnOnly never publishes): a bundle rooted
        // at a FENCED IndexedReady is served to this caller WITHOUT
        // admission. The fenced artifact's route surface was resolved
        // against superseded state, while the fact versions above
        // (`facts.whole_hash`; the LIVE import-route witness) validate
        // against a fresh view — so the read-side fact rail cannot reject the entry and
        // the admission gate is the only correct refusal point. The gate
        // keys on the VALUE-flowed `store_published` flag, not the
        // request-sticky `current_request_result_is_partial` channel:
        // the value flag needs no installed `RequestContext` (the suppress
        // mark is a silent no-op without one) and stays per-serve-precise,
        // whereas the request flag is sticky-coarse (an unrelated earlier
        // partial in the same request would wrongly decline a COMPLETE
        // bundle built from a store-current artifact — the A2 signal split
        // documented on `observe_component_meta_read_suppress`).
        if serve.store_published && import_route_witness.is_some() {
            self.resolver
                .runtime
                .prepared_decl_bundles
                .insert_arc_with_kind(
                    canonical_id.to_string(),
                    std::sync::Arc::clone(&bundle),
                    fact_versions,
                    "prepared_decl_bundles",
                );
        }

        self.provenance
            .bundle_materializations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        component_meta_trace_custom!(
            "materialize_prepared_decl_bundle",
            format!(
                "owner={} type_decls={} value_decls={} dep_edges={}",
                canonical_id,
                bundle.prepared_type_decls.len(),
                bundle.prepared_value_decls.len(),
                bundle.dep_edges.len(),
            ),
        );

        // Push the materialization record onto the per-request
        // accumulator so the footprint lane gets a per-envelope
        // duration entry for this cold bundle build.
        let duration_ms = materialize_started_at.elapsed().as_secs_f64() * 1000.0;
        crate::component_meta_audit::record_materialization(
            crate::component_meta_audit::MaterializationSubject::PreparedDeclBundle {
                canonical: std::sync::Arc::<str>::from(canonical_id),
                cold: true,
            },
            duration_ms,
        );

        Some(BundleMaterialization::Built {
            bundle,
            admitted: serve.store_published,
        })
    }

    /// Test-only bare wrapper. Production callers go through
    /// `ctx.prepared_type_decl` (which routes through `_with_store_view`).
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        // Cold-seed-routed (see [`Self::prepared_decl_bundle`]): a stale
        // read fails the warm probe closed and the bundle materialises cold.
        let view = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx = crate::resolver_core::HostResolverContext::from_cold_seed(self, &view, overlay);
        self.prepared_type_decl_in_with_context(
            &ctx,
            canonical_id,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
        .expect("prepared type declaration failed")
    }

    pub(crate) fn prepared_type_decl_in_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        let Some(bundle) = self.prepared_decl_bundle_with_store_view(view, canonical_id) else {
            return Ok(None);
        };
        let result = bundle.prepared_type_decls.get_in(owner, symbol_name);
        component_meta_trace_custom!(
            "prepared_type_decl_result",
            format!(
                "owner={} symbol={} source=bundle_hit hit={}",
                canonical_id,
                symbol_name,
                result.as_ref().is_ok_and(Option::is_some),
            ),
        );
        result
    }

    pub(crate) fn prepared_type_decl_in_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        let Some(bundle) = self.prepared_decl_bundle_with_context(ctx, canonical_id) else {
            return Ok(None);
        };
        bundle.prepared_type_decls.get_in(owner, symbol_name)
    }

    pub(crate) fn prepared_value_decl_in(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        // Cold-seed-routed (see [`Self::prepared_decl_bundle`]): a stale
        // read fails the warm probe closed and the bundle materialises cold.
        let view = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx = crate::resolver_core::HostResolverContext::from_cold_seed(self, &view, overlay);
        self.prepared_value_decl_in_with_context(&ctx, canonical_id, owner, symbol_name)
    }

    pub(crate) fn prepared_value_decl_in_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let bundle = self.prepared_decl_bundle_with_store_view(view, canonical_id)?;
        bundle.prepared_value_decls.get_in(owner, symbol_name)
    }

    pub(crate) fn prepared_value_decl_in_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let bundle = self.prepared_decl_bundle_with_context(ctx, canonical_id)?;
        bundle.prepared_value_decls.get_in(owner, symbol_name)
    }

    /// Route-aware required-import closure.
    /// Uses the shallow file state's `route_closure` to narrow the import set
    /// to only dependencies reachable from the requested route.
    ///
    /// Falls back to the whole-export closure when route-aware data is unavailable.
    pub(crate) fn required_import_routes_for_exported_route(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
    ) -> rustc_hash::FxHashMap<String, crate::resolver_core::RouteDemand> {
        use crate::resolver_core::shallow_file_state::ExportTarget;
        use crate::resolver_core::RouteDemand;

        if let Some(state) = self.routed_shallow_state(canonical_id) {
            let budget = crate::resolver_core::shallow_file_state::ResolutionBudgets::default()
                .local_closure_steps;
            if let Some((owner, symbol_name, _is_alias_export)) = state
                .export_target(exported_name)
                .and_then(|target| match target {
                    ExportTarget::Local { owner, symbol_name } => {
                        Some((*owner, symbol_name.as_str(), symbol_name != exported_name))
                    }
                    ExportTarget::Reexport { .. } => None,
                })
            {
                let closure = state.route_closure_in(owner, symbol_name, route, budget);
                let mut result = rustc_hash::FxHashMap::default();
                for ext in &closure.unresolved_external {
                    result
                        .entry(ext.local_name.clone())
                        .and_modify(|existing| {
                            *existing =
                                crate::resolver_core::merge_route_demands(existing, &ext.route);
                        })
                        .or_insert_with(|| ext.route.clone());
                }
                if state
                    .type_symbol_kind_in(owner, symbol_name)
                    .is_some_and(|kind| {
                        kind == verter_semantic::analysis::type_eval::TypeDeclKind::Class
                    })
                {
                    // A class's public declaration carrier includes member
                    // annotations that are intentionally absent from its
                    // structural dependency subset. Preserve those imports
                    // through the exact authored owner; a same-name class in
                    // another Svelte script owner is a distinct closure.
                    for required_name in
                        state.required_declaration_import_names_in(owner, symbol_name)
                    {
                        result
                            .entry(required_name)
                            .and_modify(|existing| {
                                *existing = crate::resolver_core::merge_route_demands(
                                    existing,
                                    &RouteDemand::Whole,
                                );
                            })
                            .or_insert(RouteDemand::Whole);
                    }
                }
                return result;
            }

            if !matches!(route, RouteDemand::Whole) {
                return self.required_import_routes_for_exported_route(
                    canonical_id,
                    exported_name,
                    &RouteDemand::Whole,
                );
            }
        }

        if matches!(route, RouteDemand::Whole) {
            return self
                .routed_shallow_state(canonical_id)
                .map(|state| {
                    state
                        .required_import_names(exported_name)
                        .into_iter()
                        .map(|name| (name, RouteDemand::Whole))
                        .collect()
                })
                .unwrap_or_default();
        }

        self.required_import_routes_for_exported_route(
            canonical_id,
            exported_name,
            &RouteDemand::Whole,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn required_import_names_for_exported_route(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
    ) -> rustc_hash::FxHashSet<String> {
        let required_routes =
            self.required_import_routes_for_exported_route(canonical_id, exported_name, route);
        let required = required_routes
            .keys()
            .cloned()
            .collect::<rustc_hash::FxHashSet<_>>();

        if component_meta_debug_enabled() {
            let mut required_list = required.iter().cloned().collect::<Vec<_>>();
            required_list.sort();
            component_meta_debug(format!(
                "required_import_names_for_route source={} exported={} route={:?} source_kind=fresh count={} imports=[{}]",
                canonical_id,
                exported_name,
                route,
                required.len(),
                required_list.join(", "),
            ));
        }

        required
    }

    /// Get or build the canonical shallow type file state for an imported
    /// dependency — the `is_generic_carrier` probe entry. A COLD probe
    /// JOINS the canonical `IndexedReady` build (`ensure_indexed_ready_serve`
    /// via the route-surface accessor): the probe's build IS the build,
    /// so the returned `Arc` is the IndexedReady-owned shallow state.
    ///
    /// Consumed by routed TypeInfo queries and integration tests.
    ///
    /// The lookup is **current-content-pinned**: it never reads
    /// `FileArtifactStore` through the content-agnostic `get_any`. With the
    /// own-canonical drain retired, a same-canonical content edit can leave a
    /// stale pre-edit `IndexedReady` lingering in `FileArtifactStore`; a
    /// `get_any` read would surface that stale artifact and feed a stale
    /// observed-content hash to every provenance-pure signature builder. The
    /// read is therefore pinned to the canonical's authoritative current
    /// content hash; a stale older-content artifact yields a miss.
    pub(crate) fn shallow_file_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let ctx: &dyn crate::resolver_core::ResolverContext = self;
        self.shallow_file_state_with_context(ctx, canonical_id)
    }

    /// Context-threaded core of [`Self::shallow_file_state`].
    ///
    /// `ctx` supplies the current-content oracle: the base host resolves the
    /// scheduler's `parse.whole_hash`, while
    /// [`crate::resolver_core::SessionResolverContext`] overrides it to
    /// consult the active overlay so an overlay-covered dependency pins
    /// against the overlay content hash.
    ///
    /// `canonical_id` is the **raw** requested canonical and is carried
    /// forward unchanged to every read below. Each read is an
    /// overlay-aware accessor — `indexed_for_current_content`,
    /// `routed_shallow_state_with_context`, `artifact_current_indexed`
    /// — and the `SessionView` overlay maps are keyed by the RAW overlay
    /// owner. Normalising the canonical here (the
    /// `normalized_analysis_canonical` rewrite — e.g. a runtime `.js`
    /// whose `.d.ts` companion is the analysis target) BEFORE those reads
    /// would hand the overlay-detection gate the normalised companion id,
    /// the gate would miss the overlay (keyed by the raw owner), and the
    /// reader would silently fall back to the base companion state.
    /// Normalisation is one-way — the raw owner cannot be recovered from
    /// the normalised companion — so the raw id MUST reach the
    /// overlay-detection point. Each accessor owns the raw→normalised
    /// split internally: the overlay branch resolves it through
    /// [`crate::host_manage::overlay_materialize::OverlayArtifactIdentity`]
    /// and the base branch normalises for its `FileArtifactStore` key.
    ///
    /// Resolution order (current-content-pinned read mechanism):
    /// 1. Read [`crate::project_type_store::IndexedReady`] pinned to the
    ///    canonical's authoritative current content hash via
    ///    [`crate::resolver_core::ResolverContext::indexed_for_current_content`]
    ///    — overlay-aware, scheduler-pinned, no `get_any`. A stale
    ///    older-content artifact misses here.
    /// 2. On miss for a live scheduler-tracked canonical, fall through to
    ///    the route-surface accessor
    ///    ([`Self::routed_shallow_state_with_view`]) — overlay-aware, and
    ///    its base fall-through JOINS the canonical `IndexedReady` build
    ///    (`ensure_indexed_ready_serve`), so a cold probe performs exactly the
    ///    single per-file build.
    /// 3. On miss with no `DerivedRawState` at all (a genuinely artifact-only
    ///    canonical — foreign source / test seed), the permissive
    ///    artifact-store read is allowed exactly once, through the named
    ///    [`Self::artifact_current_indexed`] helper that documents that
    ///    contract.
    pub(crate) fn shallow_file_state_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        // Step 1 — current-content-pinned `IndexedReady` fast path. A warm
        // edge-current artifact is a pure cache read; an edge-stale
        // wildcard-bearing artifact is re-indexed inside the accessor
        // (`ensure_indexed_ready_serve`) so its `export *` edges re-resolve against
        // the live file set. That re-index does not re-enter this function:
        // `ensure_indexed_ready_serve`'s materialise path never calls
        // `shallow_file_state` / the content-pinned accessor, and its own reuse
        // is edge-gated, so the re-index terminates at a fresh artifact.
        if let Some(indexed) = ctx.indexed_for_current_content(canonical_id) {
            if indexed.shallow_state.has_resolvable_surface() {
                return Some(indexed.shallow_state.clone());
            }
        }

        // Step 2 — route-surface accessor. Overlay branches serve the
        // overlay materialiser's artifact; the base fall-through joins the
        // canonical `IndexedReady` build (`ensure_indexed_ready_serve`).
        if let Some(state) = self.routed_shallow_state_with_context(ctx, canonical_id) {
            return Some(state);
        }

        // Step 3 — genuinely artifact-only canonical (no scheduler
        // `DerivedRawState`): the named artifact-current authority answers
        // for a foreign-source-loaded / test-seeded artifact. It declines
        // (returns `None`) for any canonical the scheduler tracks, so a stale
        // older-content artifact for a live scope is never surfaced here.
        self.artifact_current_indexed(canonical_id)
            .filter(|indexed| indexed.shallow_state.has_resolvable_surface())
            .map(|indexed| indexed.shallow_state.clone())
    }
}

impl VerterHost {
    /// Build the file's `ShallowFileState` from its content-addressed
    /// payload parts: the parser's route inventory, the lazy
    /// declaration-body memo, and the framework component-`default`
    /// synth.
    ///
    /// PURE PARSE DOMAIN — it performs ZERO import resolution. The
    /// shallow inventory it produces names AUTHORED specifiers; the
    /// resolved target of each is a resolve-domain answer every consumer
    /// demands from the workspace resolution authority at the point of
    /// use. Resolving here is what made a content-addressed artifact
    /// carry dependency-set-derived state, and what forced the global
    /// `edge_generation` stamp (plus a whole edge-refresh materialise
    /// lane) to guard it.
    fn build_indexed_shallow_surface(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        snapshot: &crate::types::FileAnalysisSnapshot,
        route_inventory: &Arc<
            verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory,
        >,
        decl_bodies: &Arc<crate::decl_body_memo::DeclBodyMemo>,
        eval_source: Option<&str>,
    ) -> Arc<crate::resolver_core::ShallowFileState> {
        self.provenance
            .shallow_state_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut shallow_state_inner = crate::resolver_core::ShallowFileState::from_route_inventory(
            whole_hash,
            Arc::clone(route_inventory),
            Arc::clone(decl_bodies),
        );
        // Synthesise the implicit component `default` value symbol from
        // type-based macros, dispatched through the framework registry's
        // synthesis leg — see `framework::synth` for the policy and the
        // per-framework legs (Vue's macro synth, Svelte's, …).
        self.inject_component_default_into_shallow_state(
            canonical_id,
            &mut shallow_state_inner,
            &snapshot.macros,
            eval_source,
            // The flight's already-resolved carrier artifact — never a
            // re-fetch through `current_eval_state` (which re-indexes the
            // owner mid-index and recurses).
            decl_bodies.framework_parse(),
        );
        Arc::new(shallow_state_inner)
    }

    /// Test-only seam used by mid-flight mutation tests — see the
    /// `materialize_seam_hook` field docs. Clones the installed hook out
    /// of the slot before invoking it so the hook (which typically parks
    /// on a barrier) never blocks the slot's lock.
    #[cfg(test)]
    pub(crate) fn fire_materialize_seam(&self) {
        let hook = self.materialize_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only seam for the singleflight retry loop — see the
    /// `flight_retry_seam_hook` field docs. Same clone-out-then-invoke
    /// discipline as [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_flight_retry_seam(&self) {
        let hook = self.flight_retry_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only seam fired between the raw-analysis-snapshot scheduler
    /// lane's analysis capture and its template-analysis source join —
    /// see the `raw_snapshot_template_join_seam_hook` field docs. Same
    /// clone-out-then-invoke discipline as
    /// [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_raw_snapshot_template_join_seam(&self) {
        let hook = self.raw_snapshot_template_join_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only seam fired between the template-analysis computation's
    /// by-value compute and its `derived_raw_cache` persist — see the
    /// `template_persist_seam_hook` field docs. Same
    /// clone-out-then-invoke discipline as
    /// [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_template_persist_seam(&self) {
        let hook = self.template_persist_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only seam fired between the narrowed-scope serve branch's
    /// source snapshot capture and its snapshot products assembly —
    /// see the `narrowed_scope_serve_seam_hook` field docs. Same
    /// clone-out-then-invoke discipline as
    /// [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_narrowed_scope_serve_seam(&self) {
        let hook = self.narrowed_scope_serve_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only seam fired between `get_compile_blockers`'s source
    /// snapshot capture and its snapshot products assembly — see the
    /// `compile_blockers_serve_seam_hook` field docs. Same
    /// clone-out-then-invoke discipline as
    /// [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_compile_blockers_serve_seam(&self) {
        let hook = self.compile_blockers_serve_seam_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test-only bare wrapper over [`Self::ensure_indexed_ready_serve`]
    /// that drops the publication status. PRODUCTION code must use the
    /// serve variant — the carrier is the ONLY production accessor for a
    /// cold/warm `IndexedReady`, so a fenced (ReturnOnly) serve is
    /// always visible by value at the consumption site (and via the
    /// traced-scope chokepoint flag). Test fixtures that only need the
    /// artifact call this wrapper.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn ensure_indexed_ready(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.ensure_indexed_ready_serve(canonical_id)
            .map(|serve| serve.indexed)
    }

    /// Ensure the canonical post-parse artifact is materialized for a
    /// file, with the publication status flowed by value — see
    /// [`IndexedReadyServe`] for the contract.
    ///
    /// This is the single materialization bridge for the semantic DB layer.
    ///
    /// On cache hit, returns the cached `IndexedReady` without any I/O.
    /// On miss, reads the file, parses, builds analysis/snapshot/eval, constructs
    /// `ShallowFileState`, and publishes to `FileArtifactStore`.
    pub(crate) fn ensure_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<IndexedReadyServe> {
        verter_workspace::probe_scope!(ENSURE_INDEXED_READY);
        let serve = self.ensure_indexed_ready_serve_uninstrumented(canonical_id);
        // Test-only deterministic fenced-serve override: convert a would-be
        // PUBLISHED serve into a FENCED one (fire the non-cacheability fan-out +
        // `store_published = false`) WITHOUT a `project_generation` bump, so a
        // consumer's `GenerationSuperseded` admission gate cannot mask the
        // fenced-serve refusal. The served `indexed` is preserved so the value
        // still resolves (ReturnOnly). Already-fenced serves are unchanged.
        #[cfg(test)]
        if self
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(serve) = serve {
                if serve.store_published {
                    crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                        crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                    );
                    return Some(IndexedReadyServe {
                        indexed: serve.indexed,
                        store_published: false,
                    });
                }
                return Some(serve);
            }
            return None;
        }
        // Demand-time ROUTE-fact observation: a traced compute that consumes
        // this canonical's indexed route surface depends on it — record the
        // `DerivedFactHash{Route}` fact into every active tracer so the
        // consuming cache entry's read-set (a component-meta proof, a
        // semantic-memo build, a compile-tier signature) revalidates when
        // the file's export route surface moves. Observed ONLY when the
        // serve mirrors the store-view snapshot's publish predicate —
        // store-published + edge-current + resolvable surface — with the
        // artifact's own `route_hash` (== `hash_route_surface` over the
        // served shallow state, the SAME derivation `HostStoreView::build`
        // publishes), so warm validation round-trips: the view sources the
        // identical hash from the identical artifact. A fenced serve
        // observes nothing (its compute is already refused admission), and
        // a tracer-less call skips the derivation entirely.
        if crate::resolver_core::resolver_context::fact_tracer_installed() {
            if let Some(serve) = serve.as_ref() {
                if serve.store_published {
                    if let Some(route_hash) = serve.indexed.route_surface_hash() {
                        let normalized = self.normalized_analysis_canonical(canonical_id);
                        if serve.indexed.shallow_state.has_resolvable_surface()
                            && self.indexed_surface_is_current(normalized.as_ref(), &serve.indexed)
                        {
                            crate::resolver_core::resolver_context::observe_fan_out(
                                crate::resolver_core::FactVersionRef::DerivedFactHash {
                                    canonical_id: normalized.as_ref().to_string(),
                                    kind: crate::resolver_core::DerivedFactKind::Route,
                                    hash: route_hash,
                                },
                            );
                        }
                    }
                }
            }
        }
        serve
    }

    fn ensure_indexed_ready_serve_uninstrumented(
        &self,
        canonical_id: &str,
    ) -> Option<IndexedReadyServe> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Fast path: check FileArtifactStore through the project-global cache.
        // R3: query the scheduler's current `whole_hash` for the
        // canonical and pin the lookup to it. With eager
        // `evict_canonical` removed at upsert, the `get_any`
        // permissive lookup could return a stale candidate alongside
        // the fresh content's entry; gating on the scheduler's
        // current hash forces the cache to serve the authoritative
        // version per R1 (content-addressed identity).
        let current_whole_hash = self
            .effective_file_state(canonical_id, None)
            .map(|state| state.whole_hash);
        if let Some(current_hash) = current_whole_hash {
            if let Some(indexed) = self
                .project_type_store
                .indexed()
                .get(canonical_id, current_hash)
            {
                // A content-current artifact is reusable while its parse
                // environment is unchanged. A moved parse env means the
                // retained `framework_parse` / `shallow_state` /
                // `decl_bodies` were produced under a superseded
                // environment, so falling through (not returning) routes
                // the hit into the full re-materialise (re-parse).
                if self.indexed_surface_is_current(canonical_id, &indexed) {
                    component_meta_trace_custom!(
                        "ensure_indexed_ready_fast_hit",
                        format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
                    );
                    return Some(IndexedReadyServe {
                        indexed,
                        store_published: true,
                    });
                }
            }
        } else if let Some(indexed) = self
            .artifact_current_indexed_raw(canonical_id)
            .filter(|indexed| self.indexed_surface_is_current(canonical_id, indexed))
        {
            // Scheduler doesn't have a current snapshot. The
            // artifact-current authority answers ONLY for a genuinely
            // artifact-only canonical (no scheduler `DerivedRawState` —
            // a foreign-source-loaded file or a test seed); for such a
            // canonical staleness is not driven by content upserts, so
            // the single retained artifact is the current one. A
            // canonical the scheduler DOES track (a `DerivedRawState`
            // entry exists) gets `None`, so this branch declines and the
            // materialiser below rebuilds rather than serving a
            // possibly-stale artifact.
            //
            // This peeks the artifact via the NON-recursing
            // `artifact_current_indexed_raw` (not the re-indexing
            // `artifact_current_indexed`): the edge-currency filter here +
            // the `materialize` re-index below are the single re-index entry,
            // so there is no mutual recursion with `artifact_current_indexed`
            // (which itself calls `ensure_indexed_ready_serve` on edge-stale).
            component_meta_trace_custom!(
                "ensure_indexed_ready_fast_hit",
                format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
            );
            return Some(IndexedReadyServe {
                indexed,
                store_published: true,
            });
        }

        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        let materialize = || -> Option<crate::project_type_store::IndexedFlightOutcome> {
            verter_workspace::probe_scope!(ENSURE_INDEXED_COLD);
            self.provenance
                .indexed_ready_materializes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Captured BEFORE any read/parse so the pre-publish fence
            // below detects every mid-flight mutation; the
            // `project_generation` value is also the stamp the published
            // artifact carries.
            let flight_workspace_generation = self.ws().content_generation();
            let flight_project_generation = self.project_type_store.current_project_generation();
            // The R21 parse dimension the parse below runs under — the
            // value-side stamp the reuse gates compare against the live
            // per-canonical parse env.
            let flight_parse_env_hash = self.host_view_env_hashes_for(canonical_id).parse_env_hash;
            #[cfg(test)]
            self.fire_materialize_seam();
            // Materialize: read source, build analysis, construct facts.
            //
            // Native: scheduler is the sole source authority. On a scheduler
            // miss, call `ensure_loaded` once to submit the canonical through
            // the scheduler — the canonical way to materialize a file. If
            // the scheduler still misses after `ensure_loaded`, return None
            // (file doesn't exist in the workspace).
            let (raw_source, mut framework_parse, whole_hash) = {
                let state = match self.effective_file_state(canonical_id, None) {
                    Some(state) => state,
                    None => {
                        // On scheduler miss, call ensure_loaded once — the
                        // canonical way to materialize a file into the
                        // scheduler + current request view's extension store.
                        // Raw import specifiers and empty canonicals are
                        // never loadable.
                        if canonical_id.is_empty()
                            || is_raw_import_specifier_id(canonical_id)
                            || !self.ensure_loaded(canonical_id)
                        {
                            return None;
                        }
                        self.effective_file_state(canonical_id, None)?
                    }
                };
                if !self.store_view_allows_current_whole_hash(canonical_id, state.whole_hash) {
                    return None;
                }
                (state.source, state.framework_parse, state.whole_hash)
            };

            let file_language = self.language_classifier.classify(canonical_id);

            // A carrier canonical (`.vue`, `.svelte`, …) the scheduler has not
            // parsed yet runs the carrier parser ONCE here through the counted
            // chokepoint — the carrier parse is the one legitimately separate
            // parser; everything downstream (eval source, snapshot, env,
            // analysis) reuses its framework-neutral artifact.
            if framework_parse.is_none() && file_language.is_framework_carrier() {
                framework_parse = crate::parse::build_carrier_parse_artifact_from_source(
                    &file_language,
                    &raw_source,
                    &self.provenance,
                );
            }
            let framework_parse = framework_parse;

            // `eval_is_extracted_script` records whether the eval source is the
            // position-preserving extracted carrier script — the predicate that
            // lets the snapshot build below walk the flight's single
            // eval-program parse instead of re-parsing the same script bytes.
            let (eval_source_text, eval_is_extracted_script) =
                Self::build_eval_script_source_with_extraction(
                    canonical_id,
                    raw_source.as_ref(),
                    framework_parse.as_deref(),
                );
            let eval_source = Arc::<str>::from(eval_source_text);
            // The authoritative `source_type` is resolved ONCE (scheduler
            // value first) and feeds the single eval-program parse below;
            // per-call recomputation diverged for `.vue` `lang="tsx"`.
            let source_type =
                self.imported_eval_source_type_for(canonical_id, framework_parse.as_deref());
            // THE single eval-program parse for this cold canonical
            // build — performed AND RETAINED on the lazy lowering
            // service's worker (keyed by the content-generation
            // `SnapshotKey`), so later declaration-body demands reuse
            // the same parse instead of re-parsing per touch. The cold
            // job builds only INDEX products from the borrowed program:
            // the declaration headers and exact route inventory, plus
            // (when the scheduler had no snapshot) the file-analysis
            // snapshot. ZERO declaration bodies lower here.
            let snapshot_key = crate::decl_lowering::SnapshotKey {
                canonical: Arc::from(canonical_id),
                whole_hash,
                parse_env_hash: flight_parse_env_hash,
            };

            let scheduler_snapshot = self.build_snapshot_from_scheduler(canonical_id).map(|s| {
                self.provenance
                    .indexed_ready_scheduler_snapshot_reuse
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Arc::new(s)
            });

            struct ColdIndexProducts {
                header_index: verter_semantic::analysis::decl_headers::DeclHeaderIndex,
                route_inventory:
                    verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory,
                snapshot: Option<crate::types::FileAnalysisSnapshot>,
                svelte_component_runes_mode: bool,
                owner_table: Arc<verter_semantic::analysis::TopLevelOwnerTable>,
            }

            let job_canonical = canonical_id.to_string();
            let job_raw_source = Arc::clone(&raw_source);
            let job_framework_parse = framework_parse.clone();
            let job_scope = self.config.effective_scope();
            let job_provenance = Arc::clone(&self.provenance);
            let need_snapshot = scheduler_snapshot.is_none();
            // A carrier whose neutral artifact opens through the blessed Vue
            // accessor builds the Vue-shaped snapshot; any other carrier (Svelte
            // today) builds the carrier-neutral snapshot from its retained eval
            // program. The dispatch is by the artifact's own carrier — never a
            // hardcoded extension branch.
            let is_carrier = file_language.is_framework_carrier();
            // Pin the retained parse for this content generation HERE — at
            // the cold-index parse, the earliest service parse — and hand
            // the lease to the artifact's memo below, so the header-index
            // parse and every later body demand share ONE parse for the
            // artifact's whole life (no LRU, no silent re-parse).
            let cold_lease =
                self.decl_lowering
                    .acquire_lease(&snapshot_key, &eval_source, source_type);
            if cold_lease.parsed_now {
                self.provenance
                    .eval_program_parses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            // LEASE-ONLY run: `cold_lease` (acquired above) is held on
            // this stack, so the retained snapshot is pinned for the
            // whole flight and this cold-index job reuses it — the run
            // cannot parse, per the lease-only worker contract.
            let outcome = self.decl_lowering.run_leased(
                &snapshot_key,
                move |program: Option<&crate::ParsedEvalProgram>| {
                    let owner_table = Arc::new(match program {
                        Some(parsed) => crate::parse::top_level_owner_table(
                            parsed.borrow_dependent(),
                            job_framework_parse.as_deref(),
                        )?,
                        None => verter_semantic::analysis::TopLevelOwnerTable::ordinary_file(0),
                    });
                    let svelte_component_runes_mode = program.is_some_and(|parsed| {
                        job_framework_parse.as_deref().is_some_and(|artifact| {
                            crate::parse::svelte_component_runes_mode(
                                artifact,
                                parsed.borrow_dependent(),
                            )
                        })
                    });
                    let (header_index, route_inventory) = match program {
                        Some(parsed) => {
                            let body = parsed.borrow_dependent();
                            let index = build_script_shallow_index_with_owners(
                                body,
                                parsed.source_str(),
                                &owner_table,
                            )
                            .map_err(|error| {
                                crate::parse::ScriptOwnerIndexError::ParserTable {
                                    statement_count: error.statement_count(),
                                    owner_count: error.owner_count(),
                                }
                            })?;
                            (index.declaration_headers, index.routes)
                        }
                        // Fatal parse: empty shallow index — no
                        // re-parse under a different source type (the
                        // authoritative `source_type` already failed).
                        None => Default::default(),
                    };
                    let vue_parsed = job_framework_parse
                        .as_deref()
                        .and_then(crate::typeinfo::adapters::vue::vue_parse);
                    let snapshot = if !need_snapshot {
                        None
                    } else if let Some(parsed_sfc) = vue_parsed {
                        // Vue SFC snapshot from the artifact's typed parse (opened
                        // through the blessed `vue_parse` accessor). The script
                        // program is the flight's eval program when the eval
                        // source IS the extracted script — the snapshot walks the
                        // SAME retained parse.
                        let parse = crate::parse::build_vue_snapshot_from_parsed(
                            &job_canonical,
                            job_raw_source.as_ref(),
                            job_scope,
                            parsed_sfc,
                            &job_provenance,
                            VerterHost::vue_flight_script_program(
                                eval_is_extracted_script,
                                program,
                            ),
                            Some(&owner_table),
                        );
                        Some(VerterHost::build_snapshot_from_parse(parse))
                    } else if is_carrier {
                        // A non-Vue carrier (Svelte): its eval source IS the
                        // position-preserving extracted script, so the snapshot's
                        // script program is the flight's retained eval program —
                        // walk it, parse nothing. The carrier-neutral snapshot
                        // builder runs the script analysis over that program.
                        job_framework_parse.as_deref().map(|artifact| {
                            let parse =
                                crate::parse::build_carrier_snapshot_from_artifact_with_program(
                                    &job_canonical,
                                    job_raw_source.as_ref(),
                                    job_scope,
                                    artifact,
                                    &job_provenance,
                                    VerterHost::framework_flight_script_program(
                                        eval_is_extracted_script,
                                        program,
                                    ),
                                    Some(&owner_table),
                                );
                            VerterHost::build_snapshot_from_parse(parse)
                        })
                    } else if let Some(parsed) = program {
                        let parse = crate::parse::build_non_sfc_snapshot_from_program(
                            &job_canonical,
                            job_raw_source.as_ref(),
                            source_type,
                            parsed.borrow_dependent(),
                            parsed.had_errors(),
                        );
                        Some(VerterHost::build_snapshot_from_parse(parse))
                    } else {
                        // Fatal (panicked) eval-program parse on a non-carrier
                        // canonical: a re-parse over the same bytes under
                        // the same source type panics identically, so the
                        // default-empty snapshot IS the parse outcome.
                        Some(crate::types::FileAnalysisSnapshot::default())
                    };
                    Ok::<_, crate::parse::ScriptOwnerIndexError>(ColdIndexProducts {
                        header_index,
                        route_inventory,
                        snapshot,
                        svelte_component_runes_mode,
                        owner_table,
                    })
                },
            );
            // The parse was already counted at lease acquisition above; the
            // cold-index run reused the pinned snapshot. A lease miss is
            // impossible by construction (`cold_lease` is held on this
            // stack), so the `None` arm is an invariant break: fail
            // CLOSED (no artifact) — loud in debug builds — never a
            // transient re-parse.
            let Some(products) = outcome else {
                debug_assert!(
                    false,
                    "cold-index run missed its own held lease pin for {}",
                    snapshot_key.canonical
                );
                return None;
            };
            let products = match products {
                Ok(products) => products,
                Err(error) => {
                    tracing::error!(
                        canonical = %snapshot_key.canonical,
                        error = %error,
                        "carrier owner indexing failed"
                    );
                    return None;
                }
            };
            let snapshot = scheduler_snapshot
                .unwrap_or_else(|| Arc::new(products.snapshot.unwrap_or_default()));
            let route_inventory = Arc::new(products.route_inventory);

            // The lazy declaration-body memo this artifact owns — the
            // body authority for this content generation; bodies lower
            // through the retained snapshot on first semantic demand. It
            // holds the cold-index lease so its body demands reuse that
            // one pinned parse.
            let decl_bodies = Arc::new(crate::decl_body_memo::DeclBodyMemo::new(
                snapshot_key,
                Arc::clone(&eval_source),
                framework_parse.clone(),
                source_type,
                Arc::clone(&products.owner_table),
                products.svelte_component_runes_mode,
                Arc::clone(&self.decl_lowering),
                Arc::new(products.header_index),
                Arc::clone(&self.provenance),
                Some(cold_lease.lease),
            ));

            let shallow_state = self.build_indexed_shallow_surface(
                canonical_id,
                whole_hash,
                snapshot.as_ref(),
                &route_inventory,
                &decl_bodies,
                Some(eval_source.as_ref()),
            );

            // Prefer the scheduler's file state for script_analysis (it may have
            // richer compilation context), but fall back to the snapshot's data
            // for workspace-only files that are not in the scheduler.
            let script_analysis = self
                .effective_file_state(canonical_id, None)
                .filter(|state| state.whole_hash == whole_hash)
                // `state.script_analysis` is already the shared
                // `Arc<ScriptAnalysisSnapshot>` — thread the same allocation
                // onto `IndexedReady` instead of re-wrapping a deep copy.
                .map(|state| state.script_analysis)
                .or_else(|| {
                    Some(Arc::new(
                        verter_semantic::analysis::ScriptAnalysisSnapshot {
                            imports: snapshot.imports.clone(),
                            module_references: snapshot.module_references.as_ref().clone(),
                            bindings: snapshot.bindings.clone(),
                            macros: snapshot.macros.as_ref().clone(),
                            macro_type_deps: snapshot.macro_type_deps.as_ref().clone(),
                            flags: verter_semantic::analysis::AnalysisFlags::from_bits_truncate(
                                snapshot.script_flags,
                            ),
                            ..Default::default()
                        },
                    ))
                });
            let export_signatures = Some(Arc::clone(&snapshot.export_signatures));

            // Project the AppConfig-interface flag from the merged
            // analysis snapshot onto IndexedReady. The flag is the
            // production input the `AppConfigNoOverrideProofDb`
            // producer consults to short-circuit files that cannot
            // contribute an override.
            let declares_interface_app_config = script_analysis
                .as_ref()
                .map(|sa| {
                    sa.flags.contains(
                        verter_semantic::analysis::AnalysisFlags::DECLARES_INTERFACE_APP_CONFIG,
                    )
                })
                .unwrap_or(false);

            // Publish the canonical post-parse artifact into FileArtifactStore.
            // This is the single authoritative cache consumers read from.
            let indexed = Arc::new(crate::project_type_store::IndexedReady {
                whole_hash,
                shallow_state,
                built_at_content_generation: flight_workspace_generation,
                parse_env_hash: flight_parse_env_hash,
                raw_source: Arc::clone(&raw_source),
                eval_source: Arc::clone(&eval_source),
                framework_parse,
                script_analysis,
                export_signatures,
                snapshot,
                route_inventory: Arc::clone(&route_inventory),
                declares_interface_app_config,
                macro_hot_mirror: crate::structural_carrier_producer::MacroHotMirror::default(),
            });

            // PRE-PUBLISH FENCE. A workspace content mutation or a
            // route-resolution mutation that landed during this build
            // means the artifact was produced against superseded state:
            // serve it to the caller (ReturnOnly) but publish NOTHING —
            // the next caller re-materialises against the new state.
            // Publishing a known-superseded artifact violates the
            // standing ReturnOnly rule. The `published: false` outcome
            // also keeps the flight from being retained as a joinable
            // rendezvous and tells followers to re-run rather than adopt.
            //
            // The fence check and the insert below are NOT one atomic
            // critical section: a mutation can land in the window
            // between them and the insert still publishes an artifact
            // stamped with the (now superseded) flight generations.
            // That torn insert is REJECTED READ-SIDE — it is
            // indistinguishable from an insert that completed a moment
            // BEFORE the mutation, and the same reader gates handle
            // both:
            //
            // * An own-content mutation: every reader pins its store
            //   lookup to the scheduler-current `whole_hash`
            //   (`indexed().get(canonical, current_hash)` /
            //   `artifact_current_indexed_raw`), so the torn artifact —
            //   keyed under the pre-mutation hash — is a key miss.
            // * A foreign-content mutation: the artifact retains no
            //   resolved target and no dependency-set-derived state, so
            //   nothing about it can be stale — serving it is sound.
            // * A parse-env-moving mutation:
            //   `indexed_surface_is_current` rejects on the stale
            //   `parse_env_hash`.
            //
            // Closing the window with a lock would order this store's
            // lock against the workspace/scheduler generation locks for
            // no correctness gain; the fence stays a best-effort churn
            // reducer (skip publishing KNOWN-superseded artifacts and
            // do not retain the flight as a joinable rendezvous), and
            // correctness remains read-side authoritative.
            #[cfg(test)]
            self.fire_materialize_seam();
            if self.ws().content_generation() != flight_workspace_generation
                || self.project_type_store.current_project_generation() != flight_project_generation
            {
                return Some(crate::project_type_store::IndexedFlightOutcome {
                    indexed,
                    published: false,
                });
            }
            self.project_type_store
                .indexed()
                .insert(Arc::from(canonical_id), Arc::clone(&indexed));

            Some(crate::project_type_store::IndexedFlightOutcome {
                indexed,
                published: true,
            })
        };

        // Collapse concurrent cold loads for the same canonical file through
        // the dedicated singleflight group on the resolver runtime.
        let singleflight = &self.resolver.runtime.indexed_singleflight;
        // Fixed lane identity: this singleflight is keyed by `canonical_id`
        // and re-checks the content-discriminating cache inside the flight,
        // so all callers intentionally coalesce onto one lane per canonical
        // regardless of view — `validity_fingerprint` stays `0`.
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        };
        let flight_body = || -> Result<crate::project_type_store::IndexedFlightOutcome, ()> {
            // Re-check cache inside the flight — another thread may have
            // populated it after we dropped the first probe. Gate the
            // re-check on the scheduler's current `whole_hash` for the
            // same reason as the outer fast-path: with eager
            // `evict_canonical` retired, a stale candidate could
            // coexist with the fresh entry and `get_any` is not
            // content-discriminating.
            let current_whole_hash = self
                .effective_file_state(canonical_id, None)
                .map(|state| state.whole_hash);
            // Content-current candidate (the scheduler-pinned `get` arm, or
            // the artifact-current authority for a genuinely artifact-only
            // canonical — the NON-recursing `artifact_current_indexed_raw`,
            // so there is no back-edge into the re-indexing
            // `artifact_current_indexed`).
            let content_current_candidate = match current_whole_hash {
                Some(current_hash) => self
                    .project_type_store
                    .indexed()
                    .get(canonical_id, current_hash),
                None => self.artifact_current_indexed_raw(canonical_id),
            };
            if let Some(candidate) = content_current_candidate {
                // Same gate as the outer fast path. A store hit IS the
                // published current surface.
                if self.indexed_surface_is_current(canonical_id, &candidate) {
                    return Ok(crate::project_type_store::IndexedFlightOutcome {
                        indexed: candidate,
                        published: true,
                    });
                }
                // A content-current candidate that fails the gate can
                // only have failed on parse env, and the retained
                // payload was parsed under the superseded one — so the
                // full re-materialise (re-parse) is the ONLY successor.
                // There is no route-only refresh lane: the artifact
                // carries no resolved target for a route mutation to
                // stale.
            }
            materialize().ok_or(())
        };

        // Bounded re-validation loop around the singleflight, mirroring
        // `run_stable_request`'s stability-retry contract. The `retain`
        // predicate keys off the flight outcome's publication validity:
        //
        // - A PUBLISHED outcome is retained as a joinable rendezvous —
        //   any claimant may adopt it (it was the store-current surface
        //   when the flight completed; later mutations are the read
        //   gates' job, exactly as for a plain warm hit).
        // - A FENCED outcome (the pre-publish fence tripped — a mutation
        //   landed mid-flight) is NOT retained: the publish-to-waiters
        //   and the lane removal are one atomic critical section, so no
        //   NEW claimant can join it. The LEADER serves it to its own
        //   caller (ReturnOnly — that request pre-dates the mutation). A
        //   FOLLOWER cannot prove its claim pre-dates the mutation, so it
        //   must NOT adopt a superseded artifact as current: it re-runs
        //   against fresh state on the next loop attempt (the fenced lane
        //   is already gone, so the re-run elects a fresh flight).
        //
        // The loop is bounded; under sustained churn (every attempt
        // fenced) the last fenced result is served ReturnOnly — the same
        // bounded-fallback shape as `run_stable_request`.
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_fenced: Option<Arc<crate::project_type_store::IndexedReady>> = None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result = match singleflight.run_retaining(
                canonical_id.to_owned(),
                token,
                flight_body,
                |outcome| outcome.published,
            ) {
                Ok(run_result) => run_result,
                Err(()) => return None,
            };
            let outcome = (*run_result.value).clone();
            if outcome.published {
                return Some(IndexedReadyServe {
                    indexed: outcome.indexed,
                    store_published: true,
                });
            }
            if matches!(
                run_result.role,
                crate::resolver_core::SingleflightRole::Leader
            ) {
                // FENCED leader: its own caller may consume the result
                // (the request pre-dates the mutation, and the leader's
                // recorded facts match the data it computed FROM — the
                // read-side fact rail is the stated authority), but mark
                // cache non-admission anyway as cheap defense-in-depth: an
                // enclosing cold compute that folds this ReturnOnly artifact
                // into a broader result must not warm shared caches with it
                // (symmetric with the follower fallback below). This is a
                // VALID (Complete) fenced serve — the non-cacheability rail
                // only, NEVER request partiality. The ReturnOnly status ALSO
                // flows by value (`store_published == false`) so
                // value-derived shared admissions (the prepared-decl bundle
                // gate) decline even without an installed `RequestContext`.
                //
                // Chokepoint: flag every enclosing traced cold compute
                // (semantic-memo builds, the owner-import-surface and
                // component-meta proof producers) so their admission
                // gates refuse the fenced-derived result by value.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                );
                return Some(IndexedReadyServe {
                    indexed: outcome.indexed,
                    store_published: false,
                });
            }
            last_fenced = Some(outcome.indexed);
            #[cfg(test)]
            self.fire_flight_retry_seam();
        }
        if last_fenced.is_some() {
            // Sustained-churn bounded fallback: every attempt was fenced
            // and this claimant was a FOLLOWER each time, so the served
            // artifact is a known-superseded surface whose claim
            // POST-dates the supersession. It is ReturnOnly in kind —
            // valid for this caller's read, never admissible downstream:
            // an enclosing cold compute would record live (post-mutation)
            // facts while having computed FROM the superseded data, an
            // entry the read-side fact rail cannot catch (the recorded
            // facts genuinely match the live view). Carry the ReturnOnly
            // status to the admission gates through the non-cacheability
            // fan-out (every enclosing traced cold compute) AND by value
            // (`store_published == false`). A fenced-but-VALID serve is
            // Complete, NOT partial — the non-cacheability rail only, never
            // request partiality.
            //
            // Chokepoint: flag every enclosing traced cold compute —
            // same rail as the fenced-leader arm above.
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
            );
        }
        last_fenced.map(|indexed| IndexedReadyServe {
            indexed,
            store_published: false,
        })
    }

    pub(crate) fn current_or_read_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        // Live-host probe. Resolvers that need to load a canonical
        // mid-resolution must call `ensure_loaded` explicitly; only the
        // top-level / test-scaffold path auto-loads on miss.
        //
        // An evicted canonical reports NO hash here by construction:
        // `get_whole_hash`'s scheduler branch demands a visible
        // (non-evicted) entry, and its artifact-only fallback answers
        // through the single authority gate — any `DerivedRawState`
        // entry (evicted included) means the scheduler is the content
        // authority, so the gate declines and this falls through to the
        // `ensure_loaded` reload below, which clears the evict marker
        // and re-integrates authoritative scheduler state.
        if let Some(hash) = self.get_whole_hash(canonical_id) {
            return Some(hash);
        }
        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }
        if self.ensure_loaded(canonical_id) {
            return self.get_whole_hash(canonical_id);
        }
        None
    }

    pub(crate) fn cached_import_route_resolution(
        &self,
        canonical_id: &str,
        import_source: &str,
    ) -> Option<DependencyResolution> {
        // The project-global cache already fact-validates entries on
        // warm read (each candidate's `read_set_signature.facts`
        // re-walked against the live `StoreView`), so readers consume
        // the cache permissively here.
        // import_routes lives on DerivedRawState (D48 split).
        if self.is_canonical_evicted(canonical_id) {
            return None;
        }
        let derived = self.derived_raw_cache().get(canonical_id)?;
        let resolution = derived.import_routes.get(import_source).cloned()?;
        // A known-miss is NEVER served warm: a negative answer is not
        // evidence that the answer is still negative. The caller
        // re-resolves through the one owner-edge authority, where a warm
        // candidate whose exhausted probe set is unchanged is reused, so
        // the re-resolve is cheap rather than cold.
        //
        // A POSITIVE entry is a caller-supplied authoritative route and
        // serves until the caller replaces it. There is no host-memoised
        // positive class any more, and therefore no global
        // `content_generation` equality deciding whether one is still
        // true — that was the last global-generation warm-resolution
        // validity test in the session.
        if resolution.is_known_miss() {
            // Per-request audit attribution: the caller will recompute
            // the miss against the live workspace.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::KnownMissRouteRecomputed);
            }
            return None;
        }
        Some(resolution)
    }

    fn append_file_whole_and_route_fact_versions(
        &self,
        canonical_id: &str,
        known_shallow: Option<&crate::resolver_core::ShallowFileState>,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        // Ambient-view-first hash chain. `current_or_read_whole_hash`
        // already does `ensure_loaded` on view-miss inside a request, so the
        // only remaining fallback is the caller-provided `known_shallow`
        // hash (avoids a redundant ensure_loaded round-trip when the caller
        // already has shallow state in hand).
        let whole_hash = self
            .current_or_read_whole_hash(canonical_id)
            .or_else(|| known_shallow.map(|state| state.whole_hash));
        if let Some(hash) = whole_hash {
            let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        // Live-host probe. Prefer the caller-supplied shallow state, then
        // fall back to the WARM route-surface read
        // (`current_derived_fact_hash(Route)` — a pure store read). The ROUTE
        // arm of fact capture never materialises (the whole-hash arm
        // above may `ensure_loaded` a scheduler miss — a load, not an
        // artifact build): a dependency the compute never touched has
        // no route surface to record — its `FileWholeHash` fact above
        // already invalidates on any change to the owner's OWN content,
        // and the route movements that do NOT touch owner content (a
        // cross-file edge retarget — wildcard, named reexport, or import
        // target) are caught by the edge-gated warm read declining.
        // Materialising here would breadth-walk unrelated imports just to
        // sign the result.
        let route_hash = known_shallow
            .filter(|state| state.has_resolvable_surface())
            // A bare caller-supplied surface carries no edge-resolution
            // generation, so one with SHALLOW cross-file edges cannot be
            // proven edge-current — re-derive it through the edge-gated
            // warm read rather than hashing a possibly-stale baked edge.
            // The shallow COMPONENT predicate is the right gate here (not
            // the complete `IndexedReady` authority): `hash_route_surface`
            // digests only shallow-inventory data, so import-route-only
            // edges — invisible to this hash — cannot stale it.
            .filter(|state| !state.has_shallow_cross_file_edges())
            .map(crate::resolver_store::hash_route_surface)
            .or_else(|| {
                self.current_derived_fact_hash(
                    canonical_id,
                    crate::resolver_core::DerivedFactKind::Route,
                )
            });
        if let Some(hash) = route_hash {
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    fn append_file_whole_and_route_fact_versions_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        known_shallow: Option<&crate::resolver_core::ShallowFileState>,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        let whole_hash = ctx
            .authoritative_current_content_hash(canonical_id)
            .or_else(|| known_shallow.map(|state| state.whole_hash));
        if let Some(hash) = whole_hash {
            let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        let route_hash = known_shallow
            .filter(|state| state.has_resolvable_surface())
            .filter(|state| !state.has_shallow_cross_file_edges())
            .map(crate::resolver_store::hash_route_surface)
            .or_else(|| {
                ctx.indexed_for_current_content(canonical_id)
                    .filter(|indexed| indexed.shallow_state.has_resolvable_surface())
                    .and_then(|indexed| indexed.route_surface_hash())
            });
        if let Some(hash) = route_hash {
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    pub(crate) fn resolve_direct_imported_type_root_fast_path_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<(
        (String, verter_type_expr::TopLevelOwnerId, String),
        Vec<crate::resolver_core::FactVersionRef>,
    )> {
        // A published source snapshot already owns the parser-produced local
        // export surface and the exact owner-qualified declaration headers.
        // Use that header fact before joining the heavier IndexedReady lane.
        // This is deliberately conservative: aliases and default exports need
        // the route inventory to map exported names back to local symbols, and
        // duplicate owners are ambiguous, so all three fall through.
        let session_masks_dependency = ctx.active_session_view().is_some_and(|view| {
            view.overlay_content_hash_for(dep_canonical).is_some()
                || view.is_tombstoned(dep_canonical)
        });
        if !session_masks_dependency
            && imported_name != "default"
            && !self.is_canonical_evicted(dep_canonical)
        {
            let source_snapshot = self.scheduler.try_get_source(dep_canonical);
            let source_data = source_snapshot.as_ref().and_then(|snapshot| {
                snapshot.downcast_data::<crate::host_executor::HostSourceData>()
            });
            if let Some(source_data) = source_data {
                let has_direct_local_export =
                    source_data.parse.export_signatures.iter().any(|signature| {
                        signature.name == imported_name && signature.reexport_source.is_none()
                    });
                if has_direct_local_export {
                    let mut exact_owner = None;
                    let mut ambiguous_owner = false;
                    for declaration in source_data
                        .parse
                        .script_analysis
                        .declaration_entries
                        .iter()
                        .filter(|declaration| declaration.name == imported_name)
                    {
                        match exact_owner {
                            None => exact_owner = Some(declaration.owner),
                            Some(owner) if owner == declaration.owner => {}
                            Some(_) => {
                                ambiguous_owner = true;
                                break;
                            }
                        }
                    }
                    if !ambiguous_owner {
                        if let Some(owner) = exact_owner {
                            let mut facts =
                                vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                                    canonical_id: dep_canonical.to_string(),
                                    hash: source_data.parse.whole_hash,
                                }];
                            // Cross-file ROUTE fact — recorded ONLY from an
                            // already-materialized, content-pinned, parse-current
                            // artifact (a `get`, never an `ensure`): the fast
                            // path must not index the dependency just to derive
                            // the hash. When available, the fact
                            // uses the SAME `hash_route_surface` derivation the
                            // store-view root lookup publishes, so warm validation
                            // round-trips; when the artifact is absent, the
                            // dep's `FileWholeHash` remains the (sufficient)
                            // covering fact for a direct local export.
                            if let Some(indexed) = self
                                .project_type_store
                                .indexed()
                                .get(dep_canonical, source_data.parse.whole_hash)
                            {
                                if self.indexed_surface_is_current(dep_canonical, &indexed)
                                    && indexed.shallow_state.has_resolvable_surface()
                                {
                                    facts.push(
                                        crate::resolver_core::FactVersionRef::DerivedFactHash {
                                            canonical_id: dep_canonical.to_string(),
                                            kind: crate::resolver_core::DerivedFactKind::Route,
                                            hash: crate::resolver_store::hash_route_surface(
                                                &indexed.shallow_state,
                                            ),
                                        },
                                    );
                                }
                            }
                            return Some((
                                (dep_canonical.to_string(), owner, imported_name.to_string()),
                                facts,
                            ));
                        }
                    }
                }
            }
        }

        let dep_serve = self.routed_shallow_state_serve_with_context(ctx, dep_canonical)?;
        let shallow = std::sync::Arc::clone(&dep_serve.state);
        let (target_canonical, target_symbol) = match shallow.export_target(imported_name)? {
            crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                original_name,
                ..
            } => {
                let next_canonical =
                    self.resolve_route_type_edge(dep_canonical, source_specifier)?;
                (next_canonical, original_name.clone())
            }
            crate::resolver_core::ExportTarget::Local { owner, symbol_name } => {
                let Some(import_target) = shallow.import_target_in(*owner, symbol_name.as_str())
                else {
                    if !shallow.has_type_symbol_in(*owner, symbol_name.as_str())
                        && !shallow.has_value_symbol_in(*owner, symbol_name.as_str())
                    {
                        return None;
                    }

                    let resolved = (dep_canonical.to_string(), *owner, symbol_name.clone());
                    if !dep_serve.store_published {
                        return Some((resolved, Vec::new()));
                    }

                    let mut facts = Vec::new();
                    let mut seen = rustc_hash::FxHashSet::default();
                    self.append_file_whole_and_route_fact_versions_with_context(
                        ctx,
                        dep_canonical,
                        Some(shallow.as_ref()),
                        &mut facts,
                        &mut seen,
                    );
                    return Some((resolved, facts));
                };
                let next_canonical = self.resolve_route_type_edge(
                    dep_canonical,
                    import_target.source_specifier.as_str(),
                )?;
                (next_canonical, import_target.imported_name.clone())
            }
        };
        let normalized_target = self
            .normalized_analysis_canonical(target_canonical.as_str())
            .into_owned();
        let (leaf_owner, leaf_symbol, target_hash, target_store_published) = {
            let target_serve =
                self.routed_shallow_state_serve_with_context(ctx, normalized_target.as_str())?;
            let target_state = &target_serve.state;
            match target_state.export_target(target_symbol.as_str())? {
                crate::resolver_core::ExportTarget::Local { owner, symbol_name }
                    if target_state
                        .import_target_in(*owner, symbol_name.as_str())
                        .is_none() =>
                {
                    (
                        *owner,
                        symbol_name.clone(),
                        target_state.whole_hash,
                        target_serve.store_published,
                    )
                }
                _ => return None,
            }
        };

        // ReturnOnly never publishes — imported-root fast-path arm. A
        // fenced (ReturnOnly) provider or target serve carried baked
        // reexport/import edges resolved against the superseded route
        // table, while the fact list below is read from the LIVE
        // post-mutation state — an entry the read-side fact rail cannot
        // reject. Serve the resolved tuple to this caller with EMPTY
        // facts: `ImportedRootDb`'s strict admission treats an empty fact
        // signature as the negative-cache pattern (value returned, never
        // persisted), so the next query re-resolves cold against the live
        // workspace.
        if !dep_serve.store_published || !target_store_published {
            return Some(((normalized_target, leaf_owner, leaf_symbol), Vec::new()));
        }

        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        self.append_file_whole_and_route_fact_versions_with_context(
            ctx,
            dep_canonical,
            Some(shallow.as_ref()),
            &mut facts,
            &mut seen,
        );
        let target_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: normalized_target.clone(),
            hash: target_hash,
        };
        if seen.insert(target_fact.clone()) {
            facts.push(target_fact);
        }

        Some(((normalized_target, leaf_owner, leaf_symbol), facts))
    }

    pub(crate) fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        component_meta_trace_custom!(
            "resolve_local_import_symbol_target",
            format!("owner={} requested={}", dep_canonical, resolved_name),
        );
        let shallow = self.shallow_file_state(dep_canonical)?;
        let import_target = shallow.import_target(resolved_name)?;
        let next_canonical =
            self.resolve_route_type_edge(dep_canonical, &import_target.source_specifier)?;
        component_meta_trace_custom!(
            "resolve_local_import_symbol_target_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical,
                resolved_name,
                import_target.source_specifier,
                next_canonical,
                import_target.imported_name
            ),
        );
        Some((next_canonical, import_target.imported_name.clone()))
    }

    /// Get-or-build the [`OwnerImportSurface`](crate::owner_import_surface::OwnerImportSurface)
    /// for `owner_canonical`. of the project-global cache overhaul:
    /// direct owner imports resolve exactly once per owner version and every
    /// downstream stage reads the same surface entry.
    ///
    /// Cache identity is `(owner_canonical, owner_whole_hash)`. Stale owner
    /// versions miss at the key level; building populates
    /// `project_type_store().owner_import_surfaces()` with the fully-resolved
    /// root for each direct import binding in the owner file.
    ///
    /// Builds a fresh `HostStoreView` at every call. Production
    /// resolver-tier code on the per-component-meta hot path MUST use
    /// [`Self::owner_import_surface_with_store_view`] instead. The
    /// `#[allow(dead_code)]` annotation is intentional during the 6.c
    /// substrate window — the wrapper is retained for the host's
    /// stand-alone entry-point contract and becomes live again when
    /// callers without a request-bound view (test fixtures, ambient-tier
    /// consumers) invoke it.
    #[allow(dead_code)]
    pub(crate) fn owner_import_surface(
        &self,
        owner_canonical: &str,
    ) -> Option<Arc<crate::owner_import_surface::OwnerImportSurface>> {
        // Cold-seed-routed: the warm-surface probe inside
        // `owner_import_surface_with_store_view` reads through the
        // cold-seed-aware `RequestStoreView`, so a known-stale
        // (`ReturnOnly`) read fails the `get_with_view` fact validation
        // closed and the surface re-resolves cold rather than validating a
        // stale entry against a superseded snapshot.
        let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &cold_seed, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        self.owner_import_surface_with_store_view(ctx.store_view(), owner_canonical)
    }

    /// View-bound variant of [`Self::owner_import_surface`].
    ///
    /// Validates the cached surface against the supplied request-bound
    /// view instead of requesting another root capture.
    /// Same correctness contract: R3/R26/R28 fact-validation rejects a
    /// stale entry on the next read; the producer's
    /// `validated_at_generation` ProjectGeneration fencing is
    /// preserved.
    pub(crate) fn owner_import_surface_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        owner_canonical: &str,
    ) -> Option<Arc<crate::owner_import_surface::OwnerImportSurface>> {
        let shallow = self.shallow_file_state(owner_canonical)?;
        let whole_hash = shallow.whole_hash;
        let surfaces = self.project_type_store.owner_import_surfaces();
        surfaces.get_or_compute(self, owner_canonical, whole_hash, view, || {
            component_meta_trace_custom!(
                "owner_import_surface_build",
                format!("owner={}", owner_canonical),
            );

        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work. The carrier validates only file-content
        // whole-hashes; a `ProjectGeneration` reset (tsconfig /
        // path-alias / SDK / workspace-folder change) bumps no file
        // content, so without this snapshot a
        // `bump_project_generation_and_evict` racing this cold publish
        // could strand a stale-by-project-generation surface whose
        // carrier still validates. `OwnerImportSurfaceDb::get_with_view`
        // rejects on warm read when the live generation differs.
        let validated_at_generation = self.project_type_store().current_project_generation();

        // `OwnerImportSurfaceDb::get_or_compute` owns the fact tracer around
        // this complete closure. The producer accumulates direct-chain facts
        // as value-side observations; the DB re-observes and finalises them,
        // rebuilds the admitted signature, and owns the only write.
        let cold_body = || {
            // (local_name, final_canonical, final_exported_name, target_whole_hash)
            type SurfaceBuildEntry = (Arc<str>, Arc<str>, Arc<str>, Option<Hash16>);
            let mut entries: Vec<SurfaceBuildEntry> =
                Vec::with_capacity(shallow.import_targets.len());
            // R3/R26/R28: accumulate every chain fact observed by
            // each direct import's route walk. The producer threads these
            // into the surface's `fact_dep_signature` so dependent caches
            // detect intermediate barrel changes via fact-validation
            // alone (no eager invalidation required).
            let mut chain_facts: Vec<crate::resolver_core::FactVersionRef> = Vec::new();
            let mut seen_facts: rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef> =
                rustc_hash::FxHashSet::default();
            // Direct-import specifiers the build SKIPPED because they did
            // not resolve. A skipped specifier is dependency-set-derived
            // negative state: the surface computed without it must be
            // rooted in the owner's `ImportRoute` fact rail (below) so it
            // goes stale the moment the missing target appears.
            let mut unresolved_sources: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            // ReturnOnly never publishes — fenced-walk signal. A
            // per-binding route walk that consumed a FENCED (ReturnOnly)
            // serve returns its resolved root with EMPTY route facts
            // (the strict-admission negative-cache pattern `RouteDb` /
            // `ImportedRootDb` already honour — served, never
            // persisted). The surface producer must honour the same
            // signal: a surface folding such a binding binds the
            // fenced-resolved target while its direct-hop facts
            // validate against the live view — refuse admission below.
            // The same empty-facts signal also covers a walk whose
            // resolution could not be fact-rooted at all (an absent
            // target, an unproduce-able wildcard hash) — equally
            // inadmissible negative state, fail-closed.
            let mut unrooted_route_walk = false;
            let mut resolution_refused = false;
            for (local_name, target) in shallow.import_targets.iter() {
                let resolved_canonical_id = match self
                    .resolve_type_dependency_canonical(owner_canonical, &target.source_specifier)
                {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        let Some(canonical) = admitted.into_result() else {
                            unresolved_sources.insert(target.source_specifier.clone());
                            continue;
                        };
                        canonical
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => {
                        resolution_refused = true;
                        unresolved_sources.insert(target.source_specifier.clone());
                        continue;
                    }
                };

                // Observe the producer's dep-side `FileWholeHash` for the
                // resolved_canonical_id BEFORE following the route walk;
                // even when the route returns an empty facts list (e.g.
                // a stable-miss negative result), the surface's
                // fact_dep_signature still observes the direct hop.
                self.append_file_whole_and_route_fact_versions(
                    resolved_canonical_id.as_str(),
                    None,
                    &mut chain_facts,
                    &mut seen_facts,
                );

                // Per-request hoist: thread the already-built
                // request view down through the imported-root resolver
                // instead of building a fresh owned snapshot per call
                // (the diagnostic's named hot-path site at
                // `imported_type_root.rs:49`).
                let (final_identity, route_facts) = self
                    .resolve_imported_type_root_with_facts_with_store_view(
                        view,
                        resolved_canonical_id.as_str(),
                        target.imported_name.as_str(),
                    );
                unrooted_route_walk |= route_facts.is_empty();
                for fact in route_facts.iter() {
                    if seen_facts.insert(fact.clone()) {
                        chain_facts.push(fact.clone());
                    }
                }

                let Some(final_identity) = final_identity else {
                    unresolved_sources.insert(target.source_specifier.clone());
                    continue;
                };
                let final_canonical = final_identity.canonical_id.to_string();
                let final_name = final_identity.symbol_name.to_string();

                let target_hash = self
                    .shallow_file_state(final_canonical.as_str())
                    .map(|s| s.whole_hash);

                entries.push((
                    Arc::from(local_name.as_str()),
                    Arc::from(final_canonical),
                    Arc::from(final_name),
                    target_hash,
                ));
            }
            (
                entries,
                chain_facts,
                unresolved_sources,
                unrooted_route_walk,
                resolution_refused,
            )
        };
        let (
            entries,
            mut chain_facts,
            unresolved_sources,
            unrooted_route_walk,
            resolution_refused,
        ) = cold_body();

        if resolution_refused {
            return crate::cache_runtime::singleflight::ComputeAdmission::Failed;
        }

        // A per-binding route walk returning the empty-facts strict-admission
        // signal is served but never published. This also covers a fenced serve
        // adopted cross-thread inside the route DB, where the owner tracer cannot
        // see the original serve. Direct traced non-cacheability and overflow are
        // enforced independently by `OwnerImportSurfaceDb` after this closure.
        if unrooted_route_walk {
            self.provenance
                .owner_import_surface_fenced_serve_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let surface = crate::owner_import_surface::build_owner_import_surface(
                Arc::from(owner_canonical),
                whole_hash,
                entries,
                chain_facts,
                validated_at_generation,
            );
            return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                value: surface,
                reason: crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
            };
        }

        // Root every SKIPPED unresolved direct import on the owner's
        // resolution-witness rail — the same rail that roots unresolvable
        // wildcard route misses. Resolving the skipped specifiers fans the
        // sealed transactions' observations (including the exhausted probe
        // set for each miss), so the recorded witness MOVES the moment a
        // skipped specifier becomes resolvable and the cached surface
        // (computed without that import) declines. Coverage is structural:
        // the witness is built FROM the skipped specifiers. A REFUSED
        // resolution refuses admission (fail-closed): the surface is still
        // served to the caller, and the next request cold-recomputes.
        if !unresolved_sources.is_empty() {
            let required: Vec<String> = unresolved_sources.into_iter().collect();
            match self.import_route_witness_for_specifiers(owner_canonical, &required) {
                Some(witness) => {
                    for fact in witness {
                        if !chain_facts.contains(&fact) {
                            chain_facts.push(fact);
                        }
                    }
                }
                None => {
                    self.provenance
                        .owner_import_surface_unrooted_skip_refusals
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Refusing the surface's OWN admission only protects
                    // this producer. An ENCLOSING traced cold compute (a
                    // semantic-memo build, a component-meta proof
                    // producer) observes NOTHING from the refusal: no
                    // route walk runs for a skipped specifier, the
                    // canonical-resolve miss records no tracer fact, and
                    // the owner's whole hash does not move when the
                    // missing target appears — so a value folding this
                    // surface (e.g. "this binding is not an imported
                    // root") would publish warm with no read-side rail to
                    // reject it. Mark the non-cacheability rail by hand,
                    // exactly as the unrootable-wildcard route exit does for
                    // the route-walk shape of the same hole. This is a VALID
                    // (Complete) unrootable surface, NOT a partial result —
                    // cache non-admission only, never request partiality.
                    crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                        crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
                    );
                    let surface = crate::owner_import_surface::build_owner_import_surface(
                        Arc::from(owner_canonical),
                        whole_hash,
                        entries,
                        chain_facts,
                        validated_at_generation,
                    );
                    return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                        value: surface,
                        reason: crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
                    };
                }
            }
        }

        let surface = crate::owner_import_surface::build_owner_import_surface(
            Arc::from(owner_canonical),
            whole_hash,
            entries,
            chain_facts,
            validated_at_generation,
        );
        crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface)
        })
    }

    /// Resolve a direct owner import binding to its final root identity via
    /// the owner import surface. Returns `(final_canonical,
    /// final_exported_name)` matching the legacy
    /// [`Self::resolve_imported_type_root`] contract for direct owner
    /// imports, but sourced from one cached surface per owner version.
    /// Callers that already have the owner canonical plus a local binding
    /// name must prefer this method over `resolve_imported_type_root`
    /// so direct owner imports resolve exactly once per owner version. The
    /// `resolve_imported_type_root` helper remains the authority for
    /// transitive chain walks inside route/barrel code.
    ///
    /// Test-only bare wrapper. Production callers go through
    /// `ctx.resolve_owner_direct_import` (which routes through the
    /// request-bound `_with_store_view`); the test-only arm on
    /// `impl ResolverContext for VerterHost` reaches this wrapper on
    /// test fixtures that call `host.<method>` directly.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        // Cold-seed-routed: the warm direct-import surface read inside
        // `_with_store_view` reads through the cold-seed-aware
        // `RequestStoreView`, so a stale read fails the warm validation
        // closed and the direct import re-resolves cold.
        let cold_seed = self.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(self, &cold_seed, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        self.resolve_owner_direct_import_with_store_view(
            ctx.store_view(),
            owner_canonical,
            local_name,
        )
    }

    /// View-bound variant of [`Self::resolve_owner_direct_import`].
    ///
    /// Threads the request-bound view down through
    /// [`Self::owner_import_surface_with_store_view`].
    pub(crate) fn resolve_owner_direct_import_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        let surface = self.owner_import_surface_with_store_view(view, owner_canonical)?;
        // `Arc<str>` borrows as `&str`, so the surface lookup uses the
        // caller-supplied slice directly without allocating a fresh Arc.
        let binding = surface.bindings.get(local_name)?;
        Some((
            binding.canonical_id.as_ref().to_string(),
            binding.exported_name.as_ref().to_string(),
        ))
    }
}
