use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_core::utils::oxc::vue::resolve_type::{
    extract_export_surface, AnalyzedExternalTypeSource, ResolvedElements,
};
use verter_workspace::ResolveRequestKind;

use crate::{
    resolve_external_type_from_source_body, ExportRegistryView, ExternalTypeBodyCache,
    ExternalTypeBodyResolver, RegistryExportEntry, ResolverHash16,
};

pub trait ExternalTypeGraphResolver {
    type Error;

    fn max_external_type_resolve_steps(&self) -> usize;
    fn max_external_type_resolve_depth(&self) -> usize;
    fn missing_root_dependency(&self) -> Self::Error;
    fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error;
    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error;

    fn resolve_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: ResolveRequestKind,
    ) -> Option<String>;

    fn source_hash(&self, dep_canonical: &str) -> ResolverHash16;

    fn read_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String>;

    fn required_import_names_for_type(
        &self,
        dep_canonical: &str,
        type_name: &str,
        _effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
    ) -> FxHashSet<String> {
        let _ = dep_canonical;
        analysis.required_import_names(type_name)
    }

    fn resolve_named_export_target(
        &self,
        _dep_canonical: &str,
        _type_name: &str,
        _kind: ResolveRequestKind,
    ) -> Option<(String, String)> {
        None
    }

    fn debug_enabled(&self) -> bool {
        false
    }

    fn debug_log(&self, _message: String) {}

    fn cached_source_analysis(
        &self,
        _dep_canonical: &str,
        _effective_source: &str,
    ) -> Option<AnalyzedExternalTypeSource> {
        None
    }

    fn cached_barrel_state(&self, _barrel_canonical: &str) -> Option<crate::BarrelResolutionState> {
        None
    }

    fn persist_barrel_state(&self, _barrel_canonical: &str, _state: &crate::BarrelResolutionState) {
    }

    fn note_barrel_fact_reuse(&self) {}

    fn ensure_export_registry(&self, _canonical: &str) -> Option<ExportRegistryView> {
        None
    }

    /// Look up a previously resolved type in the host-level cache.
    fn lookup_resolved_type_cache(
        &self,
        _dep_canonical: &str,
        _type_name: &str,
        _kind: ResolveRequestKind,
    ) -> Option<crate::ExternalTypeResolvedCacheEntry> {
        None
    }

    fn note_resolved_type_cache_hit(&self) {}

    /// Called after successfully resolving a type body.
    /// Implementations store into a host-level resolved type cache.
    fn note_resolved_type(
        &self,
        _dep_canonical: &str,
        _type_name: &str,
        _resolved: Option<&ResolvedElements>,
        _tracked_deps: &[String],
    ) {
    }

    fn resolve_external_type_from_analysis(
        &self,
        dep_canonical: &str,
        type_name: &str,
        effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
        imported_companions: &FxHashMap<String, ResolvedElements>,
    ) -> Option<ResolvedElements>;
}

struct GraphBodyResolver<'a, R> {
    resolver: &'a R,
}

