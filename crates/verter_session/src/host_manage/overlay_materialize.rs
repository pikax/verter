//! Overlay [`IndexedReady`](crate::project_type_store::IndexedReady)
//! materialiser. Owns the host-side cold path for building an overlay
//! candidate from `view.source(canonical)` content and publishing it
//! into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! as a multi-candidate sibling of the base host's IndexedReady.
//!
//! When the bound [`SessionView`](crate::session_view::SessionView)
//! carries an explicit overlay for the canonical, the candidate is
//! published under an
//! [`overlay_scoped`](crate::file_artifact_store::FileArtifactKey::overlay_scoped)
//! key — the overlay content hash plus the view's overlay-set
//! discriminator — so it stays isolated from the base artifact (always
//! the [`base`](crate::file_artifact_store::FileArtifactKey::base)
//! key) and from other sessions, even when the overlay source bytes are
//! identical to the base file.
//!
//! The resolver-tier seal scope reaches this body via
//! [`crate::resolver_core::ResolverContext::materialize_overlay_indexed_ready`];
//! the impl on [`crate::VerterHost`] delegates here.

use std::sync::Arc;

use crate::types::{DependencyResolution, Hash16};
use crate::VerterHost;

use super::{dep_edges_from_resolutions, is_raw_import_specifier_id, HostShallowImportResolver};

/// The two canonical identities an overlay artifact is keyed by.
///
/// An overlay [`IndexedReady`](crate::project_type_store::IndexedReady)
/// is published into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
/// under a [`FileArtifactKey`](crate::file_artifact_store::FileArtifactKey)
/// whose three components do NOT all come from one canonical id — they
/// span two distinct identities, and conflating them is a keying defect:
///
/// * **`raw_overlay_owner`** — the canonical the session edited /
///   requested. Every [`SessionView`](crate::session_view::SessionView)
///   overlay-state lookup keys by exactly this string: `source(raw)`,
///   `content_hash_for(raw)`, `overlay_content_hash_for(raw)`,
///   `overlay_artifact_discriminator(raw)`, tombstones, overlay
///   iteration. So the overlay artifact's `content_hash` and its
///   `parse_env_hash` discriminator are derived from this id.
/// * **`analysis_canonical`** — the `normalized_analysis_canonical`
///   rewrite (e.g. a runtime `.js` whose `.d.ts` companion is the
///   analysis target). It is the analysis / parse / resolve target and
///   the [`FileArtifactKey::canonical`](crate::file_artifact_store::FileArtifactKey)
///   identity.
///
/// The two coincide for an ordinary `.ts` / `.tsx` / `.d.ts` file
/// (identity normalisation); they diverge for a `.js`-with-`.d.ts`-companion
/// canonical. This type is the single owner of overlay artifact key
/// construction: every reader builds its `FileArtifactKey` through
/// [`Self::overlay_artifact_key`] / [`Self::lookup_overlay_artifacts`]
/// instead of constructing the key ad hoc from one id (which can never
/// reconstruct a key whose `canonical` is normalised but whose
/// `content_hash` + discriminator are raw-derived).
#[derive(Debug, Clone)]
pub(crate) struct OverlayArtifactIdentity {
    /// The raw canonical the session edited / requested — keys all
    /// `SessionView` overlay state.
    raw_overlay_owner: String,
    /// The `normalized_analysis_canonical` rewrite — the analysis target
    /// and the `FileArtifactKey.canonical` identity.
    analysis_canonical: String,
}

impl OverlayArtifactIdentity {
    /// The raw overlay owner canonical — keys `SessionView` overlay
    /// state (`source`, `content_hash_for`, `overlay_content_hash_for`,
    /// `overlay_artifact_discriminator`, tombstones).
    #[inline]
    pub(crate) fn raw_overlay_owner(&self) -> &str {
        &self.raw_overlay_owner
    }

    /// The normalized analysis canonical — the analysis target and the
    /// `FileArtifactStore` identity.
    #[inline]
    pub(crate) fn analysis_canonical(&self) -> &str {
        &self.analysis_canonical
    }

    /// Build the exact overlay artifact [`FileArtifactKey`](crate::file_artifact_store::FileArtifactKey)
    /// for this identity under `view`.
    ///
    /// Reads the overlay content hash and the overlay-set discriminator
    /// under the **raw overlay owner** (the `SessionView` overlay maps
    /// are keyed there), and sets the key's `canonical` to the
    /// **normalized analysis canonical** (the `FileArtifactStore`
    /// identity the materialiser publishes under). When the view carries
    /// an explicit overlay-set discriminator for the raw owner the key
    /// is `overlay_scoped`; otherwise (a base-passthrough view) it is
    /// `base`. Returns `None` when the view reports no current content
    /// hash for the raw owner (unloaded / evicted / tombstoned).
    fn overlay_artifact_key(
        &self,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        let content_hash = view.content_hash_for(&self.raw_overlay_owner)?;
        Some(self.overlay_artifact_key_for_content(view, content_hash))
    }

