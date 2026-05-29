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
        use crate::file_artifact_store::{AugmentationTargetKey, AugmentationTargetKind};
        use verter_semantic::facts::registry::{InternedGlobPattern, InternedSpecifier};

        let store = self.project_type_store.indexed();
        let env = self.host_view_env_hashes_for(canonical);
        let project_identity = self.host_view_project_identity_for(canonical);
        let make_key = |target: AugmentationTargetKind| AugmentationTargetKey {
            project_identity,
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            target,
        };
        // The augmenter's relative `declare module "./x"` specifiers
        // resolve against the augmenter's own canonical through the live
        // type-dependency resolver — the same authority the
        // augmentation-stitching pass uses.
        let resolver = |augmenter_canonical: &str, specifier: &str| {
            self.resolve_type_dependency_canonical(augmenter_canonical, specifier)
                .map(Arc::from)
        };
        // Bind a shared reference so each probe passes the resolver by
        // reference without re-borrowing at every call site.
        let resolver = &resolver;
        let any_non_empty = |target: AugmentationTargetKind| {
            !store
                .ensure_augmentation_index_populated(&make_key(target), resolver)
                .entries
                .is_empty()
        };

        // Collect the per-import probe targets from the owner's own import
        // specifiers: an external (`declare module "vue"`) or relative
        // (`declare module "./x"`) augmenter can only reach the owner
        // through a matching import. `ensure_indexed_ready` materialises
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
        let indexed_opt = self.ensure_indexed_ready(canonical);
        if let Some(indexed) = indexed_opt.as_ref() {
            for import in indexed.shallow_state.import_targets.values() {
                // Materialise each resolved dependency so a direct-dep
                // augmenter enters the store BEFORE any target probe runs:
                // the index cold-scan installs its result on first query,
                // and an augmenter that has not entered `FileArtifactStore`
                // contributes nothing (R29). Indexing every dep up front
                // (rather than interleaved with probing) keeps the result
                // independent of the unordered import-table iteration.
                if !import.canonical_id.is_empty() {
                    let _ = self.ensure_indexed_ready(&import.canonical_id);
                }
                let specifier = import.source_specifier.as_str();
                if specifier.starts_with("./") || specifier.starts_with("../") {
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
            for import in &indexed.snapshot.imports {
                if !import.bindings.is_empty() {
                    continue;
                }
                let specifier = import.source.as_str();
                let Some(resolved_canonical) =
                    self.resolve_type_dependency_canonical(canonical, specifier)
                else {
                    continue;
                };
                if resolved_canonical.is_empty() {
                    continue;
                }
                // Materialise the resolved dep so the augmenter's
                // `FileArtifacts` (with its `ModuleAugmentationFact`
                // entries) enter the artifact store and the index
                // cold-scan corpus before any target probe runs (mirror
                // of the `import_targets`-driven materialisation
                // above).
                let _ = self.ensure_indexed_ready(&resolved_canonical);
                // Read the augmenter's own augmentation facts and emit
                // a structurally-matching target kind per fact. The
                // augmenter file may not even live in the store at
                // legacy-key shape if materialisation produced only the
                // content-addressed key — `get_artifacts_any` performs
                // the permissive canonical-only lookup that ignores
                // `content_hash` for this exact callsite.
                if let Some(augmenter_artifacts) = store.get_artifacts_any(&resolved_canonical) {
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
                            &resolved_canonical,
                            &augmenter_artifacts.augmentations,
                            resolver,
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
                        } else if fact_specifier.starts_with("./")
                            || fact_specifier.starts_with("../")
                        {
                            // Resolve the augmenter's relative
                            // `declare module "./X"` against the
                            // augmenter's own canonical — same authority
                            // `augmenter_matches_target` uses.
                            if let Some(target_canonical) = self.resolve_type_dependency_canonical(
                                &resolved_canonical,
                                fact_specifier,
                            ) {
                                if !target_canonical.is_empty() {
                                    per_import_targets.push(
                                        AugmentationTargetKind::ResolvedRelativeCanonical(
                                            Arc::from(target_canonical.as_str()),
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
            }
        }

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
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn compile_output_pure_content_entry_count(&self) -> usize {
        self.project_type_store
            .compile_output_pure_content()
            .entry_count()
    }
}
