//! Overlay [`IndexedReady`](crate::project_type_store::IndexedReady)
//! materialiser. Owns the host-side cold path for building an overlay
//! candidate from `view.source(canonical)` content and publishing it
//! into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! as a multi-candidate sibling of the base host's IndexedReady.
//!
//! The resolver-tier seal scope reaches this body via
//! [`crate::resolver_core::ResolverContext::materialize_overlay_indexed_ready`];
//! the impl on [`crate::VerterHost`] delegates here.

use std::sync::Arc;

use crate::types::DependencyResolution;
use crate::VerterHost;

use super::{dep_edges_from_resolutions, is_raw_import_specifier_id, HostShallowImportResolver};

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
/// Returns `None` when `view` is absent, when `specifier` is not a
/// relative import, or when no extension candidate resolves through
/// `view.content_hash_for` / `view.source`.
fn resolve_relative_overlay_candidate(
    view: Option<&dyn crate::session_view::SessionView>,
    owner_canonical: &str,
    specifier: &str,
) -> Option<String> {
    let view = view?;
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
    /// Materialise an [`IndexedReady`](crate::project_type_store::IndexedReady)
    /// candidate for `canonical_id` from `overlay_source` and publish it
    /// under the overlay's content hash so two concurrent sessions
    /// with different overlays coexist as candidates in
    /// [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore).
    ///
    /// View-less wrapper: delegates to
    /// [`Self::materialize_overlay_indexed_ready_with_view`] with no
    /// active view, so the import-route resolution falls back to the
    /// workspace-and-disk path. Preserved for callers that have not
    /// yet been migrated to the view-aware variant.
    pub(crate) fn materialize_overlay_indexed_ready(
        &self,
        canonical_id: &str,
        overlay_source: &Arc<str>,
        overlay_whole_hash: crate::types::Hash16,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.materialize_overlay_indexed_ready_inner(
            canonical_id,
            overlay_source,
            overlay_whole_hash,
            None,
        )
    }

    /// View-aware overlay materialiser. Identical to
    /// [`Self::materialize_overlay_indexed_ready`] except that the
    /// import-route resolution prefers overlay candidates surfaced by
    /// the supplied `SessionView`.
    ///
    /// Owner overlays that import overlay-only helpers
    /// (`/src/Button.vue` importing `./theme`, `./schema`, `./tv` where
    /// none exists on disk) discover the helper's overlay canonical via
    /// `view.content_hash_for(candidate)` / `view.source(candidate)`,
    /// removing the prewarm-order dependence that would otherwise force
    /// helpers to be upserted before the owner.
    pub(crate) fn materialize_overlay_indexed_ready_with_view(
        &self,
        canonical_id: &str,
        overlay_source: &Arc<str>,
        overlay_whole_hash: crate::types::Hash16,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.materialize_overlay_indexed_ready_inner(
            canonical_id,
            overlay_source,
            overlay_whole_hash,
            Some(view),
        )
    }

    fn materialize_overlay_indexed_ready_inner(
        &self,
        canonical_id: &str,
        overlay_source: &Arc<str>,
        overlay_whole_hash: crate::types::Hash16,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Fast path: an overlay materialisation for the same content
        // hash already lives in the file-artifact store. Multi-
        // candidate storage keeps base and overlay candidates separate
        // by content_hash, so this lookup serves only the overlay.
        if let Some(indexed) = self
            .project_type_store
            .indexed()
            .get(canonical_id, overlay_whole_hash)
        {
            return Some(indexed);
        }

        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        // Cold materialisation from overlay source. The body mirrors
        // the base `ensure_indexed_ready` materialise closure but
        // never touches the scheduler — the overlay source is the
        // sole content authority for this candidate, and the
        // candidate is published as a multi-candidate sibling of the
        // base via `insert_artifacts`.
        let raw_source: Arc<str> = Arc::clone(overlay_source);
        let cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>> = None;
        let whole_hash = overlay_whole_hash;
        let snapshot = Arc::new(self.build_snapshot_from_source_state(
            canonical_id,
            &raw_source,
            cached_parse.as_deref(),
        ));

        let eval_source = Arc::<str>::from(Self::build_eval_script_source(
            raw_source.as_ref(),
            cached_parse.as_deref(),
        ));
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");

        // Seed import routes from the host's DerivedRawState if the
        // session-side caller pre-populated them. Overlays use the
        // same `set_import_dependencies` surface as the base, so
        // overlay-specific deps land here when explicitly set.
        let mut import_routes: rustc_hash::FxHashMap<String, DependencyResolution> =
            rustc_hash::FxHashMap::default();
        if let Some(cc) = self.derived_raw_cache().get(canonical_id) {
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
            let resolved: Option<String> = if kind
                == verter_workspace::ResolveRequestKind::TypeImport
            {
                primary
                    .or_else(|| self.fallback_relative_type_companion(canonical_id, specifier))
                    .or_else(|| {
                        self.ws()
                            .resolve_import(
                                canonical_id,
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
            canonical_id,
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
            canonical_id,
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
        // any) under its own content hash stays untouched.
        //
        // TODO(follow-up — Codex P1.2 / fix-agent P1.2): the overlay
        // candidate is published into the shared `FileArtifactStore`
        // under the overlay's content hash. Base-host paths that read
        // via content-agnostic lookups (`get_any` /
        // `content_hash_for_canonical`, used by `read_analysis_source`
        // and `HostView`) can therefore observe overlay-published
        // candidates from a concurrent session. Closure requires
        // either (a) splitting overlay artifacts into a view-scoped
        // storage namespace, or (b) tagging overlay-published keys so
        // base lookups skip them. Owned by the follow-up substrate
        // block that lands the cold-compute call-graph view-threading
        // refactor (substrate-reviewer's P0.2).
        //
        // TODO(follow-up — substrate-reviewer P1.2): the key uses
        // `FileArtifactKey::legacy` which zeroes `parse_env_hash` and
        // sets `parser_version = LEGACY_PARSER_VERSION`. The full R21
        // env-hash quintuple is available on the bound view
        // (`view.env_hashes()` + `view.project_identity()`) but is
        // not threaded into this call. Lift the env hashes into
        // `materialize_overlay_indexed_ready`'s call signature when
        // the broader env-hash-migration block lands.
        let key = crate::file_artifact_store::FileArtifactKey::legacy(
            Arc::from(canonical_id),
            overlay_whole_hash,
        );
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
