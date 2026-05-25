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
//! the [`legacy`](crate::file_artifact_store::FileArtifactKey::legacy)
//! key) and from other sessions, even when the overlay source bytes are
//! identical to the base file.
//!
//! The resolver-tier seal scope reaches this body via
//! [`crate::resolver_core::ResolverContext::materialize_overlay_indexed_ready`];
//! the impl on [`crate::VerterHost`] delegates here.

use std::sync::Arc;

use crate::types::DependencyResolution;
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
    /// `legacy`. Returns `None` when the view reports no current content
    /// hash for the raw owner (unloaded / evicted / tombstoned).
    fn overlay_artifact_key(
        &self,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        let content_hash = view.content_hash_for(&self.raw_overlay_owner)?;
        let key = match view.overlay_artifact_discriminator(&self.raw_overlay_owner) {
            Some(discriminator) => crate::file_artifact_store::FileArtifactKey::overlay_scoped(
                Arc::from(self.analysis_canonical.as_str()),
                content_hash,
                discriminator,
            ),
            None => crate::file_artifact_store::FileArtifactKey::legacy(
                Arc::from(self.analysis_canonical.as_str()),
                content_hash,
            ),
        };
        Some(key)
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
    /// publish site below. A base-passthrough view (no overlay for the
    /// canonical) yields the legacy key, which is correct: without an
    /// overlay there is no overlay-only route discovery and the
    /// candidate is base-equivalent.
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
    pub(crate) fn materialize_overlay_indexed_ready_with_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
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

        // Derive the overlay source + its content hash from the view —
        // ONE authority. `content_hash_for` is the view-authoritative
        // CURRENT content hash (overlay hash when masked, scheduler
        // hash otherwise); `source` returns the exact bytes that hash
        // covers. Resolving both here removes the caller-supplied-hash
        // failure mode entirely.
        //
        // The view source/hash lookups are keyed by the RAW
        // `canonical_id` — the `SessionView` overlay maps are keyed by
        // the requested canonical, so a normalised id would miss the
        // overlay. The overlay-set discriminator (also a raw-keyed view
        // lookup) and the `FileArtifactKey.canonical` (the normalised
        // analysis target) are both threaded through `identity` at the
        // fast-path lookup and the publish below.
        let overlay_source = view.source(canonical_id)?;
        let overlay_whole_hash = view.content_hash_for(canonical_id)?;

        // Fast path: an overlay materialisation for the same content
        // hash already lives in the file-artifact store under the
        // overlay-scoped key (or the legacy key when the bound view
        // carries no overlay for this canonical). Multi-candidate
        // storage keeps base and overlay candidates separate, so this
        // lookup serves only the overlay. The key is built through
        // `identity` — `canonical` is the NORMALISED analysis canonical
        // (the artifact-store identity), `content_hash` + discriminator
        // are RAW-owner-derived — so it reconstructs exactly the key
        // the publish below writes under.
        if let Some(facts) = identity.lookup_overlay_artifacts(self, view) {
            return Some(Arc::clone(&facts.indexed));
        }

        if analysis_canonical_id.is_empty() || is_raw_import_specifier_id(analysis_canonical_id) {
            return None;
        }

        // Cold materialisation from overlay source. The body mirrors
        // the base `ensure_indexed_ready` materialise closure but
        // never touches the scheduler — the overlay source is the
        // sole content authority for this candidate, and the
        // candidate is published as a multi-candidate sibling of the
        // base via `insert_artifacts`.
        let raw_source: Arc<str> = Arc::clone(&overlay_source);
        let cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>> = None;
        let whole_hash = overlay_whole_hash;
        let snapshot = Arc::new(self.build_snapshot_from_source_state(
            analysis_canonical_id,
            &raw_source,
            cached_parse.as_deref(),
        ));

        let eval_source = Arc::<str>::from(Self::build_eval_script_source(
            raw_source.as_ref(),
            cached_parse.as_deref(),
        ));
        let declaration_file = analysis_canonical_id.ends_with(".d.ts")
            || analysis_canonical_id.ends_with(".d.mts")
            || analysis_canonical_id.ends_with(".d.cts");

        // Seed import routes from the host's DerivedRawState if the
        // session-side caller pre-populated them. Overlays use the
        // same `set_import_dependencies` surface as the base, so
        // overlay-specific deps land here when explicitly set.
        let mut import_routes: rustc_hash::FxHashMap<String, DependencyResolution> =
            rustc_hash::FxHashMap::default();
        if let Some(cc) = self.derived_raw_cache().get(analysis_canonical_id) {
            for (specifier, resolution) in cc.import_routes.iter() {
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
        for (specifier, kind) in &required_import_sources {
            if import_routes.contains_key(specifier) {
                continue;
            }
            let kind = *kind;
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
                        .map(|resolution| {
                            if kind == verter_workspace::ResolveRequestKind::TypeImport {
                                self.normalize_live_type_dependency_target(
                                    analysis_canonical_id,
                                    specifier,
                                    resolution.source_id.as_str(),
                                )
                            } else {
                                resolution.source_id
                            }
                        })
                })
                .clone();
            // `resolve_relative_overlay_candidate` probes the view's
            // overlay maps (`view.source` / `view.content_hash_for`)
            // for an overlay-only relative helper, so it takes the RAW
            // `canonical_id` — the owner the overlay is keyed under.
            // Workspace resolution above uses the normalised
            // `analysis_canonical_id` (directory-equivalent for the
            // `.js`→`.d.ts` rewrite, and the base path's identity).
            let resolved: Option<String> = if kind
                == verter_workspace::ResolveRequestKind::TypeImport
            {
                primary
                    .or_else(|| {
                        self.fallback_relative_type_companion(analysis_canonical_id, specifier)
                    })
                    .or_else(|| {
                        self.ws()
                            .resolve_import(
                                analysis_canonical_id,
                                specifier,
                                verter_workspace::ResolutionContext {
                                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                    kind: verter_workspace::ResolveRequestKind::EsmImport,
                                },
                            )
                            .map(|resolution| resolution.source_id)
                    })
                    .or_else(|| resolve_relative_overlay_candidate(view, canonical_id, specifier))
            } else {
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

        let external_type_analysis = self.build_external_type_analysis(
            analysis_canonical_id,
            whole_hash,
            raw_source.as_ref(),
            cached_parse.as_deref(),
            &eval_source,
        );

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        let dep_edges = dep_edges_from_resolutions(&import_routes);
        let resolver = HostShallowImportResolver {
            dep_edges: &dep_edges,
        };
        let mut shallow_state_inner =
            crate::resolver_core::ShallowFileState::from_analysis_with_resolver(
                whole_hash,
                Arc::clone(&external_type_analysis),
                Some(eval_source.as_ref()),
                None,
                &resolver,
            );
        crate::resolver_core::vue_default_synth::inject_vue_default_into_shallow_state(
            analysis_canonical_id,
            &mut shallow_state_inner,
            &snapshot.macros,
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
            raw_source: Arc::clone(&raw_source),
            eval_source: Arc::clone(&eval_source),
            cached_parse,
            script_analysis,
            export_signatures,
            snapshot,
            external_type_analysis: Arc::clone(&external_type_analysis),
            declares_interface_app_config,
        });

        // Publish via the multi-candidate surface — base candidate (if
        // any) under its own legacy key stays untouched.
        //
        // Key selection: `identity.overlay_artifact_key` builds an
        // `overlay_scoped` key when the bound view carries an explicit
        // overlay-set discriminator for the raw owner. The discriminator
        // occupies the `parse_env_hash` dimension and is derived from
        // the session view's overlay-set fingerprint, so the overlay
        // candidate is isolated from the base artifact (always
        // `parse_env_hash = LEGACY_PARSE_ENV_HASH`) and from other
        // sessions' overlay candidates — even when the overlay source
        // bytes are identical to the base file and the content hashes
        // therefore coincide. A base-host read via `get` /
        // `get_for_current_content` (the legacy key) never reaches an
        // `overlay_scoped` entry, and a session-view read via
        // `get_overlay_scoped` never reaches the base entry. A
        // base-passthrough view (no overlay for this canonical) yields
        // the legacy key, which is correct: with no overlay there is no
        // overlay-only relative route discovery, so the candidate is
        // base-equivalent.
        //
        // Env-hash scope at this layer: the key carries the overlay
        // discriminator inside `parse_env_hash` (overlay branch) or
        // `LEGACY_PARSE_ENV_HASH` (base-passthrough branch). The
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
        let key = identity
            .overlay_artifact_key(view)
            .expect("the overlay source resolved above, so the view reports a current content hash for the raw owner");
        let payload = Arc::new(crate::file_artifact_store::FileArtifacts::with_indexed(
            Arc::clone(&indexed),
        ));
        let _ = self
            .project_type_store
            .indexed()
            .insert_artifacts(key, payload);

        Some(indexed)
    }
}