impl<R> ExternalTypeBodyResolver for GraphBodyResolver<'_, R>
where
    R: ExternalTypeGraphResolver,
{
    type Error = R::Error;

    fn max_external_type_resolve_steps(&self) -> usize {
        self.resolver.max_external_type_resolve_steps()
    }

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
        self.resolver.step_limit_exceeded(type_name, last_dep)
    }

    fn debug_enabled(&self) -> bool {
        self.resolver.debug_enabled()
    }

    fn debug_log(&self, message: String) {
        self.resolver.debug_log(message);
    }

    fn cached_source_analysis(
        &self,
        dep_canonical: &str,
        effective_source: &str,
    ) -> Option<AnalyzedExternalTypeSource> {
        self.resolver
            .cached_source_analysis(dep_canonical, effective_source)
    }

    fn required_import_names_for_type(
        &self,
        dep_canonical: &str,
        type_name: &str,
        effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
    ) -> FxHashSet<String> {
        self.resolver.required_import_names_for_type(
            dep_canonical,
            type_name,
            effective_source,
            analysis,
        )
    }

    fn note_resolved_type(
        &self,
        dep_canonical: &str,
        type_name: &str,
        resolved: Option<&ResolvedElements>,
        tracked_deps: &[String],
    ) {
        self.resolver
            .note_resolved_type(dep_canonical, type_name, resolved, tracked_deps);
    }

    fn resolve_external_type_from_analysis(
        &self,
        dep_canonical: &str,
        type_name: &str,
        effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
        imported_companions: &FxHashMap<String, ResolvedElements>,
    ) -> Option<ResolvedElements> {
        self.resolver.resolve_external_type_from_analysis(
            dep_canonical,
            type_name,
            effective_source,
            analysis,
            imported_companions,
        )
    }

    fn resolve_external_type_recursive(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<Option<ResolvedElements>, Self::Error> {
        resolve_external_type_from_graph(
            self.resolver,
            owner_canonical,
            import_source,
            type_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            required_root_dep,
            kind,
            use_host_cache,
            profile_hash,
            depth,
        )
    }

    fn resolve_type_through_barrel(
        &self,
        barrel_canonical: &str,
        type_name: &str,
        wildcard_sources: &[String],
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
        kind: ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
        _debug_enabled: bool,
    ) -> Result<Option<ResolvedElements>, Self::Error> {
        resolve_type_through_barrel_from_graph(
            self.resolver,
            barrel_canonical,
            type_name,
            wildcard_sources,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            kind,
            use_host_cache,
            profile_hash,
            depth,
        )
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_from_graph<R: ExternalTypeGraphResolver>(
    resolver: &R,
    owner_canonical: &str,
    import_source: &str,
    type_name: &str,
    tracked_deps: &mut BTreeSet<String>,
    resolution_deps: &mut BTreeSet<String>,
    cache: &mut ExternalTypeBodyCache,
    visiting: &mut FxHashSet<(String, String)>,
    required_root_dep: bool,
    kind: ResolveRequestKind,
    use_host_cache: bool,
    profile_hash: Option<u64>,
    depth: usize,
) -> Result<Option<ResolvedElements>, R::Error> {
    if depth >= resolver.max_external_type_resolve_depth() {
        return Err(resolver.depth_limit_exceeded(type_name, owner_canonical));
    }

    let Some(dep_canonical) =
        resolver.resolve_dependency_canonical(owner_canonical, import_source, kind)
    else {
        return if required_root_dep {
            Err(resolver.missing_root_dependency())
        } else {
            Ok(None)
        };
    };

    tracked_deps.insert(dep_canonical.clone());
    resolution_deps.insert(dep_canonical.clone());

    let cache_key = (dep_canonical.clone(), type_name.to_string());
    if let Some(cached) = cache.get(&cache_key) {
        return Ok(cached.clone());
    }

    // Check host-level resolved type cache before doing expensive source
    // parsing. This enables cross-owner reuse: if component A already resolved
    // Props from types.ts, component B can reuse the cached result.
    if use_host_cache {
        if let Some(entry) = resolver.lookup_resolved_type_cache(&dep_canonical, type_name, kind) {
            resolver.note_resolved_type_cache_hit();
            cache.insert(cache_key, entry.resolved.clone());
            tracked_deps.extend(entry.tracked_deps.iter().cloned());
            return Ok(entry.resolved);
        }
    }

    if let Some((target_canonical, target_type_name)) =
        resolver.resolve_named_export_target(&dep_canonical, type_name, kind)
    {
        if target_canonical != dep_canonical || target_type_name != type_name {
            tracked_deps.insert(target_canonical.clone());
            resolution_deps.insert(target_canonical.clone());
            if let Some(target_source) =
                resolver.read_source_for_type_resolution(&target_canonical, profile_hash)
            {
                let body_resolver = GraphBodyResolver { resolver };
                let resolved = resolve_external_type_from_source_body(
                    &body_resolver,
                    &target_canonical,
                    &target_type_name,
                    &target_source,
                    tracked_deps,
                    resolution_deps,
                    cache,
                    visiting,
                    kind,
                    use_host_cache,
                    profile_hash,
                    depth + 1,
                )?;
                cache.insert(cache_key, resolved.clone());
                return Ok(resolved);
            }
        }
    }

    let Some(effective_source) =
        resolver.read_source_for_type_resolution(&dep_canonical, profile_hash)
    else {
        if dep_canonical.ends_with(".vue") {
            cache.insert(cache_key, None);
            return Ok(None);
        }
        return if required_root_dep {
            Err(resolver.missing_root_dependency())
        } else {
            Ok(None)
        };
    };

    let body_resolver = GraphBodyResolver { resolver };
    let resolved = resolve_external_type_from_source_body(
        &body_resolver,
        &dep_canonical,
        type_name,
        &effective_source,
        tracked_deps,
        resolution_deps,
        cache,
        visiting,
        kind,
        use_host_cache,
        profile_hash,
        depth,
    )?;
    cache.insert(cache_key, resolved.clone());
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_type_through_barrel_from_graph<R: ExternalTypeGraphResolver>(
    resolver: &R,
    barrel_canonical: &str,
    type_name: &str,
    wildcard_sources: &[String],
    tracked_deps: &mut BTreeSet<String>,
    resolution_deps: &mut BTreeSet<String>,
    cache: &mut ExternalTypeBodyCache,
    visiting: &mut FxHashSet<(String, String)>,
    kind: ResolveRequestKind,
    use_host_cache: bool,
    profile_hash: Option<u64>,
    depth: usize,
) -> Result<Option<ResolvedElements>, R::Error> {
    let cached_barrel = cache
        .barrel_state(barrel_canonical)
        .cloned()
        .or_else(|| resolver.cached_barrel_state(barrel_canonical));
    let barrel_source_hash = resolver.source_hash(barrel_canonical);
    let valid_barrel = cached_barrel.as_ref().and_then(|state| {
        if state.source_hash != barrel_source_hash {
            return None;
        }
        for (child_canonical, expected_hash) in &state.scanned_sources {
            if resolver.source_hash(child_canonical) != *expected_hash {
                return None;
            }
        }
        Some(state.clone())
    });
    let specifier_to_canonical: Vec<(String, String)> = wildcard_sources
        .iter()
        .filter_map(|spec| {
            resolver
                .resolve_dependency_canonical(barrel_canonical, spec, kind)
                .map(|canonical| (spec.to_string(), canonical))
        })
        .collect();

    if let Some(ref state) = valid_barrel {
        resolver.note_barrel_fact_reuse();
        cache.store_barrel_state(barrel_canonical, state.clone());
    }
    let mut state = valid_barrel.unwrap_or_else(|| crate::BarrelResolutionState {
        export_map: FxHashMap::default(),
        source_hash: barrel_source_hash,
        wildcard_sources: wildcard_sources.to_vec(),
        scanned_sources: FxHashMap::default(),
        tracked_deps: FxHashSet::default(),
        fully_resolved: false,
        generation: 0,
    });

    for (child_specifier, child_canonical) in &specifier_to_canonical {
        let mut visited = FxHashSet::default();
        scan_barrel_export_surface_recursive_from_graph(
            resolver,
            child_specifier,
            child_canonical,
            type_name,
            &mut state,
            &mut visited,
            kind,
            profile_hash,
        );
        cache.store_barrel_state(barrel_canonical, state.clone());
        resolver.persist_barrel_state(barrel_canonical, &state);

        if state.export_map.contains_key(type_name) {
            tracked_deps.extend(state.tracked_deps.iter().cloned());
            resolution_deps.extend(state.tracked_deps.iter().cloned());
            return resolve_external_type_from_graph(
                resolver,
                barrel_canonical,
                child_specifier,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                false,
                kind,
                use_host_cache,
                profile_hash,
                depth + 1,
            );
        }
    }

    state.fully_resolved = specifier_to_canonical
        .iter()
        .all(|(_, canonical)| state.scanned_sources.contains_key(canonical));
    cache.store_barrel_state(barrel_canonical, state.clone());
    resolver.persist_barrel_state(barrel_canonical, &state);
    tracked_deps.extend(state.tracked_deps.iter().cloned());
    resolution_deps.extend(state.tracked_deps.iter().cloned());

    for (child_specifier, _child_canonical) in &specifier_to_canonical {
        if let Some(found) = resolve_external_type_from_graph(
            resolver,
            barrel_canonical,
            child_specifier,
            type_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            false,
            kind,
            use_host_cache,
            profile_hash,
            depth + 1,
        )? {
            return Ok(Some(found));
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn scan_barrel_export_surface_recursive_from_graph<R: ExternalTypeGraphResolver>(
    resolver: &R,
    root_specifier: &str,
    current_canonical: &str,
    type_name: &str,
    state: &mut crate::BarrelResolutionState,
    visited: &mut FxHashSet<String>,
    kind: ResolveRequestKind,
    profile_hash: Option<u64>,
) {
    if !visited.insert(current_canonical.to_string()) {
        return;
    }
    if state.scanned_sources.contains_key(current_canonical) {
        return;
    }

    let current_hash = resolver.source_hash(current_canonical);
    state.tracked_deps.insert(current_canonical.to_string());
    let wildcard_sources =
        if let Some(registry) = resolver.ensure_export_registry(current_canonical) {
            for (name, entry) in &registry.named {
                match entry {
                    RegistryExportEntry::Defined | RegistryExportEntry::Alias { .. } => {
                        state.export_map.entry(name.clone()).or_insert_with(|| {
                            (root_specifier.to_string(), current_canonical.to_string())
                        });
                    }
                }
            }
            state
                .scanned_sources
                .insert(current_canonical.to_string(), current_hash);
            registry.wildcard_edges
        } else {
            let Some(current_source) =
                resolver.read_source_for_type_resolution(current_canonical, profile_hash)
            else {
                state
                    .scanned_sources
                    .insert(current_canonical.to_string(), current_hash);
                return;
            };

            let alloc = oxc_allocator::Allocator::new();
            let surface = extract_export_surface(&current_source, &alloc);

            for name in &surface.exported_names {
                state
                    .export_map
                    .entry(name.clone())
                    .or_insert_with(|| (root_specifier.to_string(), current_canonical.to_string()));
            }
            state
                .scanned_sources
                .insert(current_canonical.to_string(), current_hash);
            surface.wildcard_reexport_sources
        };

    for nested_specifier in &wildcard_sources {
        let Some(nested_canonical) =
            resolver.resolve_dependency_canonical(current_canonical, nested_specifier, kind)
        else {
            continue;
        };
        scan_barrel_export_surface_recursive_from_graph(
            resolver,
            root_specifier,
            &nested_canonical,
            type_name,
            state,
            visited,
            kind,
            profile_hash,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_external_type_from_graph, ExternalTypeGraphResolver};
    use crate::{ExportRegistryView, ExternalTypeBodyCache, RegistryExportEntry, ResolverHash16};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_analysis::hash_16;
    use verter_core::utils::oxc::vue::resolve_type::{
        AnalyzedExternalTypeSource, ResolvedElements,
    };
    use verter_workspace::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        routes: FxHashMap<(String, String), String>,
        sources: FxHashMap<String, String>,
        registries: FxHashMap<String, ExportRegistryView>,
        named_export_targets: FxHashMap<(String, String, ResolveRequestKind), (String, String)>,
        analysis_results: FxHashMap<(String, String), Option<ResolvedElements>>,
        logs: RefCell<Vec<String>>,
        source_reads: RefCell<FxHashMap<String, usize>>,
        analysis_calls: RefCell<Vec<(String, String)>>,
    }

    impl ExternalTypeGraphResolver for TestResolver {
        type Error = String;

        fn max_external_type_resolve_steps(&self) -> usize {
            64
        }

        fn max_external_type_resolve_depth(&self) -> usize {
            16
        }

        fn missing_root_dependency(&self) -> Self::Error {
            "missing root dependency".to_string()
        }

        fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
            format!("depth limit exceeded for {type_name} at {last_dep}")
        }

        fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
            format!("step limit exceeded for {type_name} at {last_dep}")
        }

        fn resolve_dependency_canonical(
            &self,
            owner_canonical: &str,
            import_source: &str,
            _kind: ResolveRequestKind,
        ) -> Option<String> {
            self.routes
                .get(&(owner_canonical.to_string(), import_source.to_string()))
                .cloned()
        }

        fn source_hash(&self, dep_canonical: &str) -> ResolverHash16 {
            self.sources
                .get(dep_canonical)
                .map(|source| hash_16(source.as_bytes()))
                .unwrap_or_else(|| hash_16(dep_canonical.as_bytes()))
        }

        fn read_source_for_type_resolution(
            &self,
            dep_canonical: &str,
            _profile_hash: Option<u64>,
        ) -> Option<String> {
            *self
                .source_reads
                .borrow_mut()
                .entry(dep_canonical.to_string())
                .or_default() += 1;
            self.sources.get(dep_canonical).cloned()
        }

        fn resolve_named_export_target(
            &self,
            dep_canonical: &str,
            type_name: &str,
            kind: ResolveRequestKind,
        ) -> Option<(String, String)> {
            self.named_export_targets
                .get(&(dep_canonical.to_string(), type_name.to_string(), kind))
                .cloned()
        }

        fn debug_enabled(&self) -> bool {
            true
        }

        fn debug_log(&self, message: String) {
            self.logs.borrow_mut().push(message);
        }

        fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView> {
            self.registries.get(canonical).cloned()
        }

        fn resolve_external_type_from_analysis(
            &self,
            dep_canonical: &str,
            type_name: &str,
            effective_source: &str,
            analysis: &AnalyzedExternalTypeSource,
            imported_companions: &FxHashMap<String, ResolvedElements>,
        ) -> Option<ResolvedElements> {
            self.analysis_calls
                .borrow_mut()
                .push((dep_canonical.to_string(), type_name.to_string()));
            if let Some(result) = self
                .analysis_results
                .get(&(dep_canonical.to_string(), type_name.to_string()))
                .cloned()
                .flatten()
            {
                return Some(result);
            }

            let allocator = oxc_allocator::Allocator::new();
            let parsed =
                oxc_parser::Parser::new(&allocator, effective_source, oxc_span::SourceType::ts())
                    .parse();
            (!parsed.panicked).then(|| {
                verter_core::utils::oxc::vue::resolve_type::resolve_external_type_in_program_with_analyzed_symbol_companion(
                    type_name,
                    &parsed.program,
                    effective_source.as_bytes(),
                    analysis,
                    imported_companions,
                )
            })?
        }
    }

    #[test]
    fn resolve_external_type_from_graph_follows_direct_reexport_chain() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./types".to_string()),
            "/src/types.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/types.ts".to_string(), "./dep".to_string()),
            "/src/dep.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/types.ts".to_string(),
            "export { Props } from './dep'".to_string(),
        );
        resolver.sources.insert(
            "/src/dep.ts".to_string(),
            "export interface Props { msg: string }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("graph resolution should succeed");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/types.ts"));
        assert!(tracked.contains("/src/dep.ts"));
    }

    #[test]
    fn resolve_external_type_from_graph_scans_wildcard_barrels() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./barrel".to_string()),
            "/src/barrel.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./deep".to_string()),
            "/src/deep.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/barrel.ts".to_string(),
            "export * from './deep'".to_string(),
        );
        resolver.sources.insert(
            "/src/deep.ts".to_string(),
            "export interface Props { msg: string }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("barrel graph resolution should succeed");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/deep.ts"));
    }

    #[test]
    fn resolve_external_type_from_graph_uses_named_export_target_before_reading_barrel_source() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./types".to_string()),
            "/src/types/index.ts".to_string(),
        );
        resolver.named_export_targets.insert(
            (
                "/src/types/index.ts".to_string(),
                "ButtonProps".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            (
                "/src/components/Button.vue".to_string(),
                "ButtonProps".to_string(),
            ),
        );
        resolver.sources.insert(
            "/src/components/Button.vue".to_string(),
            "export interface ButtonProps { label: string }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./types",
            "ButtonProps",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("named export routing should bypass the unreadable barrel source");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/types/index.ts"));
        assert!(tracked.contains("/src/components/Button.vue"));
        assert!(!tracked.contains("/src/deep.ts"));
    }

    #[test]
    fn resolve_external_type_from_graph_reuses_request_local_barrel_state_across_lookups() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./barrel".to_string()),
            "/src/barrel.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./a".to_string()),
            "/src/a.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./b".to_string()),
            "/src/b.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/barrel.ts".to_string(),
            "export * from './a'\nexport * from './b'".to_string(),
        );
        resolver.sources.insert(
            "/src/a.ts".to_string(),
            "export interface Foo { one: string }".to_string(),
        );
        resolver.sources.insert(
            "/src/b.ts".to_string(),
            "export interface Bar { two: string }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let first = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./barrel",
            "Foo",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("first wildcard lookup should resolve");
        assert!(first.is_some());
        let reads_after_first = resolver.source_reads.borrow().clone();

        let second = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./barrel",
            "Bar",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("second wildcard lookup should reuse the local barrel state");
        assert!(second.is_some());

        let reads = resolver.source_reads.borrow();
        assert_eq!(
            reads.get("/src/a.ts").copied().unwrap_or_default(),
            reads_after_first
                .get("/src/a.ts")
                .copied()
                .unwrap_or_default(),
            "later lookups in the same request should not rescan the already-checked sibling",
        );
        assert_eq!(
            reads.get("/src/b.ts").copied().unwrap_or_default(),
            reads_after_first.get("/src/b.ts").copied().unwrap_or_default() + 2,
            "the second lookup should only add one scan read and one body read for the newly-relevant sibling",
        );
    }

    #[test]
    fn resolve_external_type_from_graph_uses_export_registry_for_barrel_scans() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./barrel".to_string()),
            "/src/barrel.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./child".to_string()),
            "/src/child.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/barrel.ts".to_string(),
            "export * from './child'".to_string(),
        );
        resolver.sources.insert(
            "/src/child.ts".to_string(),
            "export interface Props { msg: string }".to_string(),
        );
        resolver.registries.insert(
            "/src/child.ts".to_string(),
            ExportRegistryView {
                source_hash: hash_16(b"export interface Props { msg: string }"),
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: Vec::new(),
            },
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("registry-backed wildcard lookup should resolve");

        assert!(actual.is_some());
        assert_eq!(
            resolver
                .source_reads
                .borrow()
                .get("/src/child.ts")
                .copied()
                .unwrap_or_default(),
            1,
            "graph barrel scanning should use cached export registries so the child source is only read for the final body resolution",
        );
    }

    #[test]
    fn resolve_external_type_from_graph_preserves_barrel_wildcard_source_order() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./barrel".to_string()),
            "/src/barrel.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./legacy".to_string()),
            "/src/legacy.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/barrel.ts".to_string(), "./Button".to_string()),
            "/src/Button.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/barrel.ts".to_string(),
            "export * from './legacy'\nexport * from './Button'".to_string(),
        );
        resolver.sources.insert(
            "/src/legacy.ts".to_string(),
            "export interface ButtonProps { source: 'legacy' }".to_string(),
        );
        resolver.sources.insert(
            "/src/Button.ts".to_string(),
            "export interface ButtonProps { source: 'button' }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./barrel",
            "ButtonProps",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("graph barrel lookup should resolve");

        assert!(actual.is_some());
        let reads = resolver.source_reads.borrow();
        assert_eq!(
            reads.get("/src/Button.ts").copied().unwrap_or_default(),
            0,
            "later wildcard siblings should not win over an earlier duplicate export purely because their path matches the requested name",
        );
        assert_eq!(
            reads.get("/src/legacy.ts").copied().unwrap_or_default(),
            2,
            "graph barrel resolution should follow the first declared matching wildcard child",
        );
    }

    #[test]
    fn resolve_external_type_from_graph_delegates_analysis_resolution_to_resolver() {
        let mut resolver = TestResolver::default();
        resolver.routes.insert(
            ("/src/Consumer.vue".to_string(), "./types".to_string()),
            "/src/types.ts".to_string(),
        );
        resolver
            .sources
            .insert("/src/types.ts".to_string(), "const nope = 1".to_string());
        resolver.analysis_results.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            Some(ResolvedElements::default()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();
        let actual = resolve_external_type_from_graph(
            &resolver,
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            false,
            None,
            0,
        )
        .expect("graph resolution should delegate analysis resolution to the resolver");

        assert!(actual.is_some());
        assert_eq!(
            resolver.analysis_calls.borrow().as_slice(),
            &[("/src/types.ts".to_string(), "Props".to_string())],
        );
    }
}
