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

use verter_semantic::analysis::script_shallow_index::build_script_shallow_index_with_owners;

use crate::VerterHost;

use super::is_raw_import_specifier_id;

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
    /// Runtime language for this overlay identity. An existing scheduler
    /// canonical keeps its explicit language override; path classification is
    /// used only when the overlay introduces a canonical with no runtime row.
    fn file_language(&self, host: &VerterHost) -> verter_language::FileLanguage {
        host.effective_file_state(&self.analysis_canonical, None)
            .map(|state| state.file_language)
            .unwrap_or_else(|| host.language_classifier.classify(&self.analysis_canonical))
    }

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

    /// Reconstruct the exact key used by the current view without consulting
    /// another candidate from the artifact store.
    pub(crate) fn current_read_key(
        &self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        let source = view.source(&self.raw_overlay_owner)?;
        let content_hash = view.content_hash_for(&self.raw_overlay_owner)?;
        let file_language = self.file_language(host);
        let parse_env_hash = view
            .overlay_artifact_discriminator(&self.raw_overlay_owner)
            .unwrap_or(crate::file_artifact_store::BASE_PARSE_ENV_HASH);
        crate::file_artifact_store::FileArtifactKey::for_source_identity(
            Arc::from(self.analysis_canonical.as_str()),
            content_hash,
            source.as_ref(),
            file_language,
            None,
            parse_env_hash,
        )
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
    /// Build the artifact key for a CALLER-SUPPLIED content hash. READ
    /// paths use it via [`Self::overlay_artifact_key`]; the publish path
    /// uses the gated [`Self::overlay_publish_key_for_content`] instead.
    ///
    /// The base arm (no overlay-set discriminator for the raw owner)
    /// is the base-passthrough READ shape: a view with no overlay for
    /// the owner reads — and a fully overlay-FREE view publishes —
    /// the base artifact under its base key.
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
    fn overlay_publish_key_for_indexed(
        &self,
        view: &dyn crate::session_view::SessionView,
        indexed: &crate::project_type_store::IndexedReady,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        if view
            .overlay_artifact_discriminator(&self.raw_overlay_owner)
            .is_none()
            && (!view.overlay_canonicals().is_empty() || !view.tombstoned_canonicals().is_empty())
        {
            return None;
        }
        let parse_env_hash = view
            .overlay_artifact_discriminator(&self.raw_overlay_owner)
            .unwrap_or(crate::file_artifact_store::BASE_PARSE_ENV_HASH);
        Some(crate::file_artifact_store::FileArtifactKey::for_indexed(
            Arc::from(self.analysis_canonical.as_str()),
            indexed,
            parse_env_hash,
        ))
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
        let key = self.current_read_key(host, view)?;
        match view.overlay_artifact_discriminator(&self.raw_overlay_owner) {
            Some(discriminator) => host
                .project_type_store()
                .indexed()
                .get_overlay_artifacts_scoped(
                    &self.analysis_canonical,
                    key.content_hash,
                    discriminator,
                    &key.parse_key,
                    &key.file_language_id,
                ),
            None => host
                .project_type_store()
                .indexed()
                .get_base_artifacts_for_content(
                    &self.analysis_canonical,
                    key.content_hash,
                    &key.parse_key,
                    &key.file_language_id,
                ),
        }
    }
}

impl VerterHost {
    /// Return the registered carrier structure owned by the active view.
    /// Overlay source is registered through the same authority used by
    /// materialization; an unmasked file reuses the committed base envelope.
    pub(super) fn registered_structure_for_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::carrier_publication_store::RegisteredFileStructure> {
        if view.overlay_content_hash_for(canonical_id).is_none() {
            return self.registered_file_structure(canonical_id);
        }

        let source = view.source(canonical_id)?;
        let file_language = self.language_classifier.classify(canonical_id);
        self.registered_overlay_structure(canonical_id, source, &file_language, view)
    }

