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

use crate::types::*;
use crate::VerterHost;

use super::{
    collect_type_expr_symbol_refs, component_meta_debug, component_meta_debug_enabled,
    component_meta_trace_custom, dep_edges_from_resolutions, is_builtin_type_symbol,
    is_raw_import_specifier_id, is_runtime_script_target, HostShallowImportResolver,
    ImportedSymbolDependency,
};

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
    #[cfg(any(test, debug_assertions))]
    #[allow(dead_code)]
    pub(crate) fn prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let view = self.resolver_store_view();
        self.prepared_decl_bundle_with_store_view(&view, canonical_id)
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
        let flight = singleflight.run(key.clone(), token, || {
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
                });
            }
            // Per-request audit attribution: cold materialisation of
            // the prepared-decl bundle. Bumped only when the
            // singleflight leader's recheck miss confirmed the cold
            // path, so joiners that block on the leader do not double
            // count.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::PreparedDeclBundleCold);
            }
            let result = self
                .materialize_prepared_decl_bundle_from_route_owned_shallow(canonical_id)
                .or_else(|| self.materialize_prepared_decl_bundle(canonical_id));
            let stable = result.is_some();
            Ok(crate::resolver_core::StableExecutionValue {
                value: result.map(|arc| (*arc).clone()),
                stable,
            })
        });
        match flight {
            Ok(f) => f.value.value.clone().map(std::sync::Arc::new),
            Err(()) => None,
        }
    }

    /// View-aware prepared-decl bundle lookup.
    ///
    /// When the view carries an overlay source for the canonical
    /// (overlay-bearing session), the shared bundle cache (keyed by
    /// canonical alone) cannot store an overlay-specific bundle without
    /// colliding with the base. The session-tier resolver therefore
    /// bypasses the shared cache and materialises a per-call bundle
    /// rooted at `ctx.ensure_indexed_ready(canonical)` — which routes
    /// through the overlay-priority `ensure_indexed_ready_with_view`
    /// helper and returns the overlay's [`IndexedReady`] candidate.
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
        // drives `ensure_indexed_ready` on the raw owner (its overlay
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
            if view.overlay_content_hash_for(canonical_id).is_some() {
                return self.materialize_prepared_decl_bundle_via_ctx(ctx, &identity);
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
    ///   overlay (`ensure_indexed_ready` is driven on the raw owner);
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
        // Drive the overlay-aware `ensure_indexed_ready` on the RAW
        // owner — the overlay-detection gate inside
        // `ensure_indexed_ready_with_view` keys on the raw canonical.
        let facts = ctx.ensure_indexed_ready(identity.raw_overlay_owner())?;
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
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.exports.is_empty()
            && state.import_targets.is_empty()
        {
            return None;
        }
        let (dep_edges, _import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(route_canonical_id, state.as_ref());

        let script_setup_type_bindings = if bundle_canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(bundle_canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                bundle_canonical_id,
                std::sync::Arc::clone(state),
                dep_edges,
                script_setup_type_bindings,
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

    fn materialize_prepared_decl_bundle_from_route_owned_shallow(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        if !declaration_file {
            return None;
        }

        let state = self.route_owned_shallow_state(canonical_id)?;
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.exports.is_empty()
            && state.import_targets.is_empty()
        {
            return None;
        }

        let (dep_edges, import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(&state),
                dep_edges,
                rustc_hash::FxHashMap::default(),
            ),
        );

        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: state.whole_hash,
        }];
        if let Some(import_route_hash) = import_route_hash {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }

        // Strict admission. Bundles always carry `FileWholeHash`.
        self.resolver
            .runtime
            .prepared_decl_bundles
            .insert_arc_with_kind(
                canonical_id.to_string(),
                std::sync::Arc::clone(&bundle),
                facts,
                "prepared_decl_bundles",
            );

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

        Some(bundle)
    }

    /// Materialize a fresh `PreparedDeclBundle` for a canonical file, insert it
    /// into the stable cache with the appropriate fact versions, and return it.
    fn materialize_prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        // 1. Ensure source/shallow data exists.
        let facts = self.ensure_indexed_ready(canonical_id)?;
        let state = &facts.shallow_state;
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.exports.is_empty()
            && state.import_targets.is_empty()
        {
            return None;
        }
        let (dep_edges, import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());

        // 4. Build script-setup type bindings for Vue SFCs (once per bundle).
        // Non-Vue files get an empty map — zero cost.
        let script_setup_type_bindings = if canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // 5. Build the bundle atomically.
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(state),
                dep_edges,
                script_setup_type_bindings,
            ),
        );

        // 6. Compute fact versions.
        // Always include ImportRoute when present — all prepared bundles
        // embed resolved cross-file canonical IDs (dep_edges, import_bindings,
        // name_resolution, external_deps) and must be invalidated when the
        // import graph changes, regardless of whether the file is tracked.
        let whole_hash = facts.whole_hash;
        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: whole_hash,
        }];
        if let Some(import_route_hash) = import_route_hash {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }

        // 7. Insert into the stable cache. Strict admission — bundles always carry `FileWholeHash`.
        self.resolver
            .runtime
            .prepared_decl_bundles
            .insert_arc_with_kind(
                canonical_id.to_string(),
                std::sync::Arc::clone(&bundle),
                facts,
                "prepared_decl_bundles",
            );

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

        Some(bundle)
    }

    /// Test-only bare wrapper. Production callers go through
    /// `ctx.prepared_type_decl` (which routes through `_with_store_view`).
    #[cfg(any(test, debug_assertions))]
    #[allow(dead_code)]
    pub(crate) fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let view = self.resolver_store_view();
        self.prepared_type_decl_with_store_view(&view, canonical_id, symbol_name)
    }

    /// View-bound variant of [`Self::prepared_type_decl`].
    ///
    /// Threads the request-bound [`crate::resolver_store::HostStoreView`]
    /// down through [`Self::prepared_decl_bundle_with_store_view`] so the
    /// warm-hit path validates against the request's snapshot rather than
    /// triggering a fresh per-call workspace sweep.
    pub(crate) fn prepared_type_decl_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let bundle = self.prepared_decl_bundle_with_store_view(view, canonical_id)?;
        let result = bundle.prepared_type_decls.get(symbol_name);
        component_meta_trace_custom!(
            "prepared_type_decl_result",
            format!(
                "owner={} symbol={} source=bundle_hit hit={}",
                canonical_id,
                symbol_name,
                result.is_some(),
            ),
        );
        result
    }

    /// View-aware prepared type declaration lookup. Routes the
    /// underlying bundle materialisation through
    /// [`Self::prepared_decl_bundle_with_context`] so overlay-bearing
    /// sessions observe the overlay's [`IndexedReady`] when extracting
    /// a `PreparedTypeDecl`.
    pub(crate) fn prepared_type_decl_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let bundle = self.prepared_decl_bundle_with_context(ctx, canonical_id)?;
        bundle.prepared_type_decls.get(symbol_name)
    }

    pub(crate) fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let view = self.resolver_store_view();
        self.prepared_value_decl_with_store_view(&view, canonical_id, symbol_name)
    }

    /// View-bound variant of [`Self::prepared_value_decl`].
    ///
    /// Threads the request-bound [`crate::resolver_store::HostStoreView`]
    /// down through [`Self::prepared_decl_bundle_with_store_view`].
    pub(crate) fn prepared_value_decl_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let bundle = self.prepared_decl_bundle_with_store_view(view, canonical_id)?;
        bundle.prepared_value_decls.get(symbol_name)
    }

    /// View-aware prepared value declaration lookup. See
    /// [`Self::prepared_type_decl_with_context`] for the routing
    /// rationale.
    pub(crate) fn prepared_value_decl_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let bundle = self.prepared_decl_bundle_with_context(ctx, canonical_id)?;
        bundle.prepared_value_decls.get(symbol_name)
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

        if let Some(state) = self.route_owned_shallow_state(canonical_id) {
            let budget = crate::resolver_core::shallow_file_state::ResolutionBudgets::default()
                .local_closure_steps;
            if let Some((symbol_name, _is_alias_export)) = state
                .export_target(exported_name)
                .and_then(|target| match target {
                    ExportTarget::Local { symbol_name } => {
                        Some((symbol_name.as_str(), symbol_name != exported_name))
                    }
                    ExportTarget::Reexport { .. } => None,
                })
            {
                let closure = state.route_closure(symbol_name, route, budget);
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
                if state.symbol(symbol_name).is_some_and(|symbol| {
                    symbol.kind == verter_semantic::analysis::type_eval::TypeDeclKind::Class
                }) {
                    if let Some(analysis) = self.external_type_analysis(canonical_id) {
                        for required_name in analysis.required_import_names(exported_name) {
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
                .external_type_analysis(canonical_id)
                .map(|analysis| {
                    analysis
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

    /// View-aware variant of [`Self::required_import_routes_for_exported_route`].
    ///
    /// Threads the session `view` through the route-owned shallow read and
    /// the external-type analysis fall-through so an overlay that changes
    /// a barrel/re-export surface or class-body imports is reflected in
    /// the required-import map. Base callers (`view = None`) get identical
    /// behaviour to the historical body.
    pub(crate) fn required_import_routes_for_exported_route_with_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> rustc_hash::FxHashMap<String, crate::resolver_core::RouteDemand> {
        use crate::resolver_core::shallow_file_state::ExportTarget;
        use crate::resolver_core::RouteDemand;

        if let Some(state) = self.route_owned_shallow_state_with_view(canonical_id, view) {
            let budget = crate::resolver_core::shallow_file_state::ResolutionBudgets::default()
                .local_closure_steps;
            if let Some((symbol_name, _is_alias_export)) = state
                .export_target(exported_name)
                .and_then(|target| match target {
                    ExportTarget::Local { symbol_name } => {
                        Some((symbol_name.as_str(), symbol_name != exported_name))
                    }
                    ExportTarget::Reexport { .. } => None,
                })
            {
                let closure = state.route_closure(symbol_name, route, budget);
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
                if state.symbol(symbol_name).is_some_and(|symbol| {
                    symbol.kind == verter_semantic::analysis::type_eval::TypeDeclKind::Class
                }) {
                    if let Some(analysis) =
                        self.external_type_analysis_with_view(canonical_id, view)
                    {
                        for required_name in analysis.required_import_names(exported_name) {
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
                }
                return result;
            }

            if !matches!(route, RouteDemand::Whole) {
                return self.required_import_routes_for_exported_route_with_view(
                    canonical_id,
                    exported_name,
                    &RouteDemand::Whole,
                    view,
                );
            }
        }

        if matches!(route, RouteDemand::Whole) {
            return self
                .external_type_analysis_with_view(canonical_id, view)
                .map(|analysis| {
                    analysis
                        .required_import_names(exported_name)
                        .into_iter()
                        .map(|name| (name, RouteDemand::Whole))
                        .collect()
                })
                .unwrap_or_default();
        }

        self.required_import_routes_for_exported_route_with_view(
            canonical_id,
            exported_name,
            &RouteDemand::Whole,
            view,
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

    fn imported_symbol_dependencies(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical_id: &str,
        exported_name: &str,
        decl_body: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        let analysis = match self.external_type_analysis(canonical_id) {
            Some(analysis) => analysis,
            None => return Vec::new(),
        };
        let mut dependencies = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut referenced_names = std::collections::BTreeSet::new();
        collect_type_expr_symbol_refs(decl_body, &mut referenced_names);
        for referenced_name in referenced_names {
            let root_name = referenced_name
                .split('.')
                .next()
                .unwrap_or(referenced_name.as_str());
            if root_name == exported_name || is_builtin_type_symbol(root_name) {
                continue;
            }

            if let Some((import_source, imported_name)) =
                analysis.local_import_symbol_target(root_name)
            {
                let (resolved_canonical, resolved_name) = if root_name == referenced_name {
                    // Direct owner import — resolve via the project-global
                    // owner surface so every stage reads the same cached
                    // answer for this `(owner, local_name)` pair. Route
                    // through `ctx` so request-bound callers exercise the
                    // overlay-aware view rather than rebuild one per call.
                    match ctx.resolve_owner_direct_import(canonical_id, root_name) {
                        Some(resolved) => resolved,
                        None => continue,
                    }
                } else {
                    // Dotted reference like `Foo.Bar` — preserve the legacy
                    // suffixed name lookup path; the direct-import surface
                    // only caches top-level `local_name` entries.
                    let suffix = referenced_name.strip_prefix(root_name).unwrap_or("");
                    let imported_member = format!("{}{}", imported_name, suffix);
                    let Some(dep_canonical) =
                        self.resolve_type_dependency_canonical(canonical_id, import_source)
                    else {
                        continue;
                    };
                    ctx.resolve_imported_type_root(dep_canonical.as_str(), imported_member.as_str())
                };
                if seen.insert((
                    referenced_name.clone(),
                    resolved_canonical.clone(),
                    resolved_name.clone(),
                )) {
                    dependencies.push(ImportedSymbolDependency {
                        local_name: referenced_name,
                        canonical_id: resolved_canonical,
                        exported_name: resolved_name,
                    });
                }
                continue;
            }

            if analysis.local_symbol_span(root_name).is_some()
                && seen.insert((
                    root_name.to_string(),
                    canonical_id.to_string(),
                    root_name.to_string(),
                ))
            {
                dependencies.push(ImportedSymbolDependency {
                    local_name: root_name.to_string(),
                    canonical_id: canonical_id.to_string(),
                    exported_name: root_name.to_string(),
                });
            }
        }
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    pub(crate) fn imported_symbol_dependencies_for_expr(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        self.cache_only_lookup_symbol_dependencies_for_expr(ctx, canonical_id, expr)
    }

    fn cache_only_lookup_symbol_dependencies_for_expr(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        let mut dependencies = self.imported_symbol_dependencies(ctx, canonical_id, "", expr);
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    pub(crate) fn external_type_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>>
    {
        component_meta_trace_custom!(
            "external_type_analysis",
            format!("owner={} store_view={}", canonical_id, false),
        );
        let inputs = self.external_type_resolution_inputs(canonical_id)?;
        let analysis = Arc::clone(&inputs.analysis);
        let stats = analysis.stats();
        if inputs.analysis_cache_hit {
            component_meta_trace_custom!(
                "external_type_analysis_cache_hit",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        } else {
            component_meta_trace_custom!(
                "external_type_analysis_built",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        }
        Some(analysis)
    }

    /// View-aware variant of [`Self::external_type_analysis`].
    ///
    /// When `view: Some(...)` carries parse artifacts for `canonical_id`
    /// (overlay candidate published into FileArtifactStore under the
    /// overlay content hash), the analysis is read from the view's
    /// artifacts so the session-bearing cold-compute path observes
    /// overlay-rooted external-type analysis. Base callers (`view = None`)
    /// fall through to the historical content-agnostic `get_any` fast path
    /// followed by the route-owned materialiser, identical to the base
    /// `external_type_analysis` behaviour.
    pub(crate) fn external_type_analysis_with_view(
        &self,
        canonical_id: &str,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>>
    {
        component_meta_trace_custom!(
            "external_type_analysis_with_view",
            format!("owner={} store_view={}", canonical_id, view.is_some()),
        );
        let inputs = self.external_type_resolution_inputs_with_view(canonical_id, view)?;
        let analysis = Arc::clone(&inputs.analysis);
        let stats = analysis.stats();
        if inputs.analysis_cache_hit {
            component_meta_trace_custom!(
                "external_type_analysis_with_view_cache_hit",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        } else {
            component_meta_trace_custom!(
                "external_type_analysis_with_view_built",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        }
        Some(analysis)
    }

    /// Get or build the canonical shallow type file state for an imported
    /// dependency.  The state is read from `FileArtifactStore` pinned to the
    /// dependency's current content, falling back to the content-pinned
    /// route-owned shallow surface.
    ///
    /// Consumed by the frontier engine (production cache-warming pass in
    /// `resolve_external_type_from_loaded_files`) and integration tests.
    ///
    /// The lookup is **current-content-pinned**: it never reads
    /// `FileArtifactStore` through the content-agnostic `get_any`. With the
    /// own-canonical drain retired, a same-canonical content edit can leave a
    /// stale pre-edit `IndexedReady` lingering in `FileArtifactStore`; a
    /// `get_any` read would surface that stale artifact and feed a stale
    /// observed-content hash to every provenance-pure signature builder. The
    /// read is therefore pinned to the canonical's authoritative current
    /// content hash; a stale older-content artifact yields a miss.
    ///
    /// It does NOT materialise (`ensure_indexed_ready` re-enters the full
    /// materialisation path — load files, build snapshots, resolve imports —
    /// which itself calls `shallow_file_state`; that is the recursion this
    /// function must not open).
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
    /// `route_owned_shallow_state_with_context`, `artifact_current_indexed`
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
    /// 2. On miss for a live scheduler-tracked canonical, fall through to the
    ///    content-pinned route-owned shallow surface
    ///    ([`Self::route_owned_shallow_state_with_view`] — its own indexed
    ///    fast path is pinned, and the route-owned entry is freshness-gated).
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
        // Step 1 — current-content-pinned `IndexedReady` fast path. This is
        // a cache read only; it never materialises (no `ensure_indexed_ready`
        // — that would re-enter the recursion this function guards against).
        if let Some(indexed) = ctx.indexed_for_current_content(canonical_id) {
            if indexed.shallow_state.has_resolvable_surface() {
                return Some(indexed.shallow_state.clone());
            }
        }

        // Step 2 — content-pinned route-owned shallow fallback. The
        // route-owned path's own indexed fast path is content-pinned and the
        // route-owned entry carries the tiered freshness gate.
        if let Some(state) = self.route_owned_shallow_state_with_context(ctx, canonical_id) {
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

    /// Ensure the canonical post-parse artifact is materialized for a file.
    ///
    /// This is the single materialization bridge for the semantic DB layer.
    ///
    /// On cache hit, returns the cached `IndexedReady` without any I/O.
    /// On miss, reads the file, parses, builds analysis/snapshot/eval, constructs
    /// `ShallowFileState`, and publishes to `FileArtifactStore`.
    pub(crate) fn ensure_indexed_ready(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Fast path: check FileArtifactStore through the project-global cache.
        // R3 cutover: query the scheduler's current `whole_hash` for the
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
                component_meta_trace_custom!(
                    "ensure_indexed_ready_fast_hit",
                    format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
                );
                return Some(indexed);
            }
        } else if let Some(indexed) = self.artifact_current_indexed(canonical_id) {
            // Scheduler doesn't have a current snapshot. The
            // artifact-current authority answers ONLY for a genuinely
            // artifact-only canonical (no scheduler `DerivedRawState` —
            // a foreign-source-loaded file or a test seed); for such a
            // canonical staleness is not driven by content upserts, so
            // the single retained artifact is the current one. A
            // canonical the scheduler DOES track (a `DerivedRawState`
            // entry exists) gets `None` from `artifact_current_indexed`,
            // so this branch declines and the materialiser below
            // rebuilds rather than serving a possibly-stale artifact.
            component_meta_trace_custom!(
                "ensure_indexed_ready_fast_hit",
                format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
            );
            return Some(indexed);
        }

        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        let materialize = || -> Option<Arc<crate::project_type_store::IndexedReady>> {
            // Materialize: read source, build analysis, construct facts.
            //
            // Native: scheduler is the sole source authority. On a scheduler
            // miss, call `ensure_loaded` once to submit the canonical through
            // the scheduler — the canonical way to materialize a file. If
            // the scheduler still misses after `ensure_loaded`, return None
            // (file doesn't exist in the workspace).
            let (raw_source, cached_parse, whole_hash, snapshot) = {
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
                let snapshot =
                    if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical_id) {
                        self.provenance
                            .indexed_ready_scheduler_snapshot_reuse
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        snapshot
                    } else {
                        self.build_snapshot_from_source_state(
                            canonical_id,
                            &state.source,
                            state.cached_parse.as_deref(),
                        )
                    };
                (
                    state.source,
                    state.cached_parse,
                    state.whole_hash,
                    Arc::new(snapshot),
                )
            };

            let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                raw_source.as_ref(),
                cached_parse.as_deref(),
            ));
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
            // These are authoritative when the host caller has explicitly
            // provided resolution targets.
            let mut import_routes = rustc_hash::FxHashMap::default();
            {
                if let Some(cc) = self.derived_raw_cache().get(canonical_id) {
                    for (specifier, resolution) in cc.import_routes.iter() {
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
            required_import_sources.extend(snapshot.export_signatures.iter().filter_map(
                |export| {
                    let source = export.reexport_source.clone()?;
                    let kind = if declaration_file || export.is_type {
                        verter_workspace::ResolveRequestKind::TypeImport
                    } else {
                        verter_workspace::ResolveRequestKind::EsmImport
                    };
                    Some((source, kind))
                },
            ));
            required_import_sources.sort_by(
                |(left_source, left_kind), (right_source, right_kind)| {
                    left_source.cmp(right_source).then_with(|| {
                        let kind_rank = |kind: verter_workspace::ResolveRequestKind| match kind {
                            verter_workspace::ResolveRequestKind::TypeImport => 0u8,
                            verter_workspace::ResolveRequestKind::EsmImport => 1u8,
                            verter_workspace::ResolveRequestKind::RequireCall => 2u8,
                            verter_workspace::ResolveRequestKind::SfcSrcAttr => 3u8,
                        };
                        kind_rank(*left_kind).cmp(&kind_rank(*right_kind))
                    })
                },
            );
            required_import_sources.dedup();

            let mut resolve_memo: rustc_hash::FxHashMap<
                (String, verter_workspace::ResolveRequestKind),
                Option<String>,
            > = rustc_hash::FxHashMap::default();
            let mut resolve_missing =
                |specifier: &str,
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
                    let resolved: Option<String> = if kind
                        == verter_workspace::ResolveRequestKind::TypeImport
                    {
                        primary
                            .or_else(|| {
                                self.fallback_relative_type_companion(canonical_id, specifier)
                            })
                            .or_else(|| {
                                if !prefer_live_fallback {
                                    return None;
                                }
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

            for (source, kind) in &required_import_sources {
                resolve_missing(source, *kind, true);
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
            // Synthesise the implicit Vue SFC `default` value symbol
            // from type-based macros — see `vue_default_synth` for
            // the policy and rationale.
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

            // Prefer the scheduler's file state for script_analysis (it may have
            // richer compilation context), but fall back to the snapshot's data
            // for workspace-only files that are not in the scheduler.
            let script_analysis = self
                .effective_file_state(canonical_id, None)
                .filter(|state| state.whole_hash == whole_hash)
                .map(|state| Arc::new(state.script_analysis))
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

            let import_routes = Arc::new(import_routes);

            // Step 8 / F5: cache the route-surface hash on IndexedReady
            // symmetric to import_route_hash. Populated only when the
            // shallow state has a resolvable surface (matching
            // host_resolve.rs:575's existing pattern). Invalidation
            // lifecycle is identical to IndexedReady's content-hash
            // lifecycle — when canonical's whole_hash changes, a fresh
            // IndexedReady is built and route_hash is recomputed.
            // `current_derived_fact_hash` (meta_resolve.rs) reads this
            // cached hash instead of rehashing per call.
            let route_hash = shallow_state
                .has_resolvable_surface()
                .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

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
            self.project_type_store
                .indexed()
                .insert(Arc::from(canonical_id), Arc::clone(&indexed));

            Some(indexed)
        };

        // Collapse concurrent cold loads for the same canonical file through
        // the dedicated singleflight group on the resolver runtime.
        let singleflight = &self.resolver.runtime.indexed_singleflight;
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
        };
        match singleflight.run(canonical_id.to_owned(), token, || {
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
            if let Some(current_hash) = current_whole_hash {
                if let Some(indexed) = self
                    .project_type_store
                    .indexed()
                    .get(canonical_id, current_hash)
                {
                    return Ok(indexed);
                }
            } else if let Some(indexed) = self.artifact_current_indexed(canonical_id) {
                // Scheduler has no current snapshot — the artifact-current
                // authority answers only for a genuinely artifact-only
                // canonical (no `DerivedRawState`), mirroring the outer
                // fast path.
                return Ok(indexed);
            }
            materialize().ok_or(())
        }) {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Base wrapper that fixes `view = None`. Test-only — production paths
    /// reach the view-aware variant directly.
    #[cfg(test)]
    pub(crate) fn resolve_external_type_from_indexed_ready(
        &self,
        dep_canonical: &str,
        type_name: &str,
        imported_companions: &rustc_hash::FxHashMap<
            String,
            verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements,
        >,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        self.resolve_external_type_from_indexed_ready_with_view(
            dep_canonical,
            type_name,
            imported_companions,
            None,
        )
    }

    /// View-aware variant of resolve_external_type_from_indexed_ready.
    ///
    /// Reads the dependency's parse artifacts through the session view when
    /// `view` carries an overlay candidate; this is the path that lets a
    /// session-bearing component-meta cold compute see overlay-rooted
    /// resolved elements.
    pub(crate) fn resolve_external_type_from_indexed_ready_with_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        imported_companions: &rustc_hash::FxHashMap<
            String,
            verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements,
        >,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        component_meta_trace_custom!(
            "resolve_external_type_from_indexed_ready",
            format!(
                "owner={} type={} store_view={}",
                dep_canonical, type_name, false
            ),
        );
        let inputs = self.external_type_resolution_inputs_with_view(dep_canonical, view)?;
        let normalized_canonical_id = self.normalized_analysis_canonical(dep_canonical);
        let canonical_id_for_source_type = normalized_canonical_id.as_ref();
        let source_type = self.imported_eval_source_type_for(
            canonical_id_for_source_type,
            inputs.raw_source.as_ref(),
            inputs.cached_parse.as_deref(),
        );
        let Some(type_context) = self.cached_type_resolution_context_entry(
            canonical_id_for_source_type,
            inputs.whole_hash,
            &inputs.eval_source,
            source_type,
        ) else {
            component_meta_trace_custom!(
                "resolve_external_type_from_indexed_ready_result",
                format!(
                    "owner={} type={} hit=false local_symbol_target={} parse_failed_or_missing_type_context=true",
                    dep_canonical,
                    type_name,
                    inputs.analysis.has_local_symbol_target(type_name),
                ),
            );
            return None;
        };
        let program = type_context.borrow_owner().borrow_dependent();
        let base_ctx = type_context.borrow_dependent();
        let resolved = verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical(
            type_name,
            program,
            type_context.borrow_owner().source_bytes(),
            base_ctx,
            inputs.analysis.as_ref(),
            imported_companions,
            dep_canonical,
        );
        component_meta_trace_custom!(
            "resolve_external_type_from_indexed_ready_result",
            format!(
                "owner={} type={} hit={} local_symbol_target={} parse_failed=false",
                dep_canonical,
                type_name,
                resolved.is_some(),
                inputs.analysis.has_local_symbol_target(type_name),
            ),
        );
        resolved
    }

    pub(crate) fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        component_meta_trace_custom!(
            "resolve_direct_type_reexport_target",
            format!("owner={} requested={}", dep_canonical, requested_name),
        );
        let shallow = self.shallow_file_state(dep_canonical)?;
        let crate::resolver_core::ExportTarget::Reexport {
            source_specifier,
            original_name,
            canonical_id,
            ..
        } = shallow.export_target(requested_name)?
        else {
            return None;
        };
        let next_canonical = if canonical_id.is_empty() {
            self.resolve_route_type_edge(dep_canonical, source_specifier)?
        } else {
            canonical_id.clone()
        };
        component_meta_trace_custom!(
            "resolve_direct_type_reexport_target_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical, requested_name, source_specifier, next_canonical, original_name
            ),
        );
        Some((next_canonical, original_name.clone()))
    }

    pub(crate) fn current_or_read_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        // Live-host probe. Resolvers that need to load a canonical
        // mid-resolution must call `ensure_loaded` explicitly; only the
        // top-level / test-scaffold path auto-loads on miss.
        //
        // An evicted canonical must reload to authoritative state
        // before its whole-hash is reported: `get_whole_hash` has a
        // permissive `FileArtifactStore::get_any` fallback that
        // surfaces the *stale* artifact's own hash for an evicted
        // owner. Honouring that hash would let a query proceed on the
        // pre-eviction identity instead of forcing the reload the
        // evict marker demands. Route an evicted canonical through
        // `ensure_loaded` first — it clears the evict marker and
        // re-integrates authoritative scheduler state, after which
        // `get_whole_hash` returns the current content hash.
        let evicted = self.is_canonical_evicted(canonical_id);
        if !evicted {
            if let Some(hash) = self.get_whole_hash(canonical_id) {
                return Some(hash);
            }
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
        // R3/R26/R28 Gap 2: known-miss resolutions must invalidate
        // once the workspace's `content_generation` advances past
        // the value recorded at admission — a NEW canonical may now
        // satisfy a previously-unresolvable specifier. Positive
        // resolutions stay valid until the owner's source content
        // changes (evicts the DerivedRawState entry outright via
        // R4 parse-domain invalidation).
        if Self::import_route_is_known_miss(&resolution) {
            let recorded_at = derived
                .import_routes_known_miss_recorded_at_generation
                .get(import_source)
                .copied()
                .unwrap_or(0);
            let current = self.ws().content_generation();
            if recorded_at == 0 || current > recorded_at {
                // Per-request audit attribution: the known-miss entry
                // is stale relative to the current `content_generation`
                // — caller will recompute against the live workspace.
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(verter_audit::AuditEvent::KnownMissRouteRecomputed);
                }
                return None;
            }
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

        // Post-cut: live-host probe. Prefer the caller-supplied shallow state,
        // then fall back to the route-owned shallow cache. The ambient
        // request view no longer exists.
        let route_hash = known_shallow
            .filter(|state| state.has_resolvable_surface())
            .map(crate::resolver_store::hash_route_surface)
            .or_else(|| {
                self.route_owned_shallow_state(canonical_id)
                    .filter(|state| state.has_resolvable_surface())
                    .map(|state| crate::resolver_store::hash_route_surface(&state))
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
    ) -> Option<((String, String), Vec<crate::resolver_core::FactVersionRef>)> {
        let shallow = self.route_owned_shallow_state(dep_canonical)?;
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
            crate::resolver_core::ExportTarget::Local { symbol_name } => {
                let import_target = shallow.import_target(symbol_name.as_str())?;
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
        let (leaf_symbol, target_hash) = {
            let target_state = self.route_owned_shallow_state(normalized_target.as_str())?;
            match target_state.export_target(target_symbol.as_str())? {
                crate::resolver_core::ExportTarget::Local { symbol_name }
                    if target_state.import_target(symbol_name.as_str()).is_none() =>
                {
                    (symbol_name.clone(), target_state.whole_hash)
                }
                _ => return None,
            }
        };

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

        Some(((normalized_target, leaf_symbol), facts))
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

    pub(crate) fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        component_meta_trace_custom!(
            "resolve_local_export_symbol_target",
            format!("owner={} requested={}", canonical_source, exported_name),
        );
        let analysis = self.external_type_analysis(canonical_source)?;
        let target = analysis
            .local_export_symbol_target(exported_name)
            .map(str::to_string);
        if let Some(target) = target.as_deref() {
            component_meta_trace_custom!(
                "resolve_local_export_symbol_target_result",
                format!(
                    "owner={} requested={} target={}",
                    canonical_source, exported_name, target
                ),
            );
        }
        target
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
        let view = self.resolver_store_view();
        self.owner_import_surface_with_store_view(&view, owner_canonical)
    }

    /// View-bound variant of [`Self::owner_import_surface`].
    ///
    /// Validates the cached surface against the supplied request-bound
    /// view instead of building a fresh one — eliminating the per-call
    /// full-workspace snapshot the pre-6.c rail performed at this site.
    /// Same correctness contract: R3/R26/R28 fact-validation rejects a
    /// stale entry on the next read; the producer's
    /// `validated_at_generation` ProjectGeneration fencing
    /// (Block 6.B-fix `987a3ce6d`) is preserved.
    pub(crate) fn owner_import_surface_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        owner_canonical: &str,
    ) -> Option<Arc<crate::owner_import_surface::OwnerImportSurface>> {
        let shallow = self.shallow_file_state(owner_canonical)?;
        let whole_hash = shallow.whole_hash;
        let surfaces = self.project_type_store.owner_import_surfaces();
        // R3/R26/R28: fact-validate the cached surface against the
        // request-bound store view. A barrel retarget / chain-internal
        // edit invalidates the entry on read via its recorded
        // `fact_dep_signature`. Stale-key cleanup keeps the cache
        // bounded — when the chain facts no longer validate, we
        // drop the entry outright so the next build replaces it.
        if let Some(cached) = surfaces.get_with_view(self, owner_canonical, whole_hash, view) {
            return Some(cached);
        }
        if surfaces.get(owner_canonical, whole_hash).is_some() {
            surfaces.remove(owner_canonical);
        }

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

        // Block 1.H: wrap the cold body with `install_fact_tracer` so
        // the surface's `fact_dep_signature` reflects every fact the
        // chain walks observed via the resolver's TLS-installed
        // tracer fan-out. The producer ALSO accumulates `chain_facts`
        // explicitly for direct-API fan-in (legacy bookkeeping that
        // the post-Block-1.H build retains). On
        // `FactReadSetFinalise::Overflow` we refuse to admit the
        // entry — the next request cold-recomputes.
        let cold_body = || {
            // (local_name, final_canonical, final_exported_name, target_whole_hash)
            type SurfaceBuildEntry = (Arc<str>, Arc<str>, Arc<str>, Option<Hash16>);
            let mut entries: Vec<SurfaceBuildEntry> =
                Vec::with_capacity(shallow.import_targets.len());
            // R3/R26/R28 Gap 1: accumulate every chain fact observed by
            // each direct import's route walk. The producer threads these
            // into the surface's `fact_dep_signature` so dependent caches
            // detect intermediate barrel changes via fact-validation
            // alone (no eager invalidation required).
            let mut chain_facts: Vec<crate::resolver_core::FactVersionRef> = Vec::new();
            let mut seen_facts: rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef> =
                rustc_hash::FxHashSet::default();
            for (local_name, target) in shallow.import_targets.iter() {
                let resolved_canonical_id = if target.canonical_id.is_empty() {
                    match self.resolve_type_dependency_canonical(
                        owner_canonical,
                        &target.source_specifier,
                    ) {
                        Some(canonical) => canonical,
                        None => continue,
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
                let ((final_canonical, final_name), route_facts) = self
                    .resolve_imported_type_root_with_facts_with_store_view(
                        view,
                        resolved_canonical_id.as_str(),
                        target.imported_name.as_str(),
                    );
                for fact in route_facts.iter() {
                    if seen_facts.insert(fact.clone()) {
                        chain_facts.push(fact.clone());
                    }
                }

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
            (entries, chain_facts)
        };
        let (cold_output, finalise) =
            crate::fact_signature_helpers::install_fact_tracer(self, cold_body);
        self.provenance
            .owner_import_surface_fact_tracer_installs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (entries, mut chain_facts) = cold_output;
        match finalise {
            crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                // Merge traced facts into the producer-side chain_facts
                // so the surface's fact_dep_signature is the union of
                // direct-fan-in observations and TLS-traced
                // sub-query observations.
                let mut seen: rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef> =
                    chain_facts.iter().cloned().collect();
                for fact in fact_dep_signature.iter() {
                    if seen.insert(fact.clone()) {
                        chain_facts.push(fact.clone());
                    }
                }
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                self.provenance
                    .owner_import_surface_overflow_refusals
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Refuse cache admission; the caller cold-recomputes
                // on the next request. Return Some(surface) so the
                // current call still has a fresh surface to consume
                // — but do NOT insert into the warm cache.
                let surface = crate::owner_import_surface::build_owner_import_surface(
                    Arc::from(owner_canonical),
                    whole_hash,
                    entries,
                    chain_facts,
                    validated_at_generation,
                );
                return Some(surface);
            }
        }

        let surface = crate::owner_import_surface::build_owner_import_surface(
            Arc::from(owner_canonical),
            whole_hash,
            entries,
            chain_facts,
            validated_at_generation,
        );
        surfaces.insert(Arc::from(owner_canonical), Arc::clone(&surface));
        Some(surface)
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
    #[cfg(any(test, debug_assertions))]
    #[allow(dead_code)]
    pub(crate) fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        let view = self.resolver_store_view();
        self.resolve_owner_direct_import_with_store_view(&view, owner_canonical, local_name)
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
