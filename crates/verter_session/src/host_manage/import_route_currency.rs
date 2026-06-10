//! Generation-current `ImportRoute` route-table derivation.
//!
//! The `ImportRoute` derived-fact producers for `VerterHost`: the
//! generation-current route-table snapshot (known-miss entries
//! re-resolved against the current workspace generation before hashing),
//! the unresolved-wildcard source-coverage variant, and the shared
//! hashing helpers. Lives in its own module to keep
//! `component_meta_methods.rs` inside the god-module size budget; the
//! methods are consumed by the store-view build and the component-meta
//! fact-version capture.

use crate::types::Hash16;
use crate::VerterHost;

impl VerterHost {
    /// Generation-current `ImportRoute` derived-fact hash for a file.
    ///
    /// The `ImportRoute` derived fact records a file's effective
    /// import-target surface: which specifiers it imports and what
    /// each one resolves to. A file's positive resolutions only
    /// change when the file's own content changes (which re-keys its
    /// content-addressed `IndexedReady`), so for a fully-resolved file
    /// indexed with a non-empty route table the content-pinned
    /// `IndexedReady.import_routes` is authoritative. A file whose
    /// route table was populated *after* indexing keeps an empty
    /// `IndexedReady` route table; the populated `DerivedRawState`
    /// route table answers for it instead (see the source comment on
    /// the route-source selection below).
    ///
    /// A file with an **unresolvable** specifier is different: the
    /// cached route table records that specifier as a known-miss, but
    /// the miss is a snapshot of one workspace generation. When a new
    /// file later satisfies the previously-unresolvable specifier the
    /// workspace `content_generation` advances while the importer's
    /// content — hence its `IndexedReady` — does not. The
    /// content-pinned `import_route_hash` would then keep reporting
    /// the stale miss, so a cache entry whose `fact_versions` carry
    /// the importer's `ImportRoute` fact would validate against its
    /// own stale snapshot and never observe the appearance.
    ///
    /// This oracle closes that gap: when the cached route table has a
    /// known-miss entry, the miss specifiers are re-resolved against
    /// the *current* workspace before hashing. A specifier that has
    /// since become resolvable folds its new target into the hash, so
    /// the hash differs from the stored snapshot, the `ImportRoute`
    /// fact mismatches on warm validation, and the dependent cache
    /// entry recomputes. Fully-resolved files take the cached-hash
    /// fast path with no re-resolution.
    pub(crate) fn generation_current_import_route_hash(
        &self,
        canonical_id: &str,
    ) -> Option<Hash16> {
        let routes = self.current_import_route_table(canonical_id)?;
        Some(self.hash_generation_current_route_table(canonical_id, &routes))
    }

    /// Coverage-checked variant of [`Self::generation_current_import_route_hash`]
    /// (coverage-checked `ImportRoute` admission).
    ///
    /// Identical route-source selection and known-miss rehash logic, but
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
        let routes = self.current_import_route_table(canonical_id)?;
        // The produced hash can only root a known-miss the rooting loop must
        // observe if the source is actually present in the table re-resolved
        // by `hash_generation_current_route_table`. A required source absent
        // from the table is silently dropped from the hash — refuse to admit.
        for source in required_sources {
            if !routes.contains_key(source) {
                return None;
            }
        }
        Some(self.hash_generation_current_route_table(canonical_id, &routes))
    }

    /// Select the owner's effective import-route table for fact production —
    /// the single source order shared by
    /// [`Self::generation_current_import_route_hash`] and its coverage-checked
    /// sibling.
    ///
    /// Content-pinned IndexedReady read. A permissive `get_any`
    /// would let a stale `IndexedReady` surface its old route
    /// table; `current_content_pinned_indexed` returns `None` for
    /// a stale candidate so the `DerivedRawState` fallback answers
    /// with the live-tracked route table.
    ///
    /// The IndexedReady route table is the import-target surface
    /// captured when the file was indexed. It can be empty even
    /// when the file does have resolved imports: routes recorded
    /// *after* indexing — e.g. a compile-prefetch or external
    /// `src=` resolution that lands in `DerivedRawState` via
    /// `cache_positive_import_route_result` — do not back-fill the
    /// already-materialised `IndexedReady`. An empty IndexedReady
    /// route table must therefore fall through to the
    /// `DerivedRawState` table rather than shadow it, otherwise a
    /// file whose routes were populated post-indexing yields no
    /// `ImportRoute` fact and dependent caches miss route changes.
    /// The `.filter(non-empty)` makes an empty content-pinned table
    /// behave the same as a missing one and defer to the fallback.
    fn current_import_route_table(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<rustc_hash::FxHashMap<String, crate::types::DependencyResolution>>>
    {
        let routes = self
            .current_content_pinned_indexed(canonical_id)
            .map(|facts| std::sync::Arc::clone(&facts.import_routes))
            .filter(|routes| !routes.is_empty())
            .or_else(|| {
                // import_routes lives on DerivedRawState (D48 split).
                self.derived_raw_cache()
                    .get(canonical_id)
                    .map(|entry| std::sync::Arc::new(entry.import_routes.clone()))
            })?;
        // Backstop: a genuinely route-less file (no indexed routes and
        // no `DerivedRawState` routes) has no `ImportRoute` surface.
        if routes.is_empty() {
            return None;
        }
        Some(routes)
    }

    /// Hash the owner's import-route table after re-resolving any known-miss
    /// specifier against the current workspace generation — the shared body of
    /// [`Self::generation_current_import_route_hash`] and its coverage-checked
    /// sibling.
    fn hash_generation_current_route_table(
        &self,
        canonical_id: &str,
        routes: &rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
    ) -> Hash16 {
        let has_known_miss = routes.values().any(Self::import_route_is_known_miss);
        if !has_known_miss {
            // Every specifier resolved — the route table is stable
            // until the importer's own content changes.
            return crate::resolver_store::hash_import_route_targets(routes);
        }

        // Re-resolve the known-miss specifiers against the current
        // workspace so the hash reflects appearance of a previously
        // unresolvable dependency. This runs on a cache-validation read
        // path, so the re-resolve uses the side-effect-free
        // `generation_current_known_miss_resolution` rather than
        // `resolve_type_dependency_canonical` — the latter can
        // materialize a shallow-only importer (`ensure_indexed_ready`)
        // and rewrite the `import_routes` known-miss entry to a positive
        // (`cache_positive_import_route_result`) while merely building
        // the hash.
        let mut generation_current: rustc_hash::FxHashMap<
            String,
            crate::types::DependencyResolution,
        > = rustc_hash::FxHashMap::default();
        for (specifier, resolution) in routes.iter() {
            if Self::import_route_is_known_miss(resolution) {
                let current =
                    self.generation_current_known_miss_resolution(canonical_id, specifier);
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
