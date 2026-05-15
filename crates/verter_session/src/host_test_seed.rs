//! Test-only `impl VerterHost` seeding helpers.
//!
//! Contains the [`VerterHost::seed_indexed_ready_for_test`] entry point
//! used by hermetic tests to populate the canonical post-parse artifact
//! into [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! plus the resolver runtime's prepared-decl bundles.
//!
//! Gated behind `#[cfg(test)]` so the helper is absent from production
//! builds and does not extend the runtime API surface.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::types::{FileAnalysisSnapshot, Hash16};
use crate::VerterHost;

#[cfg(test)]
impl VerterHost {
    /// Seed `FileArtifactStore` with pre-built data for tests.
    pub(crate) fn seed_indexed_ready_for_test(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
        export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
        external_type_analysis: Arc<
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource,
        >,
        shallow_state: Arc<crate::resolver_core::ShallowFileState>,
        snapshot: Option<Arc<FileAnalysisSnapshot>>,
        eval_source: Option<Arc<str>>,
        import_routes: rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
    ) {
        let effective_whole_hash = if whole_hash == Hash16::default() {
            crate::hash::hash_16(raw_source.as_bytes())
        } else {
            whole_hash
        };
        let snapshot = snapshot.unwrap_or_else(|| Arc::new(FileAnalysisSnapshot::default()));
        let eval_source = eval_source.unwrap_or_else(|| Arc::clone(&raw_source));
        let mut shallow_state = (*shallow_state).clone();
        shallow_state.whole_hash = effective_whole_hash;

        let route_target = |specifier: &str| {
            import_routes.get(specifier).and_then(|resolution| {
                resolution
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| resolution.effective_target().map(str::to_string))
            })
        };
        for target in shallow_state.import_targets.values_mut() {
            if target.canonical_id.is_empty() {
                if let Some(resolved) = route_target(&target.source_specifier) {
                    target.canonical_id = resolved;
                }
            }
        }
        for export in shallow_state.exports.values_mut() {
            if let crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                canonical_id,
                ..
            } = export
            {
                if canonical_id.is_empty() {
                    if let Some(resolved) = route_target(source_specifier) {
                        *canonical_id = resolved;
                    }
                }
            }
        }
        for wildcard in &mut shallow_state.wildcard_reexports {
            if wildcard.canonical_id.is_empty() {
                if let Some(resolved) = route_target(&wildcard.source_specifier) {
                    wildcard.canonical_id = resolved;
                }
            }
        }

        // Insert import_routes into the source-content-domain DB
        // (DerivedRawState — D48 split). `import_routes` is a sub-mirror
        // of `IndexedReady.import_routes` with its own invalidation
        // trigger; see DerivedRawState type docstring.
        if !import_routes.is_empty() {
            self.derived_raw_cache()
                .entry(canonical_id.to_string())
                .or_default()
                .import_routes = import_routes.clone();
        }

        let shallow_state = Arc::new(shallow_state);

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        let import_routes_arc = Arc::new(import_routes.clone());

        // route_hash mirror — see host_manage.rs equivalent for the
        // rationale (cache content-derived hash so
        // current_derived_fact_hash skips per-call rehashing).
        let route_hash = shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

        // Project the AppConfig-interface flag onto IndexedReady from
        // the merged analysis snapshot (test seed path mirrors the
        // production path's projection logic).
        let declares_interface_app_config = script_analysis
            .as_ref()
            .map(|sa| {
                sa.flags.contains(
                    verter_semantic::analysis::AnalysisFlags::DECLARES_INTERFACE_APP_CONFIG,
                )
            })
            .unwrap_or(false);

        // Publish the canonical post-parse artifact into FileArtifactStore. This
        // is the single authoritative cache consumers read from.
        let indexed = crate::project_type_store::IndexedReady {
            whole_hash: effective_whole_hash,
            shallow_state: Arc::clone(&shallow_state),
            import_routes: Arc::clone(&import_routes_arc),
            import_route_hash,
            route_hash,
            raw_source,
            eval_source,
            cached_parse,
            script_analysis,
            export_signatures,
            snapshot,
            external_type_analysis,
            declares_interface_app_config,
        };
        self.project_type_store
            .indexed()
            .insert(Arc::from(canonical_id), Arc::new(indexed));

        let mut dep_edges = FxHashMap::default();
        for target in shallow_state.import_targets.values() {
            if !target.canonical_id.is_empty() {
                dep_edges
                    .entry(target.source_specifier.clone())
                    .or_insert_with(|| target.canonical_id.clone());
            }
        }
        for export in shallow_state.exports.values() {
            if let crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                canonical_id,
                ..
            } = export
            {
                if !canonical_id.is_empty() {
                    dep_edges
                        .entry(source_specifier.clone())
                        .or_insert_with(|| canonical_id.clone());
                }
            }
        }
        for wildcard in &shallow_state.wildcard_reexports {
            if !wildcard.canonical_id.is_empty() {
                dep_edges
                    .entry(wildcard.source_specifier.clone())
                    .or_insert_with(|| wildcard.canonical_id.clone());
            }
        }

        let bundle = Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                Arc::clone(&shallow_state),
                dep_edges,
                FxHashMap::default(),
            ),
        );
        let mut bundle_facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: effective_whole_hash,
        }];
        if !import_routes.is_empty() {
            bundle_facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: crate::resolver_store::hash_import_route_targets(&import_routes),
            });
        }
        self.resolver.runtime.prepared_decl_bundles.insert_arc(
            canonical_id.to_owned(),
            bundle,
            bundle_facts,
        );
    }
}
