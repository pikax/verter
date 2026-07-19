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
    dep_edges_from_resolutions, is_raw_import_specifier_id, is_runtime_script_target,
    HostShallowImportResolver,
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
    /// and consults the view's direct accessors
    /// ([`crate::resolver_core::StoreView::tracks_file`] for the self-root
    /// arm; [`crate::resolver_core::StoreView::derived_hash_for`] for the
    /// `ImportRoute` arm) to determine WHICH check rejected. Fires
    /// exactly one audit event per call:
    ///
    /// * `PreparedDeclBundleRejectEntryMissing` — `rejected_fact ==
    ///   None && candidate_count == 0` (no cache entry at all).
    /// * `PreparedDeclBundleRejectSelfRootUntracked` — `FileWholeHash`
    ///   self-root, `view.tracks_file(canonical)` is `false`.
    /// * `PreparedDeclBundleRejectSelfRootHashMismatch` —
    ///   `FileWholeHash` self-root, tracked but stored hash differs.
    /// * `PreparedDeclBundleRejectImportRouteAbsent` —
    ///   `DerivedFactHash { kind: ImportRoute }` for the bundle's
    ///   canonical, `view.derived_hash_for` returns `None`.
    /// * `PreparedDeclBundleRejectImportRouteMismatch` — same but the
    ///   stored hash differs from the view's hash.
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
            Some(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: fact_canonical,
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                ..
            }) if fact_canonical == canonical_id => {
                if view
                    .derived_hash_for(
                        fact_canonical,
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    )
                    .is_some()
                {
                    verter_audit::AuditEvent::PreparedDeclBundleRejectImportRouteMismatch
                } else {
                    verter_audit::AuditEvent::PreparedDeclBundleRejectImportRouteAbsent
                }
            }
            _ => verter_audit::AuditEvent::PreparedDeclBundleRejectOther,
        };
        obs.record_event(event);
    }

    /// View-bound variant of [`Self::prepared_decl_bundle`].
    ///
    /// `view` is a borrow into the request-bound [`HostStoreView`] built
    /// at the request entry point. The warm-hit path validates against
    /// this view instead of building a fresh one — eliminating the
    /// per-call full-workspace snapshot the pre-6.c rail performed.
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
                    match self.materialize_prepared_decl_bundle(view, canonical_id) {
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
        if !state.has_resolvable_surface() && state.import_targets.is_empty() {
            return None;
        }
        let (dep_edges, _import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(route_canonical_id, state.as_ref());

        let script_setup_type_bindings = if bundle_canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(bundle_canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // Canonicalize re-export-hop imports against the request-bound view
        // (overlay-aware). Route facts are NOT retained here: per R17 this
        // overlay bundle is never admitted to the shared cache, so its fact
        // rail is irrelevant — the per-call bundle is request-scoped.
        let (import_canonicalization, _import_route_facts) = self
            .build_prepared_import_canonicalization(ctx.store_view(), state.as_ref(), &dep_edges);

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
    ) -> (
        rustc_hash::FxHashMap<String, String>,
        Option<crate::resolver_core::ResolverHash16>,
    ) {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        let mut dep_edges = rustc_hash::FxHashMap::default();
        let mut import_routes = rustc_hash::FxHashMap::default();
        let mut seen_sources = rustc_hash::FxHashSet::default();

        for target in state.import_targets.values() {
            if !seen_sources.insert(target.source_specifier.clone()) {
                continue;
            }

            let cached_resolution =
                self.cached_import_route_resolution(canonical_id, target.source_specifier.as_str());
            let resolved: Option<String> = if let Some(resolution) = cached_resolution.as_ref() {
                self.prefer_type_dependency_target_from_resolution(
                    canonical_id,
                    target.source_specifier.as_str(),
                    resolution,
                )
                .or_else(|| {
                    if Self::import_route_is_known_miss(resolution) {
                        None
                    } else if !(target.canonical_id.is_empty()
                        || declaration_file && is_runtime_script_target(&target.canonical_id))
                    {
                        Some(target.canonical_id.clone())
                    } else {
                        self.resolve_route_type_edge(canonical_id, target.source_specifier.as_str())
                    }
                })
            } else if !(target.canonical_id.is_empty()
                || declaration_file && is_runtime_script_target(&target.canonical_id))
            {
                Some(target.canonical_id.clone())
            } else {
                self.resolve_route_type_edge(canonical_id, target.source_specifier.as_str())
            };
            let Some(resolved) = resolved else {
                continue;
            };

            dep_edges.insert(target.source_specifier.clone(), resolved.clone());
            import_routes.insert(
                target.source_specifier.clone(),
                cached_resolution.unwrap_or(crate::types::DependencyResolution {
                    specifier: target.source_specifier.clone(),
                    resolved_canonical_id: Some(resolved.clone()),
                    possible_canonical_ids: vec![resolved],
                }),
            );
        }

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        (dep_edges, import_route_hash)
    }

    /// Canonicalize a file's import targets to their FINAL defining-file
    /// identity through the SAME route authority the carrier fallback /
    /// dispatch fallthrough use, so the eager/prepared `name_resolution`
    /// records the FINAL definition rather than the intermediate barrel.
    ///
    /// For each import target whose resolved canonical is known, both rails are
    /// VIEW-AWARE, FULL-CHAIN-fact resolvers: the TYPE-export authority
    /// ([`Self::resolve_imported_type_root_with_facts_with_store_view`]) is tried
    /// FIRST, then the VALUE-export authority
    /// ([`Self::resolve_value_export_root_with_facts_with_store_view`]). Each
    /// returns the full route-chain fact list. Every resolvable import is recorded
    /// with its exact final `(canonical, owner, symbol)` identity, including a
    /// direct import whose defining file is cold. Prepared declarations require
    /// that owner-bearing identity; an unresolved route is left absent so
    /// preparation fails with `MissingExternalOwner` rather than synthesizing an
    /// owner from the importing declaration.
    ///
    /// The type-export route walk is symbol-space-NEUTRAL (it follows ANY
    /// re-export, value-only included, and terminates at ANY local symbol), so it
    /// already canonicalizes a CROSS-FILE value re-export and records its full
    /// chain — the value rail is reached only when the type route resolves to the
    /// barrel itself (a local binding, no cross-file hop), where its distinct work
    /// is the terminal SAME-FILE `typeof` value-alias peel. Both rails return the
    /// full chain regardless, so whichever wins records the same complete fact set.
    ///
    /// The returned `route_facts` carry every barrel/re-export participant's
    /// version (each participant's `FileWholeHash` + route surface) so the
    /// bundle's fact rail INVALIDATES on a retarget ANYWHERE on the winning chain —
    /// a content edit to a re-export clause (or a route change) on the IMMEDIATE
    /// barrel OR a MULTI-HOP inner barrel (`owner → barrel → mid → final`,
    /// retargeting `mid`) misses the warm bundle. Both rails resolve
    /// graph-native; neither materialises a dependency's `whole_env()` during
    /// prep.
    fn build_prepared_import_canonicalization(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        state: &crate::resolver_core::ShallowFileState,
        dep_edges: &rustc_hash::FxHashMap<String, String>,
    ) -> (
        crate::resolver_core::prepared_decl::ImportCanonicalization,
        Vec<crate::resolver_core::FactVersionRef>,
    ) {
        use verter_semantic::analysis::type_solver::ResolvedRootIdentity;

        let mut canonicalization =
            crate::resolver_core::prepared_decl::ImportCanonicalization::default();
        let mut route_facts: Vec<crate::resolver_core::FactVersionRef> = Vec::new();
        let interner = self.project_type_store().identity_interner();

        for (local, target) in state.owner_import_targets.iter() {
            // The import's resolved barrel canonical (dep_edges → target →
            // raw): the same precedence `resolve_import_target` applies.
            let barrel_canonical =
                if let Some(resolved) = dep_edges.get(&target.source_specifier).cloned() {
                    resolved
                } else if !target.canonical_id.is_empty() {
                    target.canonical_id.clone()
                } else {
                    // No resolvable canonical means there is no authoritative
                    // target owner to publish.
                    continue;
                };
            if barrel_canonical.is_empty() {
                continue;
            }

            // TYPE rail: resolve the direct target or walk its re-export chain to
            // the final defining declaration, recording every participant's
            // facts. The view-aware route authority materializes a cold target
            // when required and returns the exact defining owner. A same-file
            // result is still recorded: it is the sole authoritative owner for a
            // direct import, not a recoverable default.
            let (type_final, type_chain_facts) = self
                .resolve_imported_type_root_with_facts_with_store_view(
                    view,
                    &barrel_canonical,
                    &target.imported_name,
                );
            if let Some(type_final) = type_final
                .as_ref()
                .filter(|identity| identity.canonical_id.as_ref() != barrel_canonical.as_str())
            {
                canonicalization
                    .final_resolution
                    .insert(local.clone(), type_final.clone());
                route_facts.extend(type_chain_facts.iter().cloned());
                continue;
            }

            // VALUE rail. Reached ONLY when the type rail above did NOT produce a
            // DIFFERENT final canonical — i.e. `type_final_canonical ==
            // barrel_canonical`, which covers BOTH a same-file resolution (the
            // barrel declares/re-aliases the name itself) AND a type-route
            // miss/fallback (the resolver returns `(barrel, name)` when the route
            // does not resolve). The symbol-space-neutral type rail follows every
            // CROSS-FILE re-export hop (value-only included) and short-circuits
            // the moment it lands cross-file, so in the reached case the only
            // remaining work is the SAME-FILE terminal `typeof` value-alias peel
            // (`export const V: typeof realImpl = realImpl` on the barrel →
            // `realImpl`) — this rail's distinct live contribution.
            // The cross-file fact completeness is delivered by the type rail's
            // full-chain walk above, not here. Resolve through the VIEW-AWARE,
            // FULL-CHAIN-fact resolver (symmetric with the type rail, same final
            // normalization); NEVER routes through `peel_value_decl_alias` /
            // `base_eval_env_arc` / `whole_env()` (the legacy whole-env oracle
            // path) during prep.
            let (value_final, value_chain_facts) = self
                .resolve_value_export_root_with_facts_with_store_view(
                    view,
                    &barrel_canonical,
                    &target.imported_name,
                );
            if let Some(value_final) = value_final {
                if value_final.canonical_id != barrel_canonical
                    || value_final.name != target.imported_name
                {
                    canonicalization.final_resolution.insert(
                        local.clone(),
                        ResolvedRootIdentity::new_in_owner(
                            interner.intern(&value_final.canonical_id),
                            value_final.owner,
                            interner.intern(&value_final.name),
                        ),
                    );
                    route_facts.extend(value_chain_facts.iter().cloned());
                    continue;
                }
            }
            if let Some(type_final) = type_final {
                canonicalization
                    .final_resolution
                    .insert(local.clone(), type_final);
                route_facts.extend(type_chain_facts.iter().cloned());
            }
        }

        (canonicalization, route_facts)
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
        if !state.has_resolvable_surface() && state.import_targets.is_empty() {
            // Surface-emptiness is a property of the SERVED artifact,
            // not necessarily of live content — carry the serve's
            // publication status so the flight lane can judge the
            // miss's reproducibility.
            return Some(BundleMaterialization::SurfaceEmpty {
                serve_published: serve.store_published,
            });
        }

        let (dep_edges, _legacy_import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());
        // Canonicalize re-export-hop imports to the FINAL defining file; the
        // accumulated barrel route facts join the bundle's fact rail below.
        let (import_canonicalization, import_route_facts) =
            self.build_prepared_import_canonicalization(view, state.as_ref(), &dep_edges);
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

        // ImportRoute fact MUST match the view's snapshot. The
        // [`crate::resolver_store::HostStoreView::build`] /
        // `snapshot_tracked_import_route_hashes` route delegates to
        // [`crate::host_manage::component_meta_methods::VerterHost::generation_current_import_route_hash`]
        // which reads the canonical's `IndexedReady.import_routes`
        // or the `DerivedRawState.import_routes` map. This
        // routed-shallow path admits bundles for declaration
        // files (`.d.ts` / `.d.mts` / `.d.cts`) where neither layer
        // may be populated; in that case the view inserts the
        // canonical with the `empty_import_route_hash` sentinel via
        // `unwrap_or(empty_import_route_hash)`. Using the live
        // generation hash here keeps the bundle's stored fact
        // identical to the view's snapshot, eliminating the
        // perpetual `PreparedDeclBundleRejectImportRouteMismatch` /
        // `PreparedDeclBundleRejectImportRouteAbsent` warm-read
        // rejection loop the pre-fix
        // `prepared_decl_bundle_route_dep_edges` shape produced.
        let live_import_route_hash = self.generation_current_import_route_hash(canonical_id);

        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: state.whole_hash,
        }];
        if let Some(import_route_hash) = live_import_route_hash {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }
        // Fold in the barrel route-chain facts so a re-export retarget on any
        // walked barrel invalidates this bundle (no stale-served final root).
        facts.extend(import_route_facts);

        // Promote the just-materialised canonical's facts into the request
        // overlay BEFORE the bundle insert. Without this promotion the
        // request-entry [`HostStoreView`] snapshot misses the
        // just-published canonical (the snapshot is built once at
        // request entry — entries published after that lookup are
        // invisible to the view), and every subsequent warm-validation of the bundle's
        // stored `(FileWholeHash, ImportRoute)` facts falls through to
        // the base view's untracked-canonical reject. The next read
        // therefore triggers a fresh cold rebuild, and the loop
        // repeats every time the canonical is consulted. With the
        // promotion the overlay knows the canonical's authoritative
        // hashes and the next warm read matches.
        //
        // `route_hash` is `None` when the shallow state has no
        // resolvable surface — mirrors
        // `current_route_surface_hash` (only
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
        view.promote_route_completion(
            canonical_id,
            state.whole_hash,
            route_hash,
            live_import_route_hash,
        );

        // Strict admission. Bundles always carry `FileWholeHash` — gated
        // on the routed-shallow serve's publication status (ReturnOnly
        // never publishes), the same gate as the standard cold producer
        // below (`materialize_prepared_decl_bundle`): a bundle rooted at a
        // FENCED `IndexedReady` is served to this caller WITHOUT
        // admission. The fenced artifact's route surface was resolved
        // against superseded state, while the fact versions above
        // (`state.whole_hash`; the LIVE
        // `generation_current_import_route_hash`) validate against a
        // fresh view — so the read-side fact rail cannot reject the entry
        // and this gate is the only correct refusal point. The flag flows
        // BY VALUE through `routed_shallow_state_serve` (see
        // `RoutedShallowServe`), so the gate works with or without an
        // installed `RequestContext`. The `promote_route_completion` call
        // above stays ungated: the request overlay is request-scoped
        // (discarded with the request), not a shared publication.
        if serve.store_published {
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
        view: &dyn crate::resolver_core::StoreView,
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
        if !state.has_resolvable_surface() && state.import_targets.is_empty() {
            // Surface-emptiness is a property of the SERVED artifact,
            // not necessarily of live content — carry the serve's
            // publication status so the flight lane can judge the
            // miss's reproducibility.
            return Some(BundleMaterialization::SurfaceEmpty {
                serve_published: serve.store_published,
            });
        }
        let (dep_edges, _legacy_import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());

        // 4. Build script-setup type bindings for Vue SFCs (once per bundle).
        // Non-Vue files get an empty map — zero cost.
        let script_setup_type_bindings = if canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // 4b. Canonicalize re-export-hop imports to the FINAL defining file
        // through the shared route authority, against the request-bound `view`
        // (currentness-preserving — a stale seed fails the route cache's
        // validation closed, never serves a stale final root). The accumulated
        // barrel route facts are folded into the bundle's fact rail (step 6) so
        // a barrel retarget invalidates this bundle.
        let (import_canonicalization, import_route_facts) =
            self.build_prepared_import_canonicalization(view, state.as_ref(), &dep_edges);

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
        // The ImportRoute fact hash MUST match what the live
        // `HostStoreView` snapshots. Both view-side producers
        // (`HostStoreView::build` line 684, IndexedReady loop; and
        // `snapshot_tracked_import_route_hashes`, file-without-indexed
        // loop) use
        // [`crate::host_manage::component_meta_methods::VerterHost::generation_current_import_route_hash`]
        // uniformly — that method reads the canonical's
        // `IndexedReady.import_routes` (or the live
        // `DerivedRawState.import_routes` map) AND re-resolves
        // known-miss specifiers against the current workspace
        // generation. Using the static `IndexedReady.import_route_hash`
        // here diverges from the view whenever a file has a
        // known-miss specifier that has since become resolvable: the
        // view re-derives the hash including the now-positive
        // resolution, the bundle's stored fact keeps the stale miss,
        // and every warm-read validator rejection routes through
        // `PreparedDeclBundleRejectImportRouteMismatch` /
        // `PreparedDeclBundleRejectImportRouteAbsent`. Worse, the
        // routed-shallow path at
        // `materialize_prepared_decl_bundle_from_routed_shallow`
        // already uses `generation_current_import_route_hash` (line
        // 502), so the two cold-materialise paths admitted bundles
        // with DIFFERENT `ImportRoute` hashes for the same canonical
        // depending on which producer the cold-walk hit first — a
        // gratuitous extra rebuild per known-miss flip.
        //
        // Unify on the dynamic hash. For a fully-resolved file (no
        // known-miss specifiers) `generation_current_import_route_hash`
        // takes the cached-hash fast path and equals
        // `facts.import_route_hash` — zero overhead in the common
        // case.
        let whole_hash = facts.whole_hash;
        let mut fact_versions = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: whole_hash,
        }];
        if let Some(import_route_hash) = self.generation_current_import_route_hash(canonical_id) {
            fact_versions.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }
        // Fold in the barrel route-chain facts so a re-export retarget on any
        // walked barrel invalidates this bundle (no stale-served final root).
        fact_versions.extend(import_route_facts);

        // 7. Insert into the stable cache. Strict admission — bundles always
        // carry `FileWholeHash` — gated on the IndexedReady serve's
        // publication status (ReturnOnly never publishes): a bundle rooted
        // at a FENCED IndexedReady is served to this caller WITHOUT
        // admission. The fenced artifact's route surface was resolved
        // against superseded state, while the fact versions above
        // (`facts.whole_hash`; the LIVE
        // `generation_current_import_route_hash`) validate against a fresh
        // view — so the read-side fact rail cannot reject the entry and
        // the admission gate is the only correct refusal point. The gate
        // keys on the VALUE-flowed `store_published` flag, not the
        // request-sticky `current_request_result_is_partial` channel:
        // the value flag needs no installed `RequestContext` (the suppress
        // mark is a silent no-op without one) and stays per-serve-precise,
        // whereas the request flag is sticky-coarse (an unrelated earlier
        // partial in the same request would wrongly decline a COMPLETE
        // bundle built from a store-current artifact — the A2 signal split
        // documented on `observe_component_meta_read_suppress`).
        if serve.store_published {
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

    pub(crate) fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        self.prepared_value_decl_in(
            canonical_id,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol_name,
        )
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
                    for required_name in state.required_import_names_in(owner, symbol_name) {
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

/// The coherent route-surface bundle `build_indexed_route_surface`
/// produces — everything on `IndexedReady` that derives from route
/// resolution rather than from the file's own content.
struct BuiltIndexedRouteSurface {
    import_routes: Arc<rustc_hash::FxHashMap<String, crate::types::DependencyResolution>>,
    import_route_hash: Option<Hash16>,
    route_hash: Option<Hash16>,
    shallow_state: Arc<crate::resolver_core::ShallowFileState>,
    edge_generation: u64,
}

impl VerterHost {
    /// Build the COHERENT route surface for one canonical from its
    /// content-addressed payload parts: resolved `import_routes`, the
    /// `ShallowFileState` (with canonicalised cross-file edges + the Vue
    /// `default` synth), `route_hash`, `import_route_hash`, and the
    /// `edge_generation` stamp. Shared by the full materialise closure
    /// and the edge-refresh path — the refresh rebuilds exactly this
    /// surface from the RETAINED payload, never a patched map.
    fn build_indexed_route_surface(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        snapshot: &crate::types::FileAnalysisSnapshot,
        route_inventory: &Arc<
            verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory,
        >,
        decl_bodies: &Arc<crate::decl_body_memo::DeclBodyMemo>,
        eval_source: Option<&str>,
    ) -> BuiltIndexedRouteSurface {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");

        // Canonicalize shallow import/reexport edges once during module-facts
        // materialization. Later resolver stages read these facts instead of
        // treating compile-cache/store-view import-route maps as truth.
        //
        // Seed import routes from DerivedRawState if present (set by
        // `set_import_dependencies` — D48 split: import_routes live on
        // DerivedRawState as a sub-mirror of IndexedReady.import_routes).
        // Unstamped positives are authoritative caller-provided targets.
        // The per-entry freshness oracle
        // (`import_route_entry_is_generation_current`) gates everything
        // else: a HOST-MEMOIZED positive seeds only while its
        // capture-before-resolve stamp matches the live
        // `content_generation`, and a KNOWN-MISS seeds only while its
        // known-miss sidecar stamp matches — a stale negative must NOT
        // be re-baked under a fresh `edge_generation` (the file that
        // appeared may now satisfy it). Every skipped specifier
        // re-resolves through `resolve_missing` below.
        let mut import_routes = rustc_hash::FxHashMap::default();
        {
            if let Some(cc) = self.derived_raw_cache().get(canonical_id) {
                let live_generation = self.ws().content_generation();
                for (specifier, resolution) in cc.import_routes.iter() {
                    if !cc.import_route_entry_is_generation_current(
                        specifier,
                        resolution,
                        live_generation,
                    ) {
                        continue;
                    }
                    import_routes.insert(specifier.clone(), resolution.clone());
                }
            }
        }
        let mut required_import_sources = snapshot
            .imports
            .iter()
            .map(|import| {
                (
                    import.source.clone(),
                    // In declaration files (.d.ts), all imports are
                    // effectively type-only even without the `type`
                    // keyword. This ensures the TypeImport resolution
                    // path is used, which prefers .d.ts companions
                    // over .js runtime files.
                    if import.is_type_only || declaration_file {
                        verter_workspace::ResolveRequestKind::TypeImport
                    } else {
                        verter_workspace::ResolveRequestKind::EsmImport
                    },
                )
            })
            .collect::<Vec<_>>();
        required_import_sources.extend(snapshot.export_signatures.iter().filter_map(|export| {
            let source = export.reexport_source.clone()?;
            let kind = if declaration_file || export.is_type {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            Some((source, kind))
        }));
        required_import_sources.sort_by(|(left_source, left_kind), (right_source, right_kind)| {
            left_source.cmp(right_source).then_with(|| {
                let kind_rank = |kind: verter_workspace::ResolveRequestKind| match kind {
                    verter_workspace::ResolveRequestKind::TypeImport => 0u8,
                    verter_workspace::ResolveRequestKind::EsmImport => 1u8,
                    verter_workspace::ResolveRequestKind::RequireCall => 2u8,
                    verter_workspace::ResolveRequestKind::SfcSrcAttr => 3u8,
                };
                kind_rank(*left_kind).cmp(&kind_rank(*right_kind))
            })
        });
        required_import_sources.dedup();

        let mut resolve_memo: rustc_hash::FxHashMap<
            (String, verter_workspace::ResolveRequestKind),
            Option<String>,
        > = rustc_hash::FxHashMap::default();
        let mut resolve_missing = |specifier: &str,
                                   kind: verter_workspace::ResolveRequestKind,
                                   prefer_live_fallback: bool| {
            if import_routes.contains_key(specifier) {
                return;
            }
            let primary = resolve_memo
                .entry((specifier.to_string(), kind))
                .or_insert_with(|| {
                    self.ws()
                        .resolve_import(
                            canonical_id,
                            specifier,
                            verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind,
                            },
                        )
                        .map(|resolution| {
                            if kind == verter_workspace::ResolveRequestKind::TypeImport {
                                self.normalize_live_type_dependency_target(
                                    canonical_id,
                                    specifier,
                                    resolution.source_id.as_str(),
                                )
                            } else {
                                resolution.source_id
                            }
                        })
                })
                .clone();
            let resolved: Option<String> =
                if kind == verter_workspace::ResolveRequestKind::TypeImport {
                    primary
                        .or_else(|| self.fallback_relative_type_companion(canonical_id, specifier))
                        .or_else(|| {
                            if !prefer_live_fallback {
                                return None;
                            }
                            // ESM fallback for a type-route edge: normalize
                            // the effective target through declaration-
                            // companion preference, identically to the
                            // shared route-edge policy
                            // (`resolve_route_edge_canonical`). Recording the
                            // raw `source_id` here diverged the indexed
                            // shallow surface from route traversal and known-
                            // miss revalidation (so the indexed surface and route traversal never record divergent edge canonicals).
                            self.ws()
                                .resolve_import(
                                    canonical_id,
                                    specifier,
                                    verter_workspace::ResolutionContext {
                                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                        kind: verter_workspace::ResolveRequestKind::EsmImport,
                                    },
                                )
                                .map(|resolution| {
                                    self.normalize_live_type_dependency_target(
                                        canonical_id,
                                        specifier,
                                        resolution.source_id.as_str(),
                                    )
                                })
                        })
                } else {
                    primary
                };
            let mut resolution = DependencyResolution {
                specifier: specifier.to_string(),
                resolved_canonical_id: None,
                possible_canonical_ids: Vec::new(),
            };
            if let Some(resolved) = resolved {
                resolution.resolved_canonical_id = Some(resolved.clone());
                resolution.possible_canonical_ids.push(resolved);
            }
            import_routes.insert(specifier.to_string(), resolution);
        };

        // Capture the workspace generation BEFORE any edge is
        // canonicalized. The import/wildcard edges resolved below bake
        // target `canonical_id`s that depend on the dependency file set;
        // recording the generation here (and never re-stamping it after)
        // means a file-set change during or after this build leaves
        // `edge_generation < content_generation()`, so the shared
        // edge-currency oracle judges the surface stale and forces a
        // re-resolve — never a torn entry served as fresh.
        let edge_generation = self.ws().content_generation();

        for (source, kind) in &required_import_sources {
            resolve_missing(source, *kind, true);
        }

        // Re-resolve every `export *` wildcard reexport source through the
        // shared route-edge policy (`resolve_route_edge_canonical`) — the
        // SAME TS-first policy the overlay materialiser uses — so the
        // indexed wildcard `canonical_id`s agree with the overlay producer
        // and `hash_route_surface` hashes identically.
        //
        // A bare `export *` source IS captured in `snapshot.export_signatures`
        // (an `ExportSignature` with `reexport_source = Some(..)`), so the
        // `resolve_missing` loop above already resolved it. But for a PLAIN
        // (non-type) `export *` that loop classifies the source as
        // `EsmImport` and bakes the runtime `.js` `source_id` WITHOUT
        // TS-first normalization — diverging from the overlay surface,
        // which resolves the `.d.ts` companion. This pass therefore
        // OVERWRITES (does not skip) any `resolve_missing`-baked entry with
        // the policy result, so a `.js`-with-`.d.ts`-companion wildcard
        // source resolves to its declaration companion on every producer.
        // `resolve_route_edge_canonical` returning `None` (an unresolvable
        // source) leaves the `resolve_missing` known-miss in place.
        for wildcard in &route_inventory.wildcard_reexports {
            let source = wildcard.source.as_str();
            if let Some(resolved) = self.resolve_route_edge_canonical(canonical_id, source) {
                import_routes.insert(
                    source.to_string(),
                    DependencyResolution {
                        specifier: source.to_string(),
                        resolved_canonical_id: Some(resolved.clone()),
                        possible_canonical_ids: vec![resolved],
                    },
                );
            }
        }

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        let dep_edges = dep_edges_from_resolutions(&import_routes);
        let resolver = HostShallowImportResolver {
            dep_edges: &dep_edges,
        };
        // Synthesise the implicit component `default` value symbol from
        // type-based macros, dispatched through the framework registry's
        // synthesis leg — see `framework::synth` for the policy and the
        // per-framework legs (Vue's macro synth, Svelte's, …).
        self.provenance
            .shallow_state_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut shallow_state_inner =
            crate::resolver_core::ShallowFileState::from_route_inventory_with_resolver(
                whole_hash,
                Arc::clone(route_inventory),
                Arc::clone(decl_bodies),
                &resolver,
            );
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
        let shallow_state = Arc::new(shallow_state_inner);

        let route_hash = shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

        BuiltIndexedRouteSurface {
            import_routes: Arc::new(import_routes),
            import_route_hash,
            route_hash,
            shallow_state,
            edge_generation,
        }
    }

    /// Edge-refresh materialise: rebuild ONLY the route surface of a
    /// content-current `IndexedReady` whose edges or `project_generation`
    /// stamp went stale (a route-resolution mutation or a dependency
    /// file-set change while the owner's content stayed put).
    ///
    /// The content-addressed payload — `raw_source`, `eval_source`,
    /// `framework_parse`, `snapshot`, `script_analysis`,
    /// `route_inventory`, the memo-owned whole-env demand product,
    /// the shallow symbol bodies'
    /// inputs — is REUSED (`whole_hash` unchanged, no re-read, no
    /// re-parse); the COHERENT route surface (`import_routes`,
    /// `ShallowFileState` route edges, `route_hash`, `import_route_hash`)
    /// rebuilds through the same `build_indexed_route_surface` the full
    /// materialise uses, and the artifact republishes with fresh
    /// `edge_generation` / `project_generation` stamps.
    ///
    /// Runs inside the `indexed_singleflight` flight. Carries the same
    /// pre-publish fence as the full materialise: a generation move
    /// detected at the fence serves the result without publishing
    /// (ReturnOnly) — the returned outcome carries `published == false`
    /// so the flight is not retained and followers re-validate.
    ///
    /// `flight_workspace_generation` / `flight_project_generation` are
    /// flight-captured by the caller BEFORE the parse-env reuse gate
    /// that authorises entering this refresh (never re-read here): the
    /// fence must cover the gate→publish window because a
    /// parse-env-moving mutation in that window — which always bumps
    /// `project_generation` — would otherwise stamp a CURRENT
    /// `project_generation` onto a payload parsed under the superseded
    /// env. `indexed_surface_is_current` short-circuits on a current
    /// project stamp as proof of parse-env currency, so that
    /// forged-current entry is the one publish the read-side gates
    /// cannot reject; the fence comparing against the pre-gate capture
    /// declines it (ReturnOnly).
    ///
    /// The wholesale `stale.snapshot` reuse rests on snapshots carrying
    /// SPECIFIER-level route inputs only (`snapshot.imports` sources,
    /// `export_signatures.reexport_source`) — never baked resolved
    /// canonicals; the rebuild re-resolves every edge against the live
    /// file set. Pinned behaviorally by
    /// `non_wildcard_route_fact_retargets_via_edge_refresh_on_warm_host`:
    /// the refresh reuses the pre-retarget snapshot and must still
    /// resolve the NEW target — a snapshot that baked the old canonical
    /// would re-bake it and fail that test.
    fn refresh_indexed_route_surface(
        &self,
        canonical_id: &str,
        stale: &Arc<crate::project_type_store::IndexedReady>,
        flight_workspace_generation: u64,
        flight_project_generation: u64,
    ) -> crate::project_type_store::IndexedFlightOutcome {
        self.provenance
            .indexed_ready_edge_refreshes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        self.fire_materialize_seam();

        let surface = self.build_indexed_route_surface(
            canonical_id,
            stale.whole_hash,
            stale.snapshot.as_ref(),
            &stale.route_inventory,
            // The content-addressed declaration-body memo is REUSED
            // across route-only edge refreshes (same content generation;
            // bodies are canonical-free) — only the route surface and
            // its per-state classification caches rebuild.
            stale.shallow_state.decl_bodies(),
            Some(stale.eval_source.as_ref()),
        );
        let indexed = Arc::new(crate::project_type_store::IndexedReady {
            whole_hash: stale.whole_hash,
            shallow_state: surface.shallow_state,
            import_routes: surface.import_routes,
            import_route_hash: surface.import_route_hash,
            route_hash: surface.route_hash,
            edge_generation: surface.edge_generation,
            project_generation: flight_project_generation,
            // The refresh reuses `framework_parse` / `eval_env`, so it is
            // entered only when the stale artifact's parse env equals
            // the live one at the reuse gate — carry the (equal) stamp
            // forward. A parse-env move AFTER that gate bumps
            // `project_generation` and trips the fence below, so the
            // carried stamp is never published as forged-current.
            parse_env_hash: stale.parse_env_hash,
            raw_source: Arc::clone(&stale.raw_source),
            eval_source: Arc::clone(&stale.eval_source),
            framework_parse: stale.framework_parse.clone(),
            script_analysis: stale.script_analysis.clone(),
            export_signatures: stale.export_signatures.clone(),
            snapshot: Arc::clone(&stale.snapshot),
            route_inventory: Arc::clone(&stale.route_inventory),
            declares_interface_app_config: stale.declares_interface_app_config,
            macro_hot_mirror: crate::structural_carrier_producer::MacroHotMirror::default(),
        });
        // PRE-PUBLISH FENCE — same ReturnOnly contract as the full
        // materialise: serve, never publish a known-superseded surface.
        // As there, the fence→insert pair is not atomic; a mutation
        // landing in the window leaves a stale-stamped insert that every
        // reader rejects via the content-pinned lookup +
        // `indexed_surface_is_current` (see the full materialise's fence
        // comment for the per-mutation-class breakdown). That read-side
        // rejection argument holds ONLY because the published stamps are
        // the flight captures taken BEFORE the parse-env reuse gate —
        // never live re-reads: a mid-flight mutation always leaves the
        // landed stamps strictly older than the live generations, so the
        // reader gates see the entry as stale. A live re-read here would
        // stamp the post-mutation generation onto the pre-mutation
        // payload (forged-current — see the fn docs), which no read-side
        // gate can reject.
        if self.ws().content_generation() != flight_workspace_generation
            || self.project_type_store.current_project_generation() != flight_project_generation
        {
            return crate::project_type_store::IndexedFlightOutcome {
                indexed,
                published: false,
            };
        }
        self.project_type_store
            .indexed()
            .insert(Arc::from(canonical_id), Arc::clone(&indexed));
        crate::project_type_store::IndexedFlightOutcome {
            indexed,
            published: true,
        }
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

    /// Test-only seam fired after the edge-refresh parse-env reuse gate
    /// passes and before the refresh flight runs — see the
    /// `edge_refresh_gate_seam_hook` field docs. Same
    /// clone-out-then-invoke discipline as
    /// [`Self::fire_materialize_seam`].
    #[cfg(test)]
    pub(crate) fn fire_edge_refresh_gate_seam(&self) {
        let hook = self.edge_refresh_gate_seam_hook.lock().clone();
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
                // A content-current artifact is reusable ONLY while
                // edge-current. An artifact whose baked cross-file edges
                // are stale (a dependency appeared / retargeted while this
                // file's content stayed put) must be rebuilt so its edges
                // re-resolve against the live file set — the materialiser
                // below re-inserts under the same content key, replacing
                // the stale candidate with a fresh `edge_generation`.
                // Falling through (not returning) routes an edge-stale hit
                // into the rebuild.
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

            let surface = self.build_indexed_route_surface(
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
                shallow_state: surface.shallow_state,
                import_routes: surface.import_routes,
                import_route_hash: surface.import_route_hash,
                route_hash: surface.route_hash,
                edge_generation: surface.edge_generation,
                project_generation: flight_project_generation,
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
            // * A foreign-content mutation (`content_generation`
            //   advanced): `indexed_surface_is_current` →
            //   `route_surface_is_edge_current` rejects any surface
            //   with cross-file edges whose `edge_generation` predates
            //   the move; an edge-FREE surface is insensitive to the
            //   dependency file set, so serving it is sound.
            // * A route-resolution / config mutation
            //   (`project_generation` advanced):
            //   `indexed_surface_is_current` rejects on the stale
            //   `project_generation` stamp (edge-free surfaces
            //   additionally require parse-env equality — every
            //   parse-env-moving mutation bumps `project_generation`).
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
            // Edge-refresh fence generations — flight-captured BEFORE
            // any read, in particular BEFORE the parse-env reuse gate
            // below (the full materialise's own captures live inside
            // `materialize`, equally before its env read). The refresh
            // publishes under THESE stamps and fences against them: a
            // parse-env-moving mutation landing after this capture —
            // which always bumps `project_generation` — either fails
            // the gate (env already moved when compared) or trips the
            // refresh fence (generation moved after the gate passed).
            // Capturing after the gate instead would stamp the
            // post-mutation generation onto a payload parsed under the
            // superseded env — a forged-current entry
            // `indexed_surface_is_current` cannot reject (a current
            // project stamp short-circuits as proof of parse-env
            // currency).
            let flight_workspace_generation = self.ws().content_generation();
            let flight_project_generation = self.project_type_store.current_project_generation();
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
                // The content identity is unchanged — only the ROUTE
                // surface is stale (edge generation or project stamp).
                // Refresh it from the retained content-addressed payload:
                // no re-read, no re-parse, no env/analysis rebuild — the
                // coherent route surface (import_routes, route edges,
                // route_hash, import_route_hash) rebuilds and republishes
                // with fresh stamps. This REPLACES the full re-parse
                // edge-stale rebuild. The refresh REUSES `framework_parse` /
                // `eval_env`, so it is valid only while the owner's parse
                // environment (the R21 parse dimension) is unchanged — a
                // moved parse env falls through to the full re-materialise
                // (re-parse under the live env).
                if self.host_view_env_hashes_for(canonical_id).parse_env_hash
                    == candidate.parse_env_hash
                {
                    #[cfg(test)]
                    self.fire_edge_refresh_gate_seam();
                    return Ok(self.refresh_indexed_route_surface(
                        canonical_id,
                        &candidate,
                        flight_workspace_generation,
                        flight_project_generation,
                    ));
                }
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
        // R3/R26/R28: the shared per-entry freshness oracle. A
        // known-miss must invalidate once `content_generation` advances
        // past its admission stamp — a NEW canonical may now satisfy the
        // previously-unresolvable specifier. HOST-MEMOIZED positives are
        // the same dependency-set-derived class (a `.d.ts` companion or
        // a more-specific sibling can retarget them while the owner's
        // content stays put): a stamped positive serves only while its
        // capture-before-resolve stamp equals the live generation; the
        // caller re-resolves and re-admits otherwise. Caller-supplied
        // authoritative routes (`set_import_dependencies`) carry no
        // positive stamp and serve until replaced.
        let current = self.ws().content_generation();
        if !derived.import_route_entry_is_generation_current(import_source, &resolution, current) {
            if resolution.is_known_miss() {
                // Per-request audit attribution: the known-miss entry
                // is stale relative to the current `content_generation`
                // — caller will recompute against the live workspace.
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(verter_audit::AuditEvent::KnownMissRouteRecomputed);
                }
            }
            return None;
        }
        if resolution.is_known_miss() {
            // Per-request audit attribution: the known-miss entry
            // revalidated successfully against the current generation
            // — caller short-circuits without re-resolving.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::KnownMissRouteRevalidated);
            }
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
        // (`current_route_surface_hash` — a pure store read). The ROUTE
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

    pub(in crate::host_manage) fn resolve_direct_imported_type_root_fast_path(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<(
        (String, verter_type_expr::TopLevelOwnerId, String),
        Vec<crate::resolver_core::FactVersionRef>,
    )> {
        let dep_serve = self.routed_shallow_state_serve(dep_canonical)?;
        let shallow = std::sync::Arc::clone(&dep_serve.state);
        let (target_canonical, target_symbol) = match shallow.export_target(imported_name)? {
            crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id,
                ..
            } => {
                let next_canonical = if canonical_id.is_empty() {
                    self.resolve_route_type_edge(dep_canonical, source_specifier)?
                } else {
                    canonical_id.clone()
                };
                (next_canonical, original_name.clone())
            }
            crate::resolver_core::ExportTarget::Local { owner, symbol_name } => {
                let import_target = shallow.import_target_in(*owner, symbol_name.as_str())?;
                let next_canonical = if import_target.canonical_id.is_empty() {
                    self.resolve_route_type_edge(
                        dep_canonical,
                        import_target.source_specifier.as_str(),
                    )?
                } else {
                    import_target.canonical_id.clone()
                };
                (next_canonical, import_target.imported_name.clone())
            }
        };
        let normalized_target = self
            .resolve_eval_dependency_canonical(target_canonical.as_str())
            .unwrap_or(target_canonical);
        let (leaf_owner, leaf_symbol, target_hash, target_store_published) = {
            let target_serve = self.routed_shallow_state_serve(normalized_target.as_str())?;
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
        self.append_file_whole_and_route_fact_versions(
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
        let next_canonical = if import_target.canonical_id.is_empty() {
            self.resolve_route_type_edge(dep_canonical, &import_target.source_specifier)?
        } else {
            import_target.canonical_id.clone()
        };
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
    /// view instead of building a fresh one — eliminating the per-call
    /// full-workspace snapshot the legacy rail performed at this site.
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
            for (local_name, target) in shallow.import_targets.iter() {
                let resolved_canonical_id = if target.canonical_id.is_empty() {
                    match self.resolve_type_dependency_canonical(
                        owner_canonical,
                        &target.source_specifier,
                    ) {
                        Some(canonical) => canonical,
                        None => {
                            unresolved_sources.insert(target.source_specifier.clone());
                            continue;
                        }
                    }
                } else {
                    target.canonical_id.clone()
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
            )
        };
        let (entries, mut chain_facts, unresolved_sources, unrooted_route_walk) = cold_body();

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

        // Root every SKIPPED unresolved direct import in the owner's
        // `ImportRoute` fact rail — the same rail that roots unresolvable
        // wildcard route misses. `generation_current_import_route_hash`
        // re-resolves the owner's known-miss specifiers against the live
        // workspace on warm validation, so the recorded fact MOVES the
        // moment a skipped specifier becomes resolvable and the cached
        // surface (computed without that import) declines. When the rail
        // cannot cover every skipped specifier the entry is refused
        // admission (fail-closed): the surface is still served to the
        // caller, and the next request cold-recomputes.
        if !unresolved_sources.is_empty() {
            let required: Vec<String> = unresolved_sources.into_iter().collect();
            match self
                .generation_current_import_route_hash_covering_sources(owner_canonical, &required)
            {
                Some(hash) => {
                    let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                        canonical_id: owner_canonical.to_string(),
                        kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                        hash,
                    };
                    if !chain_facts.contains(&fact) {
                        chain_facts.push(fact);
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
