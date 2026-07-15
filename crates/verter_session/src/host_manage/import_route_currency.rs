//! Generation-current `ImportRoute` route-table derivation.
//!
//! The `ImportRoute` derived-fact producers for `VerterHost`: the
//! generation-current route-table snapshot (the edge-currency-gated
//! `IndexedReady` table MERGED with the post-index `DerivedRawState`
//! routes, every entry gated by the per-entry freshness oracle and
//! generation-stale entries re-resolved against the current workspace
//! generation before hashing), the unresolved-wildcard source-coverage
//! variant, and the shared hashing helpers. Lives in its own module to
//! keep `component_meta_methods.rs` inside the god-module size budget;
//! the methods are consumed by the store-view build and the
//! component-meta fact-version capture.

use crate::types::Hash16;
use crate::VerterHost;

/// The owner's effective import-route table selected for `ImportRoute`
/// fact production, plus the per-entry staleness classification the
/// hash producer needs.
struct ImportRouteTableRead {
    /// The merged route table: the content-pinned, edge-currency-gated
    /// `IndexedReady` table plus every post-index `DerivedRawState`
    /// route the indexed table does not cover.
    routes: std::sync::Arc<rustc_hash::FxHashMap<String, crate::types::DependencyResolution>>,
    /// Entries that are NOT generation-current per the per-entry
    /// freshness oracle (`import_route_entry_is_generation_current`) —
    /// re-resolved by the hash producer through their recorded
    /// resolution lane. Maps specifier → the recorded positive-stamp
    /// resolution kind (`None` for known-misses, whose sidecar records
    /// no lane). Always empty for entries served by the
    /// edge-currency-gated `IndexedReady` arm.
    stale_specifiers: rustc_hash::FxHashMap<String, Option<verter_workspace::ResolveRequestKind>>,
}

impl VerterHost {
    /// Generation-current `ImportRoute` derived-fact hash for a file.
    ///
    /// The `ImportRoute` derived fact records a file's effective
    /// import-target surface: which specifiers it imports and what
    /// each one resolves to. The content-pinned read below
    /// (`current_content_pinned_indexed`) is edge-currency-gated on the
    /// complete `IndexedReady::has_cross_file_edges` authority and
    /// re-indexes an edge-stale surface, so the indexed table's entries
    /// are current by construction (a dependency-set change routes the
    /// artifact through the edge-refresh before its routes are read
    /// here). Routes recorded *after* indexing — a compile-prefetch or
    /// external `src=` memo, a caller push that has not been re-baked
    /// yet — live only in `DerivedRawState`; they are MERGED into the
    /// table (never shadowed by the indexed arm), each gated by the
    /// per-entry freshness oracle.
    ///
    /// A file with an **unresolvable** specifier records that specifier
    /// as a known-miss snapshot of one workspace generation. When a new
    /// file later satisfies the previously-unresolvable specifier the
    /// workspace `content_generation` advances while the importer's
    /// content — hence its `IndexedReady` — does not. A hash that kept
    /// reporting the stale miss would let a dependent cache entry
    /// validate against its own stale snapshot and never observe the
    /// appearance.
    ///
    /// This oracle closes that gap: every entry that fails the
    /// per-entry freshness oracle — a known-miss whose sidecar stamp no
    /// longer matches the live generation, or a HOST-MEMOIZED positive
    /// whose capture-before-resolve stamp went stale (the same
    /// dependency-set-derived class: a `.d.ts` companion or
    /// more-specific sibling can retarget it while the owner's content
    /// stays put) — is re-resolved against the *current* workspace
    /// before hashing, through its RECORDED resolution lane. A
    /// specifier that has since become resolvable or retargeted folds
    /// its new target into the hash, so the hash differs from the
    /// stored snapshot, the `ImportRoute` fact mismatches on warm
    /// validation, and the dependent cache entry recomputes. Tables
    /// whose entries are all generation-current take the cached-hash
    /// fast path with no re-resolution.
    pub(crate) fn generation_current_import_route_hash(
        &self,
        canonical_id: &str,
    ) -> Option<Hash16> {
        let read = self.current_import_route_table(canonical_id)?;
        Some(self.hash_generation_current_route_table(
            canonical_id,
            &read.routes,
            &read.stale_specifiers,
        ))
    }