    /// Build the artifact key for a CALLER-SUPPLIED content hash. READ
    /// paths use it via [`Self::overlay_artifact_key`]; the publish path
    /// uses the gated [`Self::overlay_publish_key_for_content`] instead.
    ///
    /// The base arm (no overlay-set discriminator for the raw owner)
    /// is the base-passthrough READ shape: a view with no overlay for
    /// the owner reads — and a fully overlay-FREE view publishes —
    /// the base artifact under its base key.
    fn overlay_artifact_key_for_content(
        &self,
        view: &dyn crate::session_view::SessionView,
        content_hash: Hash16,
    ) -> crate::file_artifact_store::FileArtifactKey {
        match view.overlay_artifact_discriminator(&self.raw_overlay_owner) {
            Some(discriminator) => crate::file_artifact_store::FileArtifactKey::overlay_scoped(
                Arc::from(self.analysis_canonical.as_str()),
                content_hash,
                discriminator,
            ),
            None => crate::file_artifact_store::FileArtifactKey::base(
                Arc::from(self.analysis_canonical.as_str()),
                content_hash,
            ),
        }
    }

    /// PUBLISH-side artifact key — the gated variant of
    /// [`Self::overlay_artifact_key_for_content`].
    ///
    /// The overlay materialiser publishes under the hash its flight
    /// actually built from (`indexed.whole_hash`), never a live
    /// `content_hash_for` re-read: on the base-passthrough branch a base
    /// upsert landing between the pre-publish fence and the key build
    /// would otherwise re-key the OLD-content artifact under the NEW
    /// hash's content-pinned key. The fence guarantees live == flight
    /// when publication proceeds, so readers (which rebuild the key from
    /// the live hash) reach the same key.
    ///
    /// ## The base-key publish gate
    ///
    /// When the view carries NO overlay-set discriminator for the raw
    /// owner, the artifact key falls back to the BASE key
    /// space. That fallback is sound on the publish path ONLY for an
    /// overlay-FREE view: the materialiser's route discovery
    /// ([`resolve_relative_overlay_candidate`]) probes
    /// `view.content_hash_for` / `view.source` for HELPER canonicals —
    /// not just the owner — so an owner the view does not mask can
    /// still bake an overlay-only helper route (and a tombstoned
    /// dependency can mask a base file out of resolution). Such an
    /// artifact is view-influenced and must never enter the base key
    /// space, where a base-host read would observe session route state.
    /// Returns `None` for exactly that case — discriminator absent AND
    /// the view carries any overlay or tombstone — and the publish site
    /// declines (serves the artifact ReturnOnly, publishes nothing).
    ///
    /// Production entry points never reach the decline: every caller of
    /// `materialize_overlay_indexed_ready_with_view` gates on
    /// `view.overlay_content_hash_for(owner).is_some()` (or passes a
    /// base-passthrough view with no overlays at all), and the
    /// overlay-bearing `SessionView` impls report a discriminator
    /// exactly when they report an overlay content hash. The gate
    /// enforces that invariant at the write boundary instead of
    /// trusting caller discipline.
    fn overlay_publish_key_for_content(
        &self,
        view: &dyn crate::session_view::SessionView,
        content_hash: Hash16,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        if view
            .overlay_artifact_discriminator(&self.raw_overlay_owner)
            .is_none()
            && (!view.overlay_canonicals().is_empty() || !view.tombstoned_canonicals().is_empty())
        {
            return None;
        }
        Some(self.overlay_artifact_key_for_content(view, content_hash))
    }

    /// Read the published overlay [`FileArtifacts`](crate::file_artifact_store::FileArtifacts)
    /// bundle for this identity through `view`.
    ///
    /// This is the host-owned replacement for the architecturally
    /// unsound single-`canonical` `SessionView::parse_artifacts`: it
    /// preserves BOTH ids (raw for the overlay hash + discriminator,
    /// normalized for the artifact-store `canonical`), so it reaches the
    /// exact key the overlay materialiser published — even when
    /// `normalize(raw) != raw`. A tombstoned raw owner reports no
    /// current content hash, so the lookup yields `None`.
    pub(crate) fn lookup_overlay_artifacts(
        &self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        let key = self.overlay_artifact_key(view)?;
        host.project_type_store().indexed().get_artifacts(&key)
    }
}

/// Discover an overlay-only candidate for a relative import.
///
/// When the workspace cannot resolve a relative `./foo`-style import
/// (e.g. because the helper file exists only as a session overlay and
/// has no disk presence yet), this helper consults the session view
/// for candidates that match common TypeScript/JS extensions and
/// returns the first one the view carries content for. Used by the
/// view-aware overlay materialiser to remove prewarm-order dependence
/// when an owner overlay imports overlay-only helpers.
///
/// Returns `None` when `specifier` is not a relative import, or when
/// no extension candidate resolves through `view.content_hash_for` /
/// `view.source`.
fn resolve_relative_overlay_candidate(
    view: &dyn crate::session_view::SessionView,
    owner_canonical: &str,
    specifier: &str,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let direct = crate::id::resolve_external(owner_canonical, specifier);
    // Try the directly-joined form first (specifier may already include
    // an extension).
    if !direct.is_empty()
        && (view.content_hash_for(direct.as_str()).is_some()
            || view.source(direct.as_str()).is_some())
    {
        return Some(direct);
    }
    // Iterate the standard TS/JS extension probe order. The set
    // mirrors `effective_target` precedence: `.d.ts` > `.d.cts` >
    // `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.
    const EXTENSIONS: &[&str] = &[
        ".d.ts", ".d.cts", ".d.mts", ".ts", ".tsx", ".js", ".jsx", ".cjs", ".mjs",
    ];
    for ext in EXTENSIONS {
        let candidate = format!("{direct}{ext}");
        if view.content_hash_for(candidate.as_str()).is_some()
            || view.source(candidate.as_str()).is_some()
        {
            return Some(candidate);
        }
    }
    // Index-style resolution (./theme/index.ts) — same extension order
    // applied to a `/index` suffix.
    for ext in EXTENSIONS {
        let candidate = format!("{direct}/index{ext}");
        if view.content_hash_for(candidate.as_str()).is_some()
            || view.source(candidate.as_str()).is_some()
        {
            return Some(candidate);
        }
    }
    None
}

