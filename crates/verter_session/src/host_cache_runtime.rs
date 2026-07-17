//! `impl VerterHost` — cache-runtime accessors and probes.
//!
//! Owns the host-side methods that drive the compile-cache mode
//! classifier and surface the content-addressed compile-output node:
//! - [`VerterHost::workspace_aliases_for_canonical`] — configured
//!   workspace path-aliases for the project that owns a canonical;
//! - [`VerterHost::owner_has_module_augmentation_dependency`] — the
//!   closure-aware probe that floors a `Content`-requested compile to
//!   `Stateless` (so the fact-validated `Session` route runs) whenever
//!   any module augmentation could reach the owner's declaration graph;
//! - [`VerterHost::compile_output_pure_content`] — read accessor for the
//!   project-wide content-addressed `CompileOutputNode_PureContent`;
//! - [`VerterHost::compile_output_pure_content_entry_count`] — test-only
//!   helper that integration tests use to verify `Content` mode publishes
//!   into this node while `Session` / `Stateless` modes do not.
//!
//! Construction (field initialisation in `new` /
//! `new_with_scheduler_config` / `new_standalone` /
//! `new_standalone_with_scheduler_config`) stays in
//! [`crate::host_construction`]. Eviction lives in
//! [`crate::host_construction::VerterHost::drop_all_per_canonical_compile_caches`]
//! and the per-file invalidation paths under [`crate::host_upsert`].

use std::sync::Arc;

use crate::VerterHost;

impl VerterHost {
    /// Configured workspace path-aliases for the project that owns
    /// `canonical`, or an empty vec when no configured project claims it
    /// (ambient libs, fallback projects, or a workspace without a
    /// published snapshot).
    ///
    /// Used by the compile-cache mode classifier's
    /// [`crate::compile_cache_mode::has_workspace_alias`] predicate so a
    /// `Content`-requested compile of a file that resolves an import
    /// through an alias is correctly recognised as alias-dependent. The
    /// resolved alias dependency is part of the session resolution state,
    /// so `Session` mode stays eligible regardless.
    #[must_use]
    pub(crate) fn workspace_aliases_for_canonical(
        &self,
        canonical: &str,
    ) -> Vec<verter_workspace::WorkspaceAlias> {
        use verter_workspace::workspace_snapshot::ProjectPayload;
        let Some(root) = self.workspace().published_root() else {
            return Vec::new();
        };
        let snapshot = &root.snapshot;
        let Some(project_id) = snapshot.owners_for_file(canonical).first().copied() else {
            return Vec::new();
        };
        match snapshot
            .projects
            .get(project_id.0 as usize)
            .map(|p| &p.payload)
        {
            Some(ProjectPayload::Configured {
                workspace_aliases, ..
            }) => workspace_aliases.clone(),
            _ => Vec::new(),
        }
    }