    pub(super) fn registered_overlay_structure(
        &self,
        canonical_id: &str,
        source: Arc<str>,
        file_language: &verter_language::FileLanguage,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::carrier_publication_store::RegisteredFileStructure> {
        use verter_language::registered_source_authority::{
            CanonicalFileId, FileIncarnation, SourceGeneration,
        };

        let fingerprint = view.fingerprint();
        if fingerprint == 0 {
            return None;
        }
        let content_hash = view.content_hash_for(canonical_id)?;
        let generation = u64::from_le_bytes(content_hash[..8].try_into().ok()?).max(1);
        let incarnation = fingerprint | (1_u64 << 63);
        let registered = self
            .carrier_publication
            .source_authority
            .register_source(
                CanonicalFileId::new(canonical_id),
                FileIncarnation::new(incarnation),
                SourceGeneration::new(generation),
                file_language.clone(),
                source,
            )
            .ok()?;
        // Registered-identity fact read: the grammar comes from the file's
        // frontend catalog row, keyed adapter × carrier language. A miss
        // (unregistered carrier, or a row without a grammar fact) fails
        // closed as `None` — never another framework's grammar.
        let grammar =
            verter_compiler::framework_common::registered_carrier_projection::registered_grammar_for(
                file_language.adapter_id()?,
                file_language.carrier_language_id()?,
            )?;
        let accepted = self
            .carrier_publication
            .grammar_authority
            .accept_registered_source(
                &self.carrier_publication.source_authority,
                &registered,
                grammar,
            )
            .ok()?;
        let request = crate::carrier_publication_store::PublicationRequestContext::new(
            crate::carrier_publication_store::AuditRequestId::new(self.next_request_id()),
            crate::carrier_publication_store::PublicationSurface::Overlay,
            verter_scheduler::cancellation::CancellationToken::default(),
            registered.snapshot_id().clone(),
        );
        let envelope = self
            .carrier_publication
            .publication_store
            .publish_or_get(&accepted, request)
            .into_envelope()?;
        Some(crate::carrier_publication_store::RegisteredFileStructure::new(envelope))
    }

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

    /// Test-only exact read of the artifact keyed by the supplied session
    /// view. The key is rebuilt through the same mixed raw/analysis identity
    /// authority as production overlay reads.
    #[cfg(test)]
    pub(crate) fn exact_overlay_artifacts_for_test(
        &self,
        raw_canonical: &str,
        expected_whole_hash: crate::types::Hash16,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        let identity = self.overlay_artifact_identity(raw_canonical);
        let key = identity.current_read_key(self, view)?;
        (key.content_hash == expected_whole_hash)
            .then(|| self.project_type_store.indexed().get_artifacts(&key))?
    }

    /// Test-only indexed projection of
    /// [`Self::exact_overlay_artifacts_for_test`].
    #[cfg(test)]
    pub(crate) fn exact_overlay_indexed_for_test(
        &self,
        raw_canonical: &str,
        expected_whole_hash: crate::types::Hash16,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.exact_overlay_artifacts_for_test(raw_canonical, expected_whole_hash, view)
            .map(|artifacts| Arc::clone(&artifacts.indexed))
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
        let overlay_file_language = identity.file_language(self);
        // The overlay source never carries a scheduler carrier parse; a carrier
        // overlay (`.vue` / `.svelte`) runs the carrier parser ONCE here through
        // the counted chokepoint (the carrier-neutral producer) and everything
        // downstream reuses its framework-neutral artifact.
        let framework_parse: Option<
            Arc<verter_compiler::framework_common::FrameworkParseArtifact>,
        > = if overlay_file_language.is_framework_carrier() {
            Some(Arc::clone(
                self.registered_overlay_structure(
                    analysis_canonical_id,
                    Arc::clone(&raw_source),
                    &overlay_file_language,
                    view,
                )?
                .artifact(),
            ))
        } else {
            None
        };
        let whole_hash = overlay_whole_hash;

        // `eval_is_extracted_script` records whether the eval source is
        // the position-preserving extracted carrier script — the
        // predicate that lets the snapshot build below walk the
        // flight's single eval-program parse instead of re-parsing the
        // same script bytes.
        let (eval_source, eval_is_extracted_script) =
            Self::build_eval_script_source_with_extraction(
                canonical_id,
                raw_source.as_ref(),
                framework_parse.as_deref(),
            )?;
        // Single source type + single eval-program parse — the arena
        // stays on this flight's stack. The source type derives from the
        // OVERLAY content (the pure derivation over `raw_source` +
        // `framework_parse`, the exact inputs the snapshot below is built
        // from) — NEVER from the scheduler-stored
        // `HostSourceData::source_type`, which covers BASE content:
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
            route_inventory:
                verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory,
            snapshot: Option<crate::types::FileAnalysisSnapshot>,
            svelte_component_runes_mode: bool,
            owner_table: Arc<verter_semantic::analysis::TopLevelOwnerTable>,
        }
        let job_canonical = analysis_canonical_id.to_string();
        let job_raw_source = Arc::clone(&raw_source);
        let job_eval_source = Arc::clone(&eval_source);
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
                        &parsed_sfc,
                        job_framework_parse
                            .as_deref()
                            .expect("Vue parse came from this framework artifact"),
                        &job_provenance,
                        job_eval_source.as_ref(),
                        VerterHost::vue_flight_script_program(eval_is_extracted_script, program),
                        Some(&owner_table),
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
                            job_eval_source.as_ref(),
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
                    // overlay: a re-parse over the same bytes under the
                    // same source type panics identically, so the
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
        // stack), so the `None` arm is an invariant break: fail CLOSED
        // (no artifact) — loud in debug builds — never a transient
        // re-parse.
        let Some(products) = outcome else {
            verter_debug_assert!(
                false,
                "overlay cold-index run missed its own held lease pin for {}",
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
        let snapshot = Arc::new(products.snapshot.unwrap_or_default());
        let route_inventory = Arc::new(products.route_inventory);

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
            Arc::clone(&products.owner_table),
            products.svelte_component_runes_mode,
            Arc::clone(&self.decl_lowering),
            Arc::new(products.header_index),
            Arc::clone(&self.provenance),
            Some(cold_lease.lease),
        ));
        // Materialisation performs ZERO import resolution. The artifact
        // it publishes is a content-addressed PARSE/INDEX product: the
        // shallow inventory names AUTHORED specifiers, and every
        // consumer that needs a target demands it from the workspace
        // resolution authority at the point of use. Resolving here is
        // what made a content-addressed artifact carry
        // dependency-set-derived state and forced the global
        // edge-generation stamp that guarded it.
        self.provenance
            .shallow_state_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut shallow_state_inner = crate::resolver_core::ShallowFileState::from_route_inventory(
            whole_hash,
            Arc::clone(&route_inventory),
            Arc::clone(&decl_bodies),
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

        let indexed = Arc::new(crate::project_type_store::IndexedReady {
            whole_hash,
            file_language: overlay_file_language,
            shallow_state: Arc::clone(&shallow_state),
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
        let Some(key) = identity.overlay_publish_key_for_indexed(view, &indexed) else {
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