impl VerterHost {
    /// Construct the [`OverlayArtifactIdentity`] for a raw requested
    /// canonical.
    ///
    /// Computes `analysis_canonical = normalized_analysis_canonical(raw)`
    /// once and pairs it with the raw owner. This is the single entry
    /// point every overlay-artifact reader uses to obtain the
    /// two-identity carrier — readers then build keys / read artifacts
    /// through the carrier rather than constructing `FileArtifactKey`s
    /// ad hoc from one id.
    pub(crate) fn overlay_artifact_identity(&self, raw_canonical: &str) -> OverlayArtifactIdentity {
        let analysis_canonical = self
            .normalized_analysis_canonical(raw_canonical)
            .into_owned();
        OverlayArtifactIdentity {
            raw_overlay_owner: raw_canonical.to_string(),
            analysis_canonical,
        }
    }

    /// View-aware overlay materialiser.
    ///
    /// Materialises an [`IndexedReady`](crate::project_type_store::IndexedReady)
    /// candidate for `canonical_id` from the view's overlay source and
    /// publishes it into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
    /// as a multi-candidate sibling of the base host's `IndexedReady`.
    /// Import-route resolution prefers overlay candidates surfaced by
    /// the supplied [`SessionView`](crate::session_view::SessionView).
    ///
    /// Owner overlays that import overlay-only helpers
    /// (`/src/Button.vue` importing `./theme`, `./schema`, `./tv` where
    /// none exists on disk) discover the helper's overlay canonical via
    /// `view.content_hash_for(candidate)` / `view.source(candidate)`,
    /// removing the prewarm-order dependence that would otherwise force
    /// helpers to be upserted before the owner.
    ///
    /// When the view carries an explicit overlay for `canonical_id`
    /// (`view.overlay_artifact_discriminator(...)` is `Some`) the
    /// candidate is published under an
    /// [`overlay_scoped`](crate::file_artifact_store::FileArtifactKey::overlay_scoped)
    /// key so it never collides with the base artifact — see the
    /// publish site below. An overlay-FREE view (no overlays, no
    /// tombstones — e.g. `HostView`) yields the base key and the
    /// candidate publishes as a base artifact: with nothing masked
    /// anywhere, every route probe reads base authority and the
    /// candidate is base-equivalent. An overlay-BEARING view whose
    /// overlays do not cover `canonical_id` is the dangerous middle
    /// case — route discovery can still see overlay-only helpers for
    /// OTHER canonicals — and the publish key gate
    /// ([`OverlayArtifactIdentity::overlay_publish_key_for_content`])
    /// declines the base-keyed publish for it.
    ///
    /// ## Source + content-hash authority
    ///
    /// The materialiser derives BOTH the overlay source and its
    /// content hash from the supplied
    /// [`SessionView`](crate::session_view::SessionView) — a single
    /// authority, so the two cannot disagree on freshness. A caller
    /// cannot pass a source / hash pair: separate parameters are how a
    /// stale-hash-paired-with-fresh-source bug arises (and can race
    /// under concurrent mutation). `view.content_hash_for(canonical)`
    /// is view-authoritative-current (overlay hash when masked,
    /// scheduler-authoritative base hash otherwise), and
    /// `view.source(canonical)` returns the exact bytes that hash
    /// covers. Returns `None` when the view carries no source for the
    /// canonical or no current content hash for it (unloaded / evicted
    /// / deleted).
    ///
    /// The `SessionView` overlay maps are keyed by the RAW canonical
    /// the caller requested, so every `view.*` lookup uses that raw id.
    /// The artifact-store key and the build / parse / resolve target
    /// use the `normalized_analysis_canonical` rewrite instead (e.g. a
    /// runtime `.js` whose `.d.ts` companion is the analysis target) —
    /// see the in-body comment for the split.
    pub(crate) fn materialize_overlay_indexed_ready_serve_with_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        // Two canonical ids are in play and MUST NOT be conflated —
        // they are carried together by [`OverlayArtifactIdentity`]:
        //
        // * `canonical_id` (`identity.raw_overlay_owner()`) — the RAW,
        //   un-normalised canonical the caller requested. The
        //   `SessionView` overlay maps (`source` / `content_hash_for` /
        //   `overlay_artifact_discriminator`) are keyed by exactly this
        //   string, so every `view.*` lookup below uses it. The
        //   overlay-priority callers gate on
        //   `view.overlay_content_hash_for(canonical)` under the same
        //   raw id, so the overlay content lives under the raw id and
        //   nowhere else.
        // * `analysis_canonical_id` (`identity.analysis_canonical()`) —
        //   the `normalized_analysis_canonical` rewrite (e.g. a runtime
        //   `.js` whose `.d.ts` companion is the analysis target). It
        //   is the artifact-store key identity and the build / parse /
        //   resolve target, so it is what `FileArtifactStore` lookups,
        //   `build_snapshot_from_source_state`, workspace import
        //   resolution and the publish key use.
        //
        // Normalising before the view lookups (the prior shape) read
        // the BASE companion source — or `None` — for any canonical
        // whose normalisation is non-identity, silently dropping the
        // overlay. The split keeps view reads on the raw id.
        //
        // The fast-path lookup and the publish below both route their
        // `FileArtifactKey` construction through `identity` so the
        // mixed-identity key (`canonical = normalised`, `content_hash`
        // + discriminator = raw-derived) is built in ONE place and
        // every downstream reader reconstructs the exact same key.
        let identity = self.overlay_artifact_identity(canonical_id);
        let analysis_canonical_id = identity.analysis_canonical();

