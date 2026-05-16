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

use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_options_fingerprint,
    extract_component_meta_from_resolved, ComponentMetaOptions, HostFenceValidator,
};

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
fn strip_owner_route_fact(
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
    /// build runs inside a [`CompletionFence`](crate::completion_fence::CompletionFence)
    /// bounded to 3 attempts; repeated revalidation failures surface as a
    /// top-level `None` result rather than a publish of torn state.
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
        {
            crate::host_manage::overlay_priority::prewarm_view_overlays(self, view);
        }

        // Try the view-aware warm cache fast path.
        if let Some(warm) = self.try_component_meta_cache_hit_with_view(canonical.as_str(), view) {
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
        let ((resolved_opt, meta_opt), read_set) = self.with_fact_tracer(|| {
            let resolved = match self.resolve_component_meta_with_view(
                canonical.as_str(),
                crate::types::ProjectionMode::Expanded,
                view,
            ) {
                Some(r) => r,
                None => return (None, None),
            };
            let meta = extract_component_meta_from_resolved(
                self,
                canonical.as_str(),
                &resolved,
                true, // include_fallthrough
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
    /// owner and return the warm payload only when its recorded
    /// dep-signature revalidates against the live host. Returns `None` on
    /// any miss, stale entry, or missing shallow state.
    fn try_component_meta_cache_hit(
        &self,
        canonical: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        let shallow = self.shallow_file_state(canonical)?;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: shallow.whole_hash,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        // Bind a host-rooted view via the `ResolverContext::view()`
        // trait accessor. The session-less call path has no overlay,
        // so the host's default `view()` impl returns a `HostViewRef`
        // — the overlay-free read substrate. Routing through the trait
        // accessor (via a `&dyn ResolverContext` cast) makes the view
        // extension point uniformly observable: dyn-dispatched calls
        // through `view()` exercise the trait method so static
        // dead-code analysis sees the production caller (R18 — view is
        // passed by explicit argument, no thread-local).
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = self;
        let session_view = ctx.view();
        // Block 1.B: fact-precise validation runs first via
        // `ComponentMetaResultDb::get_with_view`. The view threaded
        // in here is the resolver-tier `HostStoreView` because
        // [`StoreView::validates_fact_signature`] is defined on
        // `StoreView`, not on `SessionView`. The session view
        // remains in scope below for the legacy whole-hash oracle.
        let store_view = self.resolver_store_view();
        let entry = self
            .project_type_store
            .component_meta_results()
            .get_with_view(self, &store_view, &key)?;
        let validator = HostFenceValidator {
            host: self,
            view: session_view.as_ref(),
        };
        use crate::completion_fence::FenceValidator;
        let dep_sig_valid = entry
            .read_set_signature
            .legacy
            .iter()
            .all(|(canonical_id, version)| validator.validate(canonical_id, version));
        if !dep_sig_valid {
            return None;
        }
        // The DB stores
        // `CachedComponentMetaResult { analysis, resolution_template, ... }`
        // so the with_resolution path can rehydrate without re-running the
        // cold resolver. The plain `get_component_meta` warm path returns
        // only the analysis projection.
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
    fn try_component_meta_cache_hit_with_view(
        &self,
        canonical: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        // Tombstoned canonicals (overlay-Delete) report `None` for
        // content hash AND source. Short-circuit the warm path: a
        // tombstoned overlay does NOT have a meaningful component-meta
        // result and must NOT collapse onto a base cache slot.
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
            owner_whole_hash,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        // Block 1.B: fact-precise validation runs first via
        // `ComponentMetaResultDb::get_with_view`. The view threaded
        // in here is the resolver-tier `HostStoreView` (matching the
        // base path); the session-aware view in scope continues to
        // gate the legacy whole-hash oracle below.
        let store_view = self.resolver_store_view();
        let entry = self
            .project_type_store
            .component_meta_results()
            .get_with_view(self, &store_view, &key)?;
        let validator = HostFenceValidator { host: self, view };
        use crate::completion_fence::FenceValidator;
        let dep_sig_valid = entry
            .read_set_signature
            .legacy
            .iter()
            .all(|(canonical_id, version)| validator.validate(canonical_id, version));
        if !dep_sig_valid {
            return None;
        }
        Some(entry.payload.analysis.clone())
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
    ) {
        if resolved.synthesis_should_suppress {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion (view-aware path): synthesis_should_suppress=true",
            );
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
            owner_whole_hash: whole_hash,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        let dep_signature = Self::build_component_meta_dep_signature(
            canonical,
            whole_hash,
            self.project_type_store.project_generation(),
            &resolved.fact_versions,
        );
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
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(cached),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                    admitted_signature,
                    dep_signature,
                ),
            },
        );
    }

    /// Publish the cold-build result into the project-global
    /// final-result cache. The dep-signature carries the owner's whole-hash,
    /// the current project generation, and every transitive file fact the
    /// resolver observed while producing the result. A later lookup
    /// revalidates the full signature against the live host so an edit to
    /// *any* file the resolver touched invalidates the cached payload — not
    /// just edits to the owner itself.
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
    ) {
        if resolved.synthesis_should_suppress {
            tracing::debug!(
                target: "verter::audit::record",
                file = %canonical,
                "skipping component-meta cache promotion: synthesis_should_suppress=true",
            );
            return;
        }
        let Some(shallow) = self.shallow_file_state(canonical) else {
            return;
        };
        let whole_hash = shallow.whole_hash;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: whole_hash,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        let dep_signature = Self::build_component_meta_dep_signature(
            canonical,
            whole_hash,
            self.project_type_store.project_generation(),
            &resolved.fact_versions,
        );
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
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(cached),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                    admitted_signature,
                    dep_signature,
                ),
            },
        );
    }

    /// Lower the resolver's observed fact-version list into a transitive
    /// `DepSignature`. Owner + project-generation facts always participate;
    /// file whole-hashes discovered during resolution are deduped per
    /// canonical so a single entry per touched file ends up in the signature.
    /// Derived-fact hashes (route / import-route) are intentionally skipped
    /// for now — they are validated via their underlying file hashes plus
    /// the project-generation bump on shape changes. Including them in the
    /// signature would require extending `HostFenceValidator` with a
    /// derived-fact-aware path, which lands with the cut.
    fn build_component_meta_dep_signature(
        owner_canonical: &str,
        owner_whole_hash: Hash16,
        project_gen: u64,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) -> crate::semantic_query::DepSignature {
        use crate::semantic_query::DepVersion;
        let mut entries: Vec<(Arc<str>, DepVersion)> = Vec::with_capacity(fact_versions.len() + 2);
        entries.push((
            Arc::<str>::from(owner_canonical),
            DepVersion::WholeHash(owner_whole_hash),
        ));
        entries.push((
            Arc::<str>::from(owner_canonical),
            DepVersion::ProjectGeneration(project_gen),
        ));
        let mut seen: rustc_hash::FxHashSet<(Arc<str>, Hash16)> = rustc_hash::FxHashSet::default();
        seen.insert((Arc::<str>::from(owner_canonical), owner_whole_hash));
        for fact in fact_versions {
            if let crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } = fact
            {
                let canonical: Arc<str> = Arc::from(canonical_id.as_str());
                if seen.insert((canonical.clone(), *hash)) {
                    entries.push((canonical, DepVersion::WholeHash(*hash)));
                }
            }
        }
        Arc::from(entries.into_boxed_slice())
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
            Some(std::sync::Arc::new(
                crate::component_meta_audit::RequestFootprintAccumulator::new(),
            ))
        } else {
            None
        };
        let ctx = crate::request_context::RequestContext::with_kind_and_timing(
            request_id,
            std::sync::Arc::<str>::from(canonical.as_str()),
            verter_audit::RequestKind::ComponentMeta,
            footprint_capture,
            self.config.audit_timing_capture && self.config.audit_enabled,
            accumulator.clone(),
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
        //
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
        let analysis =
            extract_component_meta_from_resolved(self, canonical.as_str(), &resolved, true);
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
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: shallow.whole_hash,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        // Block 1.B: fact-precise validation runs first via
        // `ComponentMetaResultDb::get_with_view`. The view threaded
        // in here is the resolver-tier `HostStoreView`; the session
        // view in scope below continues to gate the legacy whole-hash
        // oracle.
        let store_view = self.resolver_store_view();
        let entry = self
            .project_type_store
            .component_meta_results()
            .get_with_view(self, &store_view, &key)?;
        // Bind a host-rooted view; the warm-cache fast path on
        // `VerterHost` has no session context, so the overlay-free
        // `HostViewRef` is the correct read substrate.
        let view = crate::session_view::HostViewRef::new(self);
        let validator = HostFenceValidator {
            host: self,
            view: &view,
        };
        use crate::completion_fence::FenceValidator;
        let dep_sig_valid = entry
            .read_set_signature
            .legacy
            .iter()
            .all(|(canonical_id, version)| validator.validate(canonical_id, version));
        if !dep_sig_valid {
            return None;
        }

        // Rehydrate the resolution template into a fresh per-request state.
        // Returns None on the bounded eviction race where the snapshot
        // was evicted between dep_signature validation and reload.
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
                ..Default::default()
            };
            // Warm-cache replay carries the same parent-request and
            // scheduler attribution the live request would have
            // observed had it run cold — read both off the active
            // request context (installed by the audited entry-point
            // a few lines above this branch). The same context lookup
            // also surfaces the per-request peak-RSS slot.
            let mut memory = crate::component_meta_audit::RequestMemoryAudit::default();
            let (parent_request_id, scheduler_audit, waits, trace_id) =
                match crate::request_context::current_request_context() {
                    Some(ctx) if ctx.request_id == request_id => {
                        memory.process_rss_peak_bytes = ctx
                            .process_rss_peak_bytes
                            .load(std::sync::atomic::Ordering::Relaxed);
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
                        (
                            ctx.parent_request_id.map(|id| id.to_string()),
                            ctx.scheduler_audit.lock().clone(),
                            waits,
                            ctx.trace_id.clone(),
                        )
                    }
                    _ => (None, None, None, String::new()),
                };
            let synthesized = crate::component_meta_audit::RequestAuditRecord {
                request_id,
                canonical_id: canonical.to_string(),
                kind: crate::component_meta_audit::RequestKind::ComponentMeta,
                parent_request_id,
                timings: crate::component_meta_audit::RequestTimingAudit::default(),
                store,
                memory,
                footprint: None,
                scheduler: scheduler_audit,
                files: Vec::new(),
                waits,
                from_cache: true,
                kind_payload: crate::component_meta_audit::RequestKindPayload::ComponentMeta(
                    crate::component_meta_audit::ComponentMetaPayload::default(),
                ),
                trace_id,
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