    /// True iff a compile of `canonical` could consume any module
    /// augmentation reachable from its declaration graph, under the live
    /// project resolve / lib env.
    ///
    /// The compile-cache mode classifier hands the result to
    /// [`crate::compile_cache_mode::EligibilityInputs::owner_has_module_augmentation`].
    /// A content-addressed `Content` key carries no augmenter fingerprint,
    /// so editing an augmenter that contributes to a consumed module would
    /// leave the key byte-identical and serve stale output; a `true`
    /// result floors a `Content` request to `Stateless` and routes the
    /// fact-validated `Session` path instead.
    ///
    /// The signal is closure-aware via the augmentation TARGET index, not
    /// the owner's own declared augmentations: an imported / ambient
    /// augmenter (`declare module "vue"` in a sibling `.d.ts`) leaves NO
    /// trace on the owner's `FileArtifacts.augmentations`. The probe set is
    /// the union of:
    ///
    /// * one target per owner import specifier — bare specifiers probe
    ///   [`AugmentationTargetKind::ExternalSpecifier`], relative specifiers
    ///   probe [`AugmentationTargetKind::ResolvedRelativeCanonical`]
    ///   against the import's already-resolved canonical;
    /// * [`AugmentationTargetKind::GlobalAugmentation`] (a `declare global`
    ///   block augments every file regardless of imports);
    /// * one [`AugmentationTargetKind::WildcardAmbient`] per distinct
    ///   wildcard pattern declared anywhere in the base artifact set (a
    ///   wildcard ambient applies via a matching import, so it cannot be
    ///   derived from the owner's specifiers alone — see
    ///   [`crate::file_artifact_store::FileArtifactStore::declared_wildcard_ambient_patterns`]).
    ///
    /// Each target routes through
    /// [`crate::file_artifact_store::FileArtifactStore::ensure_augmentation_index_populated`],
    /// which cold-scans + installs on a miss and warm-hits on a hit — the
    /// index is populated lazily, so a passive `get` would read "empty"
    /// for a target that simply has not been queried yet. Returns `true`
    /// as soon as any probed target resolves to a non-empty augmenter set.
    ///
    /// The classifier calls this only for `Content` requests (`Session`
    /// stays `Session` under every reason and `Stateless` is the floor),
    /// so the scan cost is paid only on the rare explicit `Content`
    /// opt-in.
    #[must_use]
    pub(crate) fn owner_has_module_augmentation_dependency(&self, canonical: &str) -> bool {
        use crate::fact_emission::GLOBAL_AUGMENTATION_TAG;
        use crate::file_artifact_store::{
            AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind,
        };
        use verter_semantic::facts::registry::{InternedGlobPattern, InternedSpecifier};

        let store = self.project_type_store.indexed();
        let env = self.host_view_env_hashes_for(canonical);
        let project_identity = self.host_view_project_identity_for(canonical);
        // Cache-mode classification is a base resolve-domain probe (no session
        // overlay), so it keys under `Base` and scans base artifacts only.
        let make_key = |target: AugmentationTargetKind| AugmentationTargetKey {
            project_identity,
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            population: AugmentationPopulation::Base,
            target,
        };
        // The augmenter's relative `declare module "./x"` specifiers
        // resolve against the augmenter's own canonical through the live
        // type-dependency resolver — the same authority the
        // augmentation-stitching pass uses.
        //
        // Memoise the `(augmenter_canonical, specifier) → resolved`
        // map per invocation. The walk resolves the same tuple from
        // multiple sites: direct emission for `declare module "./X"`
        // facts, the re-export edge walk, and inside
        // `ensure_augmentation_index_populated`'s cold scan
        // (`augmenter_matches_target` calls the resolver per
        // candidate fact). A multi-fact augmenter that retargets the
        // same sibling specifier multiplies these duplicate resolves
        // without the memo. The cache is invocation-local — the
        // resolver is closure-bound to `&self` so a global cache would
        // need a lifetime story; per-invocation suffices because the
        // probe runs once per `Content` request.
        type ResolveMemo =
            std::cell::RefCell<rustc_hash::FxHashMap<(Arc<str>, Arc<str>), Option<Arc<str>>>>;
        let resolve_memo: ResolveMemo = std::cell::RefCell::new(rustc_hash::FxHashMap::default());
        let memoised_resolve = |augmenter_canonical: &str, specifier: &str| -> Option<Arc<str>> {
            let key = (
                Arc::<str>::from(augmenter_canonical),
                Arc::<str>::from(specifier),
            );
            if let Some(cached) = resolve_memo.borrow().get(&key).cloned() {
                return cached;
            }
            let resolved = self
                .resolve_type_dependency_canonical(augmenter_canonical, specifier)
                .map(Arc::<str>::from);
            resolve_memo.borrow_mut().insert(key, resolved.clone());
            resolved
        };
        // Bind a shared reference so each probe passes the resolver by
        // reference without re-borrowing at every call site.
        let resolver = &memoised_resolve;
        let any_non_empty = |target: AugmentationTargetKind| {
            !store
                .ensure_augmentation_index_populated(&make_key(target), resolver, None)
                .entries
                .is_empty()
        };

        // Collect the per-import probe targets from the owner's own import
        // specifiers: an external (`declare module "vue"`) or relative
        // (`declare module "./x"`) augmenter can only reach the owner
        // through a matching import. `ensure_indexed_ready_serve` materialises
        // the owner's artifact (and its shallow import table) into the
        // store on a miss.
        //
        // Two parallel sources contribute import specifiers:
        // (1) [`ShallowFileState::import_targets`] — only carries imports
        //     that introduced AT LEAST ONE local binding (named, default,
        //     or namespace) because the shallow inventory is keyed on the
        //     local name. A pure side-effect import
        //     (`import "./augment";`) has NO local binding and therefore
        //     leaves no entry here.
        // (2) [`FileAnalysisSnapshot::imports`] — the parser-level
        //     `AnalyzedImport` list always pushes ONE entry per
        //     `ImportDeclaration` regardless of binding count, so a
        //     side-effect import shows up here with `bindings.is_empty()`.
        //     `resolve_snapshot_imports` populates `resolved_canonical_id`
        //     for every entry, including side-effect ones.
        //
        // (1) is sufficient to cover named / default / namespace imports.
        // The cache-mode classifier additionally needs side-effect imports
        // because a side-effect augmenter (`import "./aug";` where
        // `aug.ts` carries a `declare module "./local" {}` augmentation)
        // would otherwise leave the consumer in `Content` mode and serve
        // stale output when the augmenter is edited. Iterate snapshot
        // imports to pick up the side-effect-only relative case.
        let mut per_import_targets: Vec<AugmentationTargetKind> = Vec::new();
        // ReturnOnly never publishes — FAIL-CLOSED rule for this probe.
        // A successful-but-FENCED serve published NO artifacts row, so
        // the served file's augmentation facts are invisible to the
        // index cold-scan (`current_content_pinned_artifacts` reads the
        // published row) and the probe would fail OPEN: a `Content`
        // compile admits a content-addressed entry carrying no augmenter
        // fingerprint into the one cache family with NO read-side fact
        // rail. Any fenced serve in the walk therefore answers `true`
        // (augmentation inventory unverifiable for this request — floor
        // the compile to the fact-validated route; the next request
        // re-probes against published state).
        let indexed_serve = self.ensure_indexed_ready_serve(canonical);
        if indexed_serve
            .as_ref()
            .is_some_and(|serve| !serve.store_published)
        {
            return true;
        }
        let indexed_opt = indexed_serve.map(|serve| serve.indexed);
        if let Some(indexed) = indexed_opt.as_ref() {
            for import in indexed.shallow_state.import_targets.values() {
                // Materialise each resolved dependency so a direct-dep
                // augmenter enters the store BEFORE any target probe runs:
                // the index cold-scan installs its result on first query,
                // and an augmenter that has not entered `FileArtifactStore`
                // contributes nothing (R29). Indexing every dep up front
                // (rather than interleaved with probing) keeps the result
                // independent of the unordered import-table iteration. A
                // FENCED dep serve published no artifacts row — fail
                // closed (see the owner serve above).
                if !import.canonical_id.is_empty()
                    && self
                        .ensure_indexed_ready_serve(&import.canonical_id)
                        .is_some_and(|serve| !serve.store_published)
                {
                    return true;
                }
                let specifier = import.source_specifier.as_str();
                // Relative classification is the full TS `pathIsRelative`
                // class (bare `.`/`..` + `./`/`../`/`.\`/`..\` prefixes) —
                // the SAME predicate the workspace resolver uses. A
                // narrower `./`/`../` prefix check buckets a bare-`..`
                // import as `ExternalSpecifier("..")`, which a relative
                // `declare module './index'` fact can never match: the
                // probe reports "no augmenters", and a Content request
                // admits a content-addressed entry with NO augmenter
                // fingerprint — stale serves after the augmenter edits.
                if verter_workspace::resolver::is_relative_specifier(specifier) {
                    per_import_targets.push(AugmentationTargetKind::ResolvedRelativeCanonical(
                        Arc::from(import.canonical_id.as_str()),
                    ));
                } else if !specifier.contains('*') {
                    per_import_targets.push(AugmentationTargetKind::ExternalSpecifier(
                        InternedSpecifier::from(specifier),
                    ));
                }
            }
            // Side-effect imports (no bindings) escape the
            // `import_targets` map because that map is keyed by local
            // name. Walk the parser-level `snapshot.imports` list and
            // handle any entry whose `bindings.is_empty()`, regardless
            // of whether the specifier is relative
            // (`import "./augment";`) or bare
            // (`import "pkg-augment";`). Both shapes can deliver a
            // `declare module "<target>"` augmentation that the
            // content-addressed key would otherwise miss — the relative
            // form re-exports an augmenter file whose
            // `declare module "./local"` retargets a sibling canonical;
            // the bare form pulls in a packaged augmenter whose
            // `declare module "vue"` (or similar) retargets an external
            // module the owner does not import by binding. The probe's
            // structural question is "does any canonical we materialise
            // declare ANY augmentation" — the answer is independent of
            // the augmentation's target shape.
            //
            // The `IndexedReady` snapshot returned here may carry an
            // unresolved `AnalyzedImport.resolved_canonical_id` (that
            // field is populated by `resolve_snapshot_imports` on a
            // separate code path used by component-meta callers; the
            // augmentation probe is reached earlier). Resolve the
            // specifier directly through the live type-dependency
            // resolver — the same authority `import_targets` uses for
            // binding-driven imports. The resolver handles both
            // relative and bare specifiers uniformly (workspace
            // routing, `.d.ts` preference, alias mappings).
            //
            // After materialising the resolved augmenter, enumerate its
            // own `ModuleAugmentationFact` entries and emit one
            // `AugmentationTargetKind` per fact so the existing index
            // probe machinery (`ensure_augmentation_index_populated`)
            // picks the augmenter up regardless of whether it augments
            // an external module (`declare module "vue"`), a relative
            // sibling (`declare module "./local"`), a wildcard ambient,
            // or the global scope. This is the structural sibling of
            // the binding-driven `import_targets` loop above: that
            // loop derives the per-import target kind from the OWNER's
            // import specifier (the augmented module the owner consumes);
            // here we derive it from the AUGMENTER's own augmentation
            // facts because a side-effect import gives the owner no
            // local hint about which module is being augmented.
            // Side-effect imports may resolve to a barrel that itself
            // carries NO `ModuleAugmentationFact` entries but re-exports
            // the actual augmenter file (`pkg/index.d.ts` doing
            // `export * from "./augment"` where `augment.d.ts` declares
            // `declare module "vue"`). Walk each side-effect-imported
            // canonical and its re-export edges iteratively so the
            // augmenter is discovered regardless of whether it lives at
            // the import target or deeper in the re-export chain.
            //
            // `REEXPORT_WALK_DEPTH` bounds the chain length; a barrel
            // re-exporting through a few internal modules is normal,
            // but unbounded recursion would let a pathological re-export
            // cycle stall the probe.
            const REEXPORT_WALK_DEPTH: usize = 8;
            let mut visited: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
            let mut queue: std::collections::VecDeque<(Arc<str>, usize)> =
                std::collections::VecDeque::new();
            for import in &indexed.snapshot.imports {
                if !import.bindings.is_empty() {
                    continue;
                }
                let specifier = import.source.as_str();
                let Some(resolved_canonical) = resolver(canonical, specifier) else {
                    continue;
                };
                if resolved_canonical.is_empty() {
                    continue;
                }
                queue.push_back((resolved_canonical, 0));
            }
            while let Some((resolved_canonical, depth)) = queue.pop_front() {
                if !visited.insert(Arc::clone(&resolved_canonical)) {
                    continue;
                }
                if depth >= REEXPORT_WALK_DEPTH {
                    continue;
                }
                // Materialise the resolved dep so the augmenter's
                // `FileArtifacts` (with its `ModuleAugmentationFact`
                // entries) enter the artifact store and the index
                // cold-scan corpus before any target probe runs (mirror
                // of the `import_targets`-driven materialisation
                // above). An un-materialisable canonical (absent file,
                // stale leftover the authority gates reject) contributes
                // NOTHING — its retained store rows must not feed the
                // walk state no serving path would return. A FENCED
                // serve published no artifacts row — fail closed (see
                // the owner serve above).
                let Some(walk_serve) = self.ensure_indexed_ready_serve(&resolved_canonical) else {
                    continue;
                };
                if !walk_serve.store_published {
                    return true;
                }
                let walk_indexed = walk_serve.indexed;
                // Read the augmenter's own augmentation facts and emit a
                // structurally-matching target kind per fact — through
                // the SAME current-content-pinned accessor every other
                // cross-file-edge reader uses (the permissive
                // `get_artifacts_any` scan can return an arbitrary stale
                // content row when an old version lingers as a
                // multi-candidate sibling).
                if let Some(augmenter_artifacts) =
                    self.current_content_pinned_artifacts(&resolved_canonical)
                {
                    // Invalidate any `augmentation_index` entry whose
                    // cold scan ran BEFORE this augmenter entered the
                    // store. A pre-augmenter probe of the same target
                    // (e.g. another file imported `'vue'` and triggered
                    // an empty cold scan for `ExternalSpecifier("vue")`
                    // before pkg-augment was loaded) would warm-hit
                    // here and falsely report "no augmenters for this
                    // target" — letting a `Content` request reuse a
                    // content-addressed entry that does not fingerprint
                    // the augmenter. Removing the stale entries forces
                    // the next probe to cold-scan against the now-
                    // fresh artifact set.
                    if !augmenter_artifacts.augmentations.is_empty() {
                        store.invalidate_augmentation_index_for_augmenter(
                            &augmenter_artifacts.augmentations,
                        );
                    }
                    for fact in augmenter_artifacts.augmentations.iter() {
                        let fact_specifier: &str = fact.specifier.as_ref();
                        if fact_specifier == GLOBAL_AUGMENTATION_TAG {
                            per_import_targets.push(AugmentationTargetKind::GlobalAugmentation);
                        } else if fact_specifier.contains('*') {
                            per_import_targets.push(AugmentationTargetKind::WildcardAmbient(
                                InternedGlobPattern::from(fact_specifier),
                            ));
                        } else if verter_workspace::resolver::is_relative_specifier(fact_specifier)
                        {
                            // Resolve the augmenter's relative
                            // `declare module "./X"` against the
                            // augmenter's own canonical — same authority
                            // `augmenter_matches_target` uses (and the
                            // same full `pathIsRelative` class, so a
                            // `declare module '..'` fact resolves to the
                            // parent index instead of masquerading as an
                            // external module named `..`). Routed
                            // through the invocation-local memo so the
                            // same `(augmenter, specifier)` tuple
                            // resolves at most once across the per-fact
                            // emission and the
                            // `ensure_augmentation_index_populated`
                            // cold scan.
                            if let Some(target_canonical) =
                                resolver(&resolved_canonical, fact_specifier)
                            {
                                if !target_canonical.is_empty() {
                                    per_import_targets.push(
                                        AugmentationTargetKind::ResolvedRelativeCanonical(
                                            target_canonical,
                                        ),
                                    );
                                }
                            }
                        } else {
                            per_import_targets.push(AugmentationTargetKind::ExternalSpecifier(
                                InternedSpecifier::from(fact_specifier),
                            ));
                        }
                    }
                }
                // A barrel entry may carry NO augmentation facts itself
                // and re-export the actual augmenter through
                // `export * from "./augment"` or
                // `export { X } from "./augment"`. The resolver
                // materialises the barrel; walking its re-export edges
                // here lets the per-fact probe see the augmenter at
                // the next BFS level. The same iterative walk also
                // covers chained barrels (a barrel re-exporting a
                // barrel) up to `REEXPORT_WALK_DEPTH`.
                //
                // The `shallow_state.wildcard_reexports[i].canonical_id`
                // and `ExportTarget::Reexport.canonical_id` fields are
                // populated by the shallow-analysis resolver, which can
                // leave them empty for declaration files whose
                // module-resolution context differs from the live
                // type-dependency resolver (e.g. packaged `.d.ts`
                // entries). Re-resolve the raw source specifier through
                // `resolve_type_dependency_canonical` here — same
                // authority the binding-driven walk above uses — so a
                // barrel whose shallow-time resolver returned `""`
                // still surfaces the re-exported augmenter.
                {
                    use crate::resolver_core::shallow_file_state::ExportTarget;
                    // The barrel edges come from the artifact the ensure
                    // above returned (never a permissive `get_any` scan,
                    // which can surface a stale multi-candidate row).
                    // Baked edge `canonical_id`s are ROUTE-derived: they
                    // are consumed only while the surface passes the
                    // shared currency gate; a route-stale surface (e.g.
                    // a fenced ReturnOnly serve under sustained churn)
                    // re-resolves the raw source specifiers through the
                    // live resolver instead.
                    let baked_edges_current =
                        self.indexed_surface_is_current(&resolved_canonical, &walk_indexed);
                    for wildcard in &walk_indexed.shallow_state.wildcard_reexports {
                        let target_canonical: Option<Arc<str>> =
                            if baked_edges_current && !wildcard.canonical_id.is_empty() {
                                Some(Arc::from(wildcard.canonical_id.as_str()))
                            } else {
                                resolver(&resolved_canonical, &wildcard.source_specifier)
                                    .filter(|c| !c.is_empty())
                            };
                        if let Some(c) = target_canonical {
                            queue.push_back((c, depth + 1));
                        }
                    }
                    for target in walk_indexed.shallow_state.exports.values() {
                        if let ExportTarget::Reexport {
                            canonical_id: cached_canonical,
                            source_specifier,
                            ..
                        } = target
                        {
                            let target_canonical: Option<Arc<str>> =
                                if baked_edges_current && !cached_canonical.is_empty() {
                                    Some(Arc::from(cached_canonical.as_str()))
                                } else {
                                    resolver(&resolved_canonical, source_specifier)
                                        .filter(|c| !c.is_empty())
                                };
                            if let Some(c) = target_canonical {
                                queue.push_back((c, depth + 1));
                            }
                        }
                    }
                }
            }
        }

        // Dedup the emitted target set before probing. A multi-fact
        // augmenter that retargets the same external module twice
        // (`declare module "vue" { ... } declare module "vue" { ... }`),
        // or whose facts overlap with the owner's binding-driven
        // probes, would otherwise probe the same `(target, env)` key
        // multiple times. Each duplicate probe still warm-hits the
        // augmentation_index after the first, but the dedup keeps the
        // walk linear in distinct targets — clearer when the probe
        // count gets reported in audit telemetry.
        let mut seen: rustc_hash::FxHashSet<AugmentationTargetKind> =
            rustc_hash::FxHashSet::default();
        per_import_targets.retain(|target| seen.insert(target.clone()));

        // Probe every target through the (now fully materialised) store.
        // Ambient kinds apply WITHOUT a precise importer-specifier match —
        // a global block augments every file and a wildcard ambient
        // augments any matching import — so they are always probed.
        for target in per_import_targets {
            if any_non_empty(target) {
                return true;
            }
        }
        if any_non_empty(AugmentationTargetKind::GlobalAugmentation) {
            return true;
        }
        for pattern in store.declared_wildcard_ambient_patterns() {
            if any_non_empty(AugmentationTargetKind::WildcardAmbient(pattern)) {
                return true;
            }
        }
        false
    }

    /// Content-addressed compile-output cache node, shared project-wide.
    /// Used by the `CompileCacheMode::Content` compile route; the
    /// fact-validated `Session` route uses the per-profile compile cache
    /// instead.
    #[must_use]
    pub(crate) fn compile_output_pure_content(
        &self,
    ) -> &crate::cache_runtime::CompileOutputNodePureContent {
        self.project_type_store.compile_output_pure_content()
    }

    /// Number of entries in the content-addressed compile-output cache.
    /// Used by integration tests to verify that `Content`-mode publishes
    /// land in this node while `Session` / `Stateless` modes do not.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn compile_output_pure_content_entry_count(&self) -> usize {
        self.project_type_store
            .compile_output_pure_content()
            .entry_count()
    }
}