    /// Coverage-checked variant of [`Self::generation_current_import_route_hash`]
    /// (coverage-checked `ImportRoute` admission).
    ///
    /// Identical route-source selection and stale-entry rehash logic, but
    /// returns `None` unless EVERY `required_source` is present as a key in the
    /// owner's route table. The hole-2 rooting loop records the unresolved
    /// wildcard edges it actually hit as `(owner, source)` pairs; an owner can
    /// have a fully-resolved route surface (so the plain
    /// `generation_current_import_route_hash` returns `Some`) that nonetheless
    /// does NOT track the wildcard source — e.g. a PARTIAL
    /// `set_import_dependencies` snapshot that resolves a sibling but omits the
    /// wildcard. Hashing that partial table produces a fact that is reproduced
    /// verbatim after the wildcard target appears, stale-serving the cached
    /// `Miss`. Requiring full coverage forces the rooting loop to fall back to
    /// the empty-facts negative-cache path (served, never persisted) when the
    /// hash cannot root every unresolved wildcard the traversal hit.
    pub(crate) fn generation_current_import_route_hash_covering_sources(
        &self,
        canonical_id: &str,
        required_sources: &[String],
    ) -> Option<Hash16> {
        let read = self.current_import_route_table(canonical_id)?;
        // The produced hash can only root a known-miss the rooting loop must
        // observe if the source is actually present in the table re-resolved
        // by `hash_generation_current_route_table`. A required source absent
        // from the table is silently dropped from the hash — refuse to admit.
        for source in required_sources {
            if !read.routes.contains_key(source) {
                return None;
            }
        }
        Some(self.hash_generation_current_route_table(
            canonical_id,
            &read.routes,
            &read.stale_specifiers,
        ))
    }

    /// Select the owner's effective import-route table for fact production —
    /// the single source order shared by
    /// [`Self::generation_current_import_route_hash`] and its coverage-checked
    /// sibling.
    ///
    /// Content-pinned IndexedReady read first. A permissive `get_any`
    /// would let a stale `IndexedReady` surface its old route table;
    /// `current_content_pinned_indexed` is gated on
    /// `indexed_surface_is_current` (the complete-authority edge gate +
    /// project stamp) and re-indexes a stale candidate, so the indexed
    /// arm's entries are current by construction — they enter the merge
    /// with NO stale classification.
    ///
    /// The IndexedReady route table is the import-target surface captured
    /// when the file was indexed. Routes recorded *after* indexing — a
    /// compile-prefetch or external `src=` resolution that lands in
    /// `DerivedRawState` via `cache_positive_import_route_result`, or a
    /// caller push not yet re-baked — do not back-fill the
    /// already-materialised `IndexedReady`. They are MERGED in here: a
    /// mixed file (script import + external `src=` route) must record an
    /// `ImportRoute` fact covering BOTH families, otherwise a `src=`
    /// retarget leaves the recorded fact reproduced verbatim and the
    /// dependent compile slot survives the retarget. On a key present in
    /// both tables the indexed entry wins (it is edge-current by
    /// construction; an overlapping `DerivedRawState` memo from the same
    /// generation resolves identically, and an older one is exactly what
    /// the re-index replaced).
    ///
    /// Every merged `DerivedRawState` entry is classified by the
    /// per-entry freshness oracle
    /// (`import_route_entry_is_generation_current`): stale entries —
    /// stale-stamped positives AND stale known-misses (including the
    /// fail-closed missing-stamp case) — are recorded in
    /// `stale_specifiers` with their recorded resolution lane so the
    /// hash producer re-resolves exactly them. Generation-current
    /// entries (including current known-misses) hash as stored — the
    /// oracle, not an entry-class heuristic, is the single staleness
    /// policy.
    fn current_import_route_table(&self, canonical_id: &str) -> Option<ImportRouteTableRead> {
        let indexed_routes = self
            .current_content_pinned_indexed(canonical_id)
            .map(|facts| std::sync::Arc::clone(&facts.import_routes))
            .filter(|routes| !routes.is_empty());

        // Post-index routes live on DerivedRawState (D48 split). Collect
        // the entries the indexed table does not cover, classifying each
        // through the per-entry freshness oracle. The dashmap ref is
        // dropped before any further host call.
        let mut stale_specifiers: rustc_hash::FxHashMap<
            String,
            Option<verter_workspace::ResolveRequestKind>,
        > = rustc_hash::FxHashMap::default();
        let mut post_index: Vec<(String, crate::types::DependencyResolution)> = Vec::new();
        if let Some(entry) = self.derived_raw_cache().get(canonical_id) {
            let live_generation = self.ws().content_generation();
            for (specifier, resolution) in entry.import_routes.iter() {
                if indexed_routes
                    .as_ref()
                    .is_some_and(|routes| routes.contains_key(specifier))
                {
                    continue;
                }
                if !entry.import_route_entry_is_generation_current(
                    specifier,
                    resolution,
                    live_generation,
                ) {
                    stale_specifiers.insert(
                        specifier.clone(),
                        entry.positive_route_resolution_kind(specifier),
                    );
                }
                post_index.push((specifier.clone(), resolution.clone()));
            }
        }

        if post_index.is_empty() {
            // Backstop: a genuinely route-less file (no indexed routes and
            // no DerivedRawState routes) has no `ImportRoute` surface.
            return indexed_routes.map(|routes| ImportRouteTableRead {
                routes,
                stale_specifiers,
            });
        }
        let mut merged = indexed_routes
            .map(|routes| (*routes).clone())
            .unwrap_or_default();
        merged.extend(post_index);
        Some(ImportRouteTableRead {
            routes: std::sync::Arc::new(merged),
            stale_specifiers,
        })
    }