        // Fast path: an overlay materialisation for the same content
        // hash already lives in the file-artifact store under the
        // overlay-scoped key (or the base key when the bound view
        // carries no overlay for this canonical). Multi-candidate
        // storage keeps base and overlay candidates separate, so this
        // lookup serves only the overlay. The key is built through
        // `identity` — `canonical` is the NORMALISED analysis canonical
        // (the artifact-store identity), `content_hash` + discriminator
        // are RAW-owner-derived — so it reconstructs exactly the key
        // the publish below writes under.
        if let Some(facts) = identity.lookup_overlay_artifacts(self, view) {
            // Reuse the cached overlay artifact ONLY while it is edge-current.
            // A wildcard-bearing overlay `IndexedReady` bakes its `export *`
            // edge `canonical_id`s at the workspace generation when they were
            // resolved; the artifact is keyed by overlay content hash, so a
            // BASE file-set change (a dependency appears / retargets) advances
            // `content_generation` without touching the overlay source and
            // would otherwise serve the stale baked edge. Falling through here
            // RE-MATERIALISES the overlay artifact from the overlay source
            // (re-resolving the edges against the live file set) — it must NOT
            // fall back to the base surface (that would be overlay-blindness).
            if self.indexed_surface_is_current(analysis_canonical_id, &facts.indexed) {
                // A store hit IS the published current overlay surface.
                return Some(crate::host_manage::prepared_decl::IndexedReadyServe {
                    indexed: Arc::clone(&facts.indexed),
                    store_published: true,
                });
            }
        }

        if analysis_canonical_id.is_empty() || is_raw_import_specifier_id(analysis_canonical_id) {
            return None;
        }