    /// Hash the owner's import-route table after re-resolving every
    /// oracle-stale entry against the current workspace — the shared
    /// body of [`Self::generation_current_import_route_hash`] and its
    /// coverage-checked sibling. `stale_specifiers` is the per-entry
    /// oracle's verdict computed by `current_import_route_table`:
    /// stale-stamped HOST-MEMOIZED positives (the dependency file set
    /// moved since the route was memoized — a `.d.ts` companion, a
    /// more-specific sibling) and stale known-misses (a previously
    /// unresolvable dependency may have appeared). Each re-resolves
    /// side-effect-free through its RECORDED resolution lane
    /// (`generation_current_route_resolution`) so the re-resolved
    /// canonical agrees with what the original recorder would produce
    /// today. A table whose entries are all generation-current takes
    /// the cached-hash fast path with no re-resolution; "every
    /// specifier resolved" is NOT by itself stability — a stamp-stale
    /// positive is resolved yet possibly retargeted.
    fn hash_generation_current_route_table(
        &self,
        canonical_id: &str,
        routes: &rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
        stale_specifiers: &rustc_hash::FxHashMap<
            String,
            Option<verter_workspace::ResolveRequestKind>,
        >,
    ) -> Hash16 {
        if stale_specifiers.is_empty() {
            // Every entry is generation-current — the route table is
            // stable until the importer's own content changes or the
            // workspace generation moves.
            return crate::resolver_store::hash_import_route_targets(routes);
        }

        // Re-resolve the stale entries against the current workspace so
        // the hash reflects appearance of a previously unresolvable
        // dependency and retargeting of a stale-stamped positive. This
        // runs on a cache-validation read path, so the re-resolve uses
        // the side-effect-free `generation_current_route_resolution`
        // (the shared `resolve_route_edge_canonical` policy, or the
        // recorded `SfcSrcAttr` lane for external `src=` memos) rather
        // than `resolve_type_dependency_canonical` — the latter can
        // materialize a shallow-only importer (`ensure_indexed_ready_serve`)
        // and rewrite the `import_routes` entry
        // (`cache_positive_import_route_result`) while merely building
        // the hash.
        let mut generation_current: rustc_hash::FxHashMap<
            String,
            crate::types::DependencyResolution,
        > = rustc_hash::FxHashMap::default();
        for (specifier, resolution) in routes.iter() {
            if let Some(recorded_kind) = stale_specifiers.get(specifier) {
                let current = self.generation_current_route_resolution(
                    canonical_id,
                    specifier,
                    *recorded_kind,
                );
                generation_current.insert(
                    specifier.clone(),
                    crate::types::DependencyResolution {
                        specifier: specifier.clone(),
                        resolved_canonical_id: current.clone(),
                        possible_canonical_ids: current.into_iter().collect(),
                    },
                );
            } else {
                generation_current.insert(specifier.clone(), resolution.clone());
            }
        }
        crate::resolver_store::hash_import_route_targets(&generation_current)
    }
}