        // Overlay singleflight (the Canonical-Dependency-Cache collapse
        // contract): concurrent overlay requests for the SAME canonical
        // + overlay content + overlay-set discriminator collapse onto
        // one cold build. The lane key embeds all three identity
        // dimensions (NUL separators cannot occur in canonical ids), so
        // different sessions' overlays — and different overlay contents
        // — never share a lane. These view reads are lane IDENTITY only;
        // the flight body re-reads source/hash authoritatively under its
        // own fence stamps.
        let lane_hash = view.content_hash_for(canonical_id)?;
        let lane_discriminator = view.overlay_artifact_discriminator(canonical_id);
        let lane_key = format!(
            "overlay\u{0}{analysis_canonical_id}\u{0}{lane_hash:02x?}\u{0}{lane_discriminator:02x?}"
        );
        let singleflight = &self.resolver.runtime.indexed_singleflight;
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        };
        let flight_body = || -> Result<crate::project_type_store::IndexedFlightOutcome, ()> {
            // Re-check inside the flight — another flight may have
            // published while this claimant waited for the lane.
            if let Some(facts) = identity.lookup_overlay_artifacts(self, view) {
                if self.indexed_surface_is_current(analysis_canonical_id, &facts.indexed) {
                    return Ok(crate::project_type_store::IndexedFlightOutcome {
                        indexed: Arc::clone(&facts.indexed),
                        published: true,
                    });
                }
            }
            self.materialize_overlay_cold(&identity, canonical_id, view)
                .ok_or(())
        };
        // Bounded re-validation loop — the same contract as the base
        // `ensure_indexed_ready_serve` retry loop: a PUBLISHED outcome is a
        // joinable rendezvous; a FENCED outcome serves only the leader
        // (ReturnOnly); a follower re-runs against fresh state; the
        // bounded sustained-churn fallback carries its ReturnOnly status
        // to the admission gates through the suppression channel.
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_fenced: Option<Arc<crate::project_type_store::IndexedReady>> = None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result =
                match singleflight.run_retaining(lane_key.clone(), token, flight_body, |outcome| {
                    outcome.published
                }) {
                    Ok(run_result) => run_result,
                    Err(()) => return None,
                };
            let outcome = (*run_result.value).clone();
            if outcome.published {
                return Some(crate::host_manage::prepared_decl::IndexedReadyServe {
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
                // (symmetric with the follower fallback below). A
                // fenced-but-VALID serve is Complete, NOT partial — cache
                // non-admission only, never request partiality. The fenced
                // consumption ALSO flows by value (the TLS chokepoint flag)
                // so enclosing traced cold computes refuse admission.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                );
                return Some(crate::host_manage::prepared_decl::IndexedReadyServe {
                    indexed: outcome.indexed,
                    store_published: false,
                });
            }
            last_fenced = Some(outcome.indexed);
        }
        if last_fenced.is_some() {
            // Sustained-churn follower fallback: a fenced-but-VALID serve is
            // Complete, NOT partial — mark cache non-admission only, never
            // request partiality (the by-value `store_published == false`
            // and the fan-out both refuse shared-cache admission).
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
            );
        }
        last_fenced.map(
            |indexed| crate::host_manage::prepared_decl::IndexedReadyServe {
                indexed,
                store_published: false,
            },
        )
    }

    /// Test-only bare wrapper over
    /// [`Self::materialize_overlay_indexed_ready_serve_with_view`] that
    /// drops the publication status. PRODUCTION code must use the serve
    /// variant (the carrier is the only production accessor for an
    /// overlay `IndexedReady`).
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn materialize_overlay_indexed_ready_with_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.materialize_overlay_indexed_ready_serve_with_view(canonical_id, view)
            .map(|serve| serve.indexed)
    }

    /// The overlay cold build — runs INSIDE the overlay singleflight
    /// flight. The body mirrors the base `ensure_indexed_ready_serve`
    /// materialise closure but never touches the scheduler: the overlay
    /// source is the sole content authority for this candidate, and the
    /// candidate is published as a multi-candidate sibling of the base
    /// via `insert_artifacts`. Generation stamps are captured BEFORE any
    /// content read — on a base-passthrough view the source/hash reads
    /// consult the LIVE scheduler, so a pre-stamp read would leave a
    /// fence-invisible window in which a base mutation lands between the
    /// content read and the stamp capture.
    fn materialize_overlay_cold(
        &self,
        identity: &OverlayArtifactIdentity,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::project_type_store::IndexedFlightOutcome> {
        let analysis_canonical_id = identity.analysis_canonical();
        self.provenance
            .indexed_ready_materializes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Flight generation stamps, captured BEFORE ANY work — content
        // reads included: the pre-publish fence below compares against
        // these, so every mid-flight base file-set or route-resolution
        // mutation is detected. They are also the stamps the published
        // artifact carries (`edge_generation` is the generation at which
        // the wildcard/import edges are canonicalized; a later file-set
        // change leaves the surface edge-stale for the shared oracle).
        let flight_workspace_generation = self.ws().content_generation();
        let flight_project_generation = self.project_type_store.current_project_generation();
        // The R21 parse dimension the overlay parse below runs under —
        // same value-side stamp contract as the base materialise.
        let flight_parse_env_hash = self
            .host_view_env_hashes_for(analysis_canonical_id)
            .parse_env_hash;
        #[cfg(test)]
        self.fire_materialize_seam();
        // Content reads — ONE authority, now fence-covered.
        // `content_hash_for` is the view-authoritative CURRENT content
        // hash (overlay hash when masked, scheduler hash otherwise);
        // `source` returns the exact bytes that hash covers. Keyed by
        // the RAW `canonical_id` — the `SessionView` overlay maps are
        // keyed by the requested canonical, so a normalised id would
        // miss the overlay.
        let overlay_source = view.source(canonical_id)?;
        let overlay_whole_hash = view.content_hash_for(canonical_id)?;
        let raw_source: Arc<str> = Arc::clone(&overlay_source);
        let overlay_file_language = self.language_classifier.classify(analysis_canonical_id);
        // The overlay source never carries a scheduler carrier parse; a carrier
        // overlay (`.vue` / `.svelte`) runs the carrier parser ONCE here through
        // the counted chokepoint (the carrier-neutral producer) and everything
        // downstream reuses its framework-neutral artifact.
        let framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>> =
            crate::parse::build_carrier_parse_artifact_from_source(
                &overlay_file_language,
                raw_source.as_ref(),
                &self.provenance,
            );
        let whole_hash = overlay_whole_hash;

        // `eval_is_extracted_script` records whether the eval source is
        // the position-preserving extracted carrier script — the
        // predicate that lets the snapshot build below walk the
        // flight's single eval-program parse instead of re-parsing the
        // same script bytes.
        let (eval_source_text, eval_is_extracted_script) =
            Self::build_eval_script_source_with_extraction(
                canonical_id,
                raw_source.as_ref(),
                framework_parse.as_deref(),
            );
        let eval_source = Arc::<str>::from(eval_source_text);
        // Single source type + single eval-program parse — the arena
        // stays on this flight's stack. The source type derives from the
        // OVERLAY content (the pure derivation over `raw_source` +
        // `framework_parse`, the exact inputs the snapshot below is built
        // from) — NEVER from the scheduler stamp
        // (`imported_eval_source_type_for`), which covers BASE content:
        // an overlay flipping the script lang would parse the overlay
        // eval source under the stale base type (fatal parse → empty
        // env) while the snapshot reports the overlay lang — an
        // intra-artifact divergence on the single-env artifact. On a
        // base-passthrough view the pure derivation equals the scheduler
        // stamp (same pure function over the same content).
        let source_type = crate::parse::imported_eval_source_type(
            &overlay_file_language,
            framework_parse.as_deref(),
        );
        // THE single eval-program parse for this overlay flight —
        // performed and retained on the lazy lowering service's worker,
        // content-addressed by `(canonical, overlay whole_hash,
        // parse_env)`: identical overlay bytes share the parse (a pure
        // function of the bytes), while the overlay artifact's MEMO
        // below stays instance-isolated, so overlay body results never
        // populate a base read. The cold job builds only INDEX
        // products; zero declaration bodies lower here.
        let snapshot_key = crate::decl_lowering::SnapshotKey {
            canonical: Arc::from(analysis_canonical_id),
            whole_hash,
            parse_env_hash: flight_parse_env_hash,
        };
        struct ColdIndexProducts {
            header_index: verter_semantic::analysis::decl_headers::DeclHeaderIndex,
            analysis: verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource,
            snapshot: Option<crate::types::FileAnalysisSnapshot>,
            svelte_component_runes_mode: bool,
        }
        let job_canonical = analysis_canonical_id.to_string();
        let job_raw_source = Arc::clone(&raw_source);
        let job_framework_parse = framework_parse.clone();
        let job_scope = self.config.effective_scope();
        let job_provenance = Arc::clone(&self.provenance);
        let is_carrier = overlay_file_language.is_framework_carrier();
        // Pin the overlay's retained parse HERE (the cold-index parse) and
        // hand the lease to the overlay artifact's memo below, so the
        // header-index parse and every later overlay body demand share ONE
        // parse for the overlay artifact's life.
        let cold_lease = self
            .decl_lowering
            .acquire_lease(&snapshot_key, &eval_source, source_type);
        if cold_lease.parsed_now {
            self.provenance
                .eval_program_parses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // LEASE-ONLY run: `cold_lease` (acquired above) is held on this
        // stack, so the retained snapshot is pinned for the whole flight
        // and this cold-index job reuses it — the run cannot parse, per
        // the lease-only worker contract.
        let outcome = self.decl_lowering.run_leased(
            &snapshot_key,
            move |program: Option<&crate::ParsedEvalProgram>| {
                let svelte_component_runes_mode = program.is_some_and(|parsed| {
                    job_framework_parse.as_deref().is_some_and(|artifact| {
                        crate::parse::svelte_component_runes_mode(
                            artifact,
                            parsed.borrow_dependent(),
                        )
                    })
                });
                let (header_index, analysis) = match program {
                    Some(parsed) => {
                        let body = parsed.borrow_dependent();
                        (
                            verter_semantic::analysis::decl_headers::build_decl_header_index(
                                body,
                                parsed.source_str(),
                            ),
                            verter_parser::utils::oxc::script::type_inventory::analyze_external_type_program_headers(body),
                        )
                    }
                    None => Default::default(),
                };
                let vue_parsed = job_framework_parse
                    .as_deref()
                    .and_then(crate::typeinfo::adapters::vue::vue_parse);
                let snapshot = if let Some(parsed_sfc) = vue_parsed {
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
                    );
                    Some(VerterHost::build_snapshot_from_parse(parse))
                } else if is_carrier {
                    // A non-Vue carrier (Svelte) overlay: the snapshot's script
                    // program is the flight's retained eval program — walk it,
                    // parse nothing.
                    job_framework_parse.as_deref().map(|artifact| {
                        let parse = crate::parse::build_carrier_snapshot_from_artifact_with_program(
                            &job_canonical,
                            job_raw_source.as_ref(),
                            job_scope,
                            artifact,
                            &job_provenance,
                            VerterHost::framework_flight_script_program(
                                eval_is_extracted_script,
                                program,
                            ),
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
                    // overlay: a re-parse over the same bytes under the
                    // same source type panics identically, so the
                    // default-empty snapshot IS the parse outcome.
                    Some(crate::types::FileAnalysisSnapshot::default())
                };
                ColdIndexProducts {
                    header_index,
                    analysis,
                    snapshot,
                    svelte_component_runes_mode,
                }
            },
        );
        // The parse was already counted at lease acquisition above; the
        // cold-index run reused the pinned snapshot. A lease miss is
        // impossible by construction (`cold_lease` is held on this
        // stack), so the `None` arm is an invariant break: fail CLOSED
        // (no artifact) — loud in debug builds — never a transient
        // re-parse.
        let Some(products) = outcome else {
            debug_assert!(
                false,
                "overlay cold-index run missed its own held lease pin for {}",
                snapshot_key.canonical
            );
            return None;
        };
        let snapshot = Arc::new(products.snapshot.unwrap_or_default());
        let external_type_analysis = Arc::new(products.analysis);

        // The OVERLAY artifact's own declaration-body memo — a fresh
        // instance per overlay materialise, so overlay bodies are
        // memoized only on the overlay artifact that produced them and
        // can never answer a base demand. It holds the cold-index lease so
        // its body demands reuse that one pinned overlay parse.
        let decl_bodies = Arc::new(crate::decl_body_memo::DeclBodyMemo::new(
            snapshot_key,
            Arc::clone(&eval_source),
            framework_parse.clone(),
            source_type,
            products.svelte_component_runes_mode,
            Arc::clone(&self.decl_lowering),
            Arc::new(products.header_index),
            Arc::clone(&self.provenance),
            Some(cold_lease.lease),
        ));
        let declaration_file = analysis_canonical_id.ends_with(".d.ts")
            || analysis_canonical_id.ends_with(".d.mts")
            || analysis_canonical_id.ends_with(".d.cts");

        // Seed import routes from the host's DerivedRawState if the
        // session-side caller pre-populated them. Overlays use the
        // same `set_import_dependencies` surface as the base, so
        // overlay-specific deps land here when explicitly set. Same
        // gate as the base `build_indexed_route_surface` seed — the
        // per-entry freshness oracle: a generation-stamped
        // host-memoized positive seeds only while its stamp matches
        // the live `content_generation`, and a known-miss seeds only
        // while its known-miss sidecar stamp matches — a stale entry
        // re-resolves below instead of re-baking.
        let mut import_routes: rustc_hash::FxHashMap<String, DependencyResolution> =
            rustc_hash::FxHashMap::default();
        if let Some(cc) = self.derived_raw_cache().get(analysis_canonical_id) {
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

        let mut required_import_sources: Vec<(String, verter_workspace::ResolveRequestKind)> =
            snapshot
                .imports
                .iter()
                .map(|import| {
                    (
                        import.source.clone(),
                        if import.is_type_only || declaration_file {
                            verter_workspace::ResolveRequestKind::TypeImport
                        } else {
                            verter_workspace::ResolveRequestKind::EsmImport
                        },
                    )
                })
                .collect();
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

        // The flight stamps double as the artifact's edge/project stamps —
        // captured at flight start so the fence window covers the whole
        // build, parse included.
        let edge_generation = flight_workspace_generation;
        let project_generation = flight_project_generation;

        for (specifier, kind) in &required_import_sources {
            if import_routes.contains_key(specifier) {
                continue;
            }
            let kind = *kind;
            // `resolve_relative_overlay_candidate` probes the view's
            // overlay maps (`view.source` / `view.content_hash_for`)
            // for an overlay-only relative helper, so it takes the RAW
            // `canonical_id` — the owner the overlay is keyed under.
            // Workspace resolution uses the normalised
            // `analysis_canonical_id` (directory-equivalent for the
            // `.js`→`.d.ts` rewrite, and the base path's identity).
            let resolved: Option<String> = if kind
                == verter_workspace::ResolveRequestKind::TypeImport
            {
                // Type-route edges resolve through the SINGLE shared route-edge
                // policy (`resolve_route_edge_canonical`): TypeImport →
                // relative companion → ESM fallback, ALL normalized identically
                // to route traversal + known-miss revalidation. Recording the
                // RAW `EsmImport` `source_id` here (the runtime `.js`) diverged
                // the overlay's route facts from the base `IndexedReady` route
                // surface (which records the `.d.ts` companion) — a stale serve across
                // the overlay boundary. Then the
                // overlay-only relative candidate (overlay maps are keyed by the
                // RAW owner). `export *` wildcard sources flow through this same
                // chain, so normalizing it normalizes wildcard edges too.
                self.resolve_route_edge_canonical(analysis_canonical_id, specifier)
                    .or_else(|| resolve_relative_overlay_candidate(view, canonical_id, specifier))
            } else {
                let primary = resolve_memo
                    .entry((specifier.clone(), kind))
                    .or_insert_with(|| {
                        self.ws()
                            .resolve_import(
                                analysis_canonical_id,
                                specifier,
                                verter_workspace::ResolutionContext {
                                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                    kind,
                                },
                            )
                            .map(|resolution| resolution.source_id)
                    })
                    .clone();
                primary
                    .or_else(|| resolve_relative_overlay_candidate(view, canonical_id, specifier))
            };
            let mut resolution = DependencyResolution {
                specifier: specifier.clone(),
                resolved_canonical_id: None,
                possible_canonical_ids: Vec::new(),
            };
            if let Some(resolved) = resolved {
                resolution.resolved_canonical_id = Some(resolved.clone());
                resolution.possible_canonical_ids.push(resolved);
            }
            import_routes.insert(specifier.clone(), resolution);
        }

        // Re-resolve every `export *` wildcard reexport source through the
        // shared route-edge policy and OVERWRITE the loop-baked entry, mirroring
        // the base indexed materialiser. The loop above classifies a PLAIN
        // (non-type) `export *` as `EsmImport` and bakes the runtime `.js`
        // `source_id` without TS-first normalization; this pass routes the
        // wildcard edge through `resolve_route_edge_canonical` (the `.d.ts`
        // companion), so the overlay wildcard `canonical_id`s agree with the
        // base `IndexedReady` route surface. An unresolvable source leaves
        // the loop-baked known-miss in place.
        for source in external_type_analysis.wildcard_reexport_sources() {
            if let Some(resolved) = self.resolve_route_edge_canonical(analysis_canonical_id, source)
            {
                import_routes.insert(
                    source.clone(),
                    DependencyResolution {
                        specifier: source.clone(),
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
        self.provenance
            .shallow_state_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut shallow_state_inner =
            crate::resolver_core::ShallowFileState::from_analysis_with_resolver(
                whole_hash,
                Arc::clone(&external_type_analysis),
                Arc::clone(&decl_bodies),
                &resolver,
            );
        self.inject_component_default_into_shallow_state(
            analysis_canonical_id,
            &mut shallow_state_inner,
            &snapshot.macros,
            Some(eval_source.as_ref()),
            decl_bodies.framework_parse(),
        );
        let shallow_state = Arc::new(shallow_state_inner);

        let analysis_flags =
            verter_semantic::analysis::AnalysisFlags::from_bits_truncate(snapshot.script_flags);
        let declares_interface_app_config = analysis_flags
            .contains(verter_semantic::analysis::AnalysisFlags::DECLARES_INTERFACE_APP_CONFIG);
        let script_analysis = Some(Arc::new(
            verter_semantic::analysis::ScriptAnalysisSnapshot {
                imports: snapshot.imports.clone(),
                module_references: snapshot.module_references.as_ref().clone(),
                bindings: snapshot.bindings.clone(),
                macros: snapshot.macros.as_ref().clone(),
                macro_type_deps: snapshot.macro_type_deps.as_ref().clone(),
                flags: analysis_flags,
                ..Default::default()
            },
        ));
        let export_signatures = Some(Arc::clone(&snapshot.export_signatures));

        let import_routes = Arc::new(import_routes);

        let route_hash = shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

        let indexed = Arc::new(crate::project_type_store::IndexedReady {
            whole_hash,
            shallow_state: Arc::clone(&shallow_state),
            import_routes: Arc::clone(&import_routes),
            import_route_hash,
            route_hash,
            edge_generation,
            project_generation,
            parse_env_hash: flight_parse_env_hash,
            raw_source: Arc::clone(&raw_source),
            eval_source: Arc::clone(&eval_source),
            framework_parse,
            script_analysis,
            export_signatures,
            snapshot,
            external_type_analysis: Arc::clone(&external_type_analysis),
            declares_interface_app_config,
            macro_hot_mirror: crate::structural_carrier_producer::MacroHotMirror::default(),
        });

        // Publish via the multi-candidate surface — base candidate (if
        // any) under its own base key stays untouched.
        //
        // Key selection: `identity.overlay_artifact_key` builds an
        // `overlay_scoped` key when the bound view carries an explicit
        // overlay-set discriminator for the raw owner. The discriminator
        // occupies the `parse_env_hash` dimension and is derived from
        // the session view's overlay-set fingerprint, so the overlay
        // candidate is isolated from the base artifact (always
        // `parse_env_hash = BASE_PARSE_ENV_HASH`) and from other
        // sessions' overlay candidates — even when the overlay source
        // bytes are identical to the base file and the content hashes
        // therefore coincide. A base-host read via `get` /
        // `get_for_current_content` (the base key) never reaches an
        // `overlay_scoped` entry, and a session-view read via
        // `get_overlay_scoped` never reaches the base entry. An
        // overlay-FREE view (no overlays, no tombstones) yields the
        // base key, which is correct: with nothing masked anywhere,
        // route discovery read base authority throughout and the
        // candidate is base-equivalent. NOTE the owner having no
        // overlay is NOT sufficient for base-equivalence —
        // `resolve_relative_overlay_candidate` probes the view's
        // overlay maps for HELPER canonicals, so an unmasked owner on
        // an overlay-bearing view can bake overlay-only routes; the
        // publish key gate below declines the base-keyed publish for
        // exactly that case.
        //
        // Env-hash scope at this layer: the key carries the overlay
        // discriminator inside `parse_env_hash` (overlay branch) or
        // `BASE_PARSE_ENV_HASH` (base-passthrough branch). The
        // remaining env-hash dimensions (`resolve_env_hash`,
        // `type_env_hash`, `lib_env_hash`, `project_identity`) are
        // composed by the downstream caches that read this artifact
        // (`AnalysisReadyDb`, `RouteDb`, `MaterializeStructureDb`,
        // `ComponentMetaResultDb`) — that is where the wider env
        // split is the cache-correctness boundary. The materialiser's
        // contract here is candidate isolation between overlay and
        // base entries within the file-artifact substrate, which the
        // overlay discriminator inside `parse_env_hash` already
        // satisfies.
        // The publish key is built through `identity.overlay_artifact_key`
        // — `canonical` is the NORMALISED analysis canonical, the
        // `content_hash` + discriminator are RAW-owner-derived — so it
        // is byte-identical to the key the fast-path lookup above
        // reconstructs and the key every downstream reader rebuilds
        // through the same `OverlayArtifactIdentity` helper. A later
        // call short-circuits on the cached candidate.
        // PRE-PUBLISH FENCE — the same ReturnOnly contract as the base
        // materialise and the edge refresh. A base file-set mutation
        // (`content_generation`) or a route-resolution mutation
        // (`project_generation`) that landed during this build means the
        // overlay surface was resolved against superseded state: serve
        // the artifact to the caller (its request pre-dates the
        // mutation), publish NOTHING. The next overlay read finds no
        // candidate (or an edge-stale one) and re-materialises against
        // the live state. Publishing a known-superseded artifact
        // violates the standing ReturnOnly rule.
        //
        // The fence→insert pair is not atomic; a mutation landing in
        // the window leaves a stale-stamped publish. That torn insert
        // is rejected READ-SIDE, exactly as for the base materialise:
        // both overlay readers (the entry fast path and the in-flight
        // re-check in `materialize_overlay_indexed_ready_with_view`)
        // gate the `lookup_overlay_artifacts` hit on
        // `indexed_surface_is_current`, which rejects the pre-mutation
        // `edge_generation` / `project_generation` stamps; an overlay
        // CONTENT change re-keys the lookup itself (the key carries the
        // overlay content hash). The fence stays a best-effort churn
        // reducer; correctness is read-side authoritative.
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
        // Publish under the FLIGHT-CAPTURED content hash (the hash this
        // artifact was actually built from), never a live
        // `content_hash_for` re-read: a base upsert landing between the
        // fence above and this key build would re-key the old-content
        // artifact under the new hash's content-pinned key (hash-MOVED
        // poisoning). The fence guarantees live == flight when
        // publication proceeds, so readers rebuilding the key from the
        // live hash reach this same key; a hash that moved post-fence
        // was already declined ReturnOnly by the fence's
        // `content_generation` arm.
        //
        // The key build is GATED (`overlay_publish_key_for_content`): an
        // owner with no overlay discriminator on an overlay-bearing view
        // would fall back to the BASE key while its route discovery may
        // have baked overlay-only helper routes — a view-influenced
        // artifact must never enter the base key space. The gate
        // declines (serve ReturnOnly, publish nothing); production
        // callers gate on `overlay_content_hash_for(owner)` and never
        // reach the decline.
        let Some(key) = identity.overlay_publish_key_for_content(view, indexed.whole_hash) else {
            return Some(crate::project_type_store::IndexedFlightOutcome {
                indexed,
                published: false,
            });
        };
        let payload = Arc::new(crate::file_artifact_store::FileArtifacts::with_indexed(
            Arc::clone(&indexed),
        ));
        let _ = self
            .project_type_store
            .indexed()
            .insert_artifacts(key, payload);

        Some(crate::project_type_store::IndexedFlightOutcome {
            indexed,
            published: true,
        })
    }
}
