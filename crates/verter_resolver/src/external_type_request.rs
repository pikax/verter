use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_core::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_vfs::ResolveRequestKind;

use crate::{
    resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
    RegistryResolvedTarget, RegistryRoute, ResolverHash16,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalTypeRouteEntry {
    pub owner_hash: ResolverHash16,
    pub target: Option<RegistryResolvedTarget>,
    pub tracked_deps: Vec<String>,
    pub route_hashes: Vec<(String, ResolverHash16)>,
    pub negative_barrel_generation: Option<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct ExternalTypeResolvedCacheEntry {
    pub resolved: Option<ResolvedElements>,
    pub tracked_deps: Vec<String>,
}

pub trait ExternalTypeRequestResolver: ExternalTypeBodyResolver {
    fn max_external_type_resolve_depth(&self) -> usize;

    fn missing_root_dependency(&self) -> Self::Error;

    fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error;

    fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: ResolveRequestKind,
    ) -> Option<String>;

    fn lookup_route_cache(
        &self,
        _owner_canonical: &str,
        _import_source: &str,
        _type_name: &str,
        _kind: ResolveRequestKind,
    ) -> Option<ExternalTypeRouteEntry> {
        None
    }

    fn whole_hash(&self, canonical: &str) -> ResolverHash16;

    fn compute_source_hash(&self, source: &str) -> ResolverHash16;

    fn lookup_resolved_type_cache(
        &self,
        dep_canonical: &str,
        dep_source_hash: ResolverHash16,
        type_name: &str,
        kind: ResolveRequestKind,
    ) -> Option<ExternalTypeResolvedCacheEntry>;

    fn note_resolved_type_cache_hit(&self) {}

    fn note_resolved_type_cache_miss(&self) {}

    fn note_route_fact_reuse(&self) {}

    fn resolve_type_via_registry(
        &self,
        _canonical: &str,
        _type_name: &str,
        _kind: ResolveRequestKind,
        _visited: &mut FxHashSet<(String, String)>,
    ) -> Option<RegistryRoute> {
        None
    }

    fn read_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String>;

    fn store_resolved_type_cache(
        &self,
        _dep_canonical: &str,
        _dep_source_hash: ResolverHash16,
        _type_name: &str,
        _kind: ResolveRequestKind,
        _resolved: Option<ResolvedElements>,
        _tracked_deps: Vec<String>,
    ) {
    }

    fn store_route_cache(
        &self,
        _owner_canonical: &str,
        _import_source: &str,
        _type_name: &str,
        _kind: ResolveRequestKind,
        _entry: ExternalTypeRouteEntry,
    ) {
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_request<R: ExternalTypeRequestResolver>(
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
        resolver.resolve_loaded_dependency_canonical(owner_canonical, import_source, kind)
    else {
        return if required_root_dep {
            Err(resolver.missing_root_dependency())
        } else {
            Ok(None)
        };
    };

    let debug_enabled = resolver.debug_enabled();
    tracked_deps.insert(dep_canonical.clone());
    resolution_deps.insert(dep_canonical.clone());

    if use_host_cache && profile_hash.is_none() {
        if let Some(route_entry) =
            resolver.lookup_route_cache(owner_canonical, import_source, type_name, kind)
        {
            resolver.note_route_fact_reuse();
            for dep in &route_entry.tracked_deps {
                tracked_deps.insert(dep.clone());
                resolution_deps.insert(dep.clone());
            }

            if let Some(target) = &route_entry.target {
                if debug_enabled {
                    resolver.debug_log(format!(
                        "resolve_external_type route-cache-hit owner={} type={} -> {}#{}",
                        owner_canonical, type_name, target.final_canonical_id, target.exported_name
                    ));
                }

                if let Some(final_source) =
                    resolver.read_source_for_type_resolution(&target.final_canonical_id, None)
                {
                    let target_hash = resolver.compute_source_hash(&final_source);
                    if let Some(entry) = resolver.lookup_resolved_type_cache(
                        &target.final_canonical_id,
                        target_hash,
                        &target.exported_name,
                        kind,
                    ) {
                        resolver.note_resolved_type_cache_hit();
                        for dep in &entry.tracked_deps {
                            tracked_deps.insert(dep.clone());
                            resolution_deps.insert(dep.clone());
                        }
                        cache.insert(
                            (dep_canonical.clone(), type_name.to_string()),
                            entry.resolved.clone(),
                        );
                        return Ok(entry.resolved);
                    }
                }
            } else {
                if debug_enabled {
                    resolver.debug_log(format!(
                        "resolve_external_type route-cache-negative owner={} type={}",
                        owner_canonical, type_name
                    ));
                }
                return Ok(None);
            }
        }
    }

    let cache_key = (dep_canonical.clone(), type_name.to_string());
    let mut effective_dep_canonical = dep_canonical.clone();
    let mut effective_type_name = type_name.to_string();
    let mut registry_route_target: Option<RegistryResolvedTarget> = None;
    let mut registry_route_hashes: Vec<(String, ResolverHash16)> = Vec::new();
    if debug_enabled {
        resolver.debug_log(format!(
            "resolve_external_type enter depth={} owner={} import={} dep={} type={}",
            visiting.len(),
            owner_canonical,
            import_source,
            dep_canonical,
            type_name,
        ));
    }
    if let Some(cached) = cache.get(&cache_key) {
        if debug_enabled {
            resolver.debug_log(format!(
                "resolve_external_type cache-hit dep={} type={} hit={}",
                dep_canonical,
                type_name,
                cached.is_some(),
            ));
        }
        return Ok(cached.clone());
    }

    if use_host_cache && profile_hash.is_none() {
        let mut registry_visited = FxHashSet::default();
        if let Some(route) = resolver.resolve_type_via_registry(
            &dep_canonical,
            type_name,
            kind,
            &mut registry_visited,
        ) {
            if let Some(target) = &route.target {
                if let Some(target_source) = resolver
                    .read_source_for_type_resolution(&target.final_canonical_id, profile_hash)
                {
                    let target_hash = resolver.compute_source_hash(&target_source);
                    if let Some(entry) = resolver.lookup_resolved_type_cache(
                        &target.final_canonical_id,
                        target_hash,
                        &target.exported_name,
                        kind,
                    ) {
                        resolver.note_resolved_type_cache_hit();
                        for dep in &entry.tracked_deps {
                            tracked_deps.insert(dep.clone());
                            resolution_deps.insert(dep.clone());
                        }
                        for dep in &route.tracked_deps {
                            tracked_deps.insert(dep.clone());
                            resolution_deps.insert(dep.clone());
                        }
                        cache.insert(cache_key.clone(), entry.resolved.clone());
                        if debug_enabled {
                            resolver.debug_log(format!(
                                "resolve_external_type registry-hit dep={} type={} -> {}#{}",
                                dep_canonical,
                                type_name,
                                target.final_canonical_id,
                                target.exported_name
                            ));
                        }
                        if depth == 0 {
                            resolver.store_route_cache(
                                owner_canonical,
                                import_source,
                                type_name,
                                kind,
                                ExternalTypeRouteEntry {
                                    owner_hash: resolver.whole_hash(owner_canonical),
                                    target: Some(target.clone()),
                                    tracked_deps: resolution_deps.iter().cloned().collect(),
                                    route_hashes: route.route_hashes.clone(),
                                    negative_barrel_generation: None,
                                },
                            );
                        }
                        return Ok(entry.resolved);
                    }
                }
                if debug_enabled {
                    resolver.debug_log(format!(
                        "resolve_external_type registry-route-no-payload dep={} type={} -> {}#{}",
                        dep_canonical, type_name, target.final_canonical_id, target.exported_name
                    ));
                }
                for dep in &route.tracked_deps {
                    tracked_deps.insert(dep.clone());
                    resolution_deps.insert(dep.clone());
                }
                effective_dep_canonical = target.final_canonical_id.clone();
                effective_type_name = target.exported_name.clone();
                registry_route_target = Some(target.clone());
                registry_route_hashes = route.route_hashes;
            }
        }
    }

    let effective_source =
        match resolver.read_source_for_type_resolution(&effective_dep_canonical, profile_hash) {
            Some(source) => source,
            None => {
                if effective_dep_canonical.ends_with(".vue") {
                    cache.insert(cache_key.clone(), None);
                    return Ok(None);
                }
                return if required_root_dep {
                    Err(resolver.missing_root_dependency())
                } else {
                    Ok(None)
                };
            }
        };

    let dep_source_hash = if profile_hash.is_none() {
        Some(resolver.compute_source_hash(&effective_source))
    } else {
        None
    };

    if use_host_cache {
        if let Some(dep_hash) = dep_source_hash {
            if let Some(entry) = resolver.lookup_resolved_type_cache(
                &effective_dep_canonical,
                dep_hash,
                &effective_type_name,
                kind,
            ) {
                resolver.note_resolved_type_cache_hit();
                for dep in &entry.tracked_deps {
                    tracked_deps.insert(dep.clone());
                    resolution_deps.insert(dep.clone());
                }
                cache.insert(cache_key.clone(), entry.resolved.clone());
                cache.insert(
                    (effective_dep_canonical.clone(), effective_type_name.clone()),
                    entry.resolved.clone(),
                );
                return Ok(entry.resolved);
            }
            resolver.note_resolved_type_cache_miss();
        }
    }

    let resolved = resolve_external_type_from_source_body(
        resolver,
        &effective_dep_canonical,
        &effective_type_name,
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

    if use_host_cache {
        if let Some(dep_hash) = dep_source_hash {
            resolver.store_resolved_type_cache(
                &effective_dep_canonical,
                dep_hash,
                &effective_type_name,
                kind,
                resolved.clone(),
                resolution_deps.iter().cloned().collect(),
            );
        }
    }

    cache.insert(cache_key.clone(), resolved.clone());
    cache.insert(
        (effective_dep_canonical.clone(), effective_type_name.clone()),
        resolved.clone(),
    );

    if depth == 0 && use_host_cache && profile_hash.is_none() {
        let mut route_hashes: Vec<(String, ResolverHash16)> = resolution_deps
            .iter()
            .map(|dep| (dep.clone(), resolver.whole_hash(dep)))
            .collect();
        for (canonical, hash) in registry_route_hashes {
            if !route_hashes
                .iter()
                .any(|(existing_canonical, _)| existing_canonical == &canonical)
            {
                route_hashes.push((canonical, hash));
            }
        }
        resolver.store_route_cache(
            owner_canonical,
            import_source,
            type_name,
            kind,
            ExternalTypeRouteEntry {
                owner_hash: resolver.whole_hash(owner_canonical),
                target: resolved.as_ref().map(|_| {
                    registry_route_target.unwrap_or(RegistryResolvedTarget {
                        final_canonical_id: effective_dep_canonical.clone(),
                        exported_name: effective_type_name.clone(),
                    })
                }),
                tracked_deps: resolution_deps.iter().cloned().collect(),
                route_hashes,
                negative_barrel_generation: None,
            },
        );
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_external_type_request, ExternalTypeBodyCache, ExternalTypeBodyResolver,
        ExternalTypeRequestResolver, ExternalTypeResolvedCacheEntry, ExternalTypeRouteEntry,
    };
    use crate::{RegistryResolvedTarget, RegistryRoute, ResolverHash16};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_analysis::hash_16;
    use verter_core::utils::oxc::vue::resolve_type::{
        AnalyzedExternalTypeSource, ResolvedElements, RuntimeType,
    };
    use verter_vfs::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        dependency_routes: FxHashMap<(String, String, ResolveRequestKind), String>,
        route_cache:
            FxHashMap<(String, String, String, ResolveRequestKind), ExternalTypeRouteEntry>,
        resolved_cache: RefCell<
            FxHashMap<
                (String, ResolverHash16, String, ResolveRequestKind),
                ExternalTypeResolvedCacheEntry,
            >,
        >,
        registry_routes: FxHashMap<(String, String, ResolveRequestKind), RegistryRoute>,
        sources: FxHashMap<String, String>,
        recursive_results: FxHashMap<(String, String, String), Option<ResolvedElements>>,
        stored_routes: RefCell<
            FxHashMap<(String, String, String, ResolveRequestKind), ExternalTypeRouteEntry>,
        >,
        cache_hits: RefCell<usize>,
        cache_misses: RefCell<usize>,
    }

    impl ExternalTypeBodyResolver for TestResolver {
        type Error = String;

        fn max_external_type_resolve_steps(&self) -> usize {
            64
        }

        fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
            format!("step limit exceeded for {type_name} at {last_dep}")
        }

        fn resolve_external_type_from_analysis(
            &self,
            _dep_canonical: &str,
            type_name: &str,
            effective_source: &str,
            analysis: &AnalyzedExternalTypeSource,
            imported_companions: &FxHashMap<String, ResolvedElements>,
        ) -> Option<ResolvedElements> {
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

        fn resolve_external_type_recursive(
            &self,
            owner_canonical: &str,
            import_source: &str,
            type_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
            _required_root_dep: bool,
            _kind: ResolveRequestKind,
            _use_host_cache: bool,
            _profile_hash: Option<u64>,
            _depth: usize,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
            Ok(self
                .recursive_results
                .get(&(
                    owner_canonical.to_string(),
                    import_source.to_string(),
                    type_name.to_string(),
                ))
                .cloned()
                .flatten())
        }

        fn resolve_type_through_barrel(
            &self,
            _barrel_canonical: &str,
            _type_name: &str,
            _wildcard_sources: &[String],
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
            _kind: ResolveRequestKind,
            _use_host_cache: bool,
            _profile_hash: Option<u64>,
            _depth: usize,
            _debug_enabled: bool,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
            Ok(None)
        }
    }

    impl ExternalTypeRequestResolver for TestResolver {
        fn max_external_type_resolve_depth(&self) -> usize {
            16
        }

        fn missing_root_dependency(&self) -> Self::Error {
            "missing root dependency".to_string()
        }

        fn depth_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
            format!("depth limit exceeded for {type_name} at {last_dep}")
        }

        fn resolve_loaded_dependency_canonical(
            &self,
            owner_canonical: &str,
            import_source: &str,
            kind: ResolveRequestKind,
        ) -> Option<String> {
            self.dependency_routes
                .get(&(owner_canonical.to_string(), import_source.to_string(), kind))
                .cloned()
        }

        fn lookup_route_cache(
            &self,
            owner_canonical: &str,
            import_source: &str,
            type_name: &str,
            kind: ResolveRequestKind,
        ) -> Option<ExternalTypeRouteEntry> {
            self.route_cache
                .get(&(
                    owner_canonical.to_string(),
                    import_source.to_string(),
                    type_name.to_string(),
                    kind,
                ))
                .cloned()
        }

        fn whole_hash(&self, canonical: &str) -> ResolverHash16 {
            hash_16(canonical.as_bytes())
        }

        fn compute_source_hash(&self, source: &str) -> ResolverHash16 {
            hash_16(source.as_bytes())
        }

        fn lookup_resolved_type_cache(
            &self,
            dep_canonical: &str,
            dep_source_hash: ResolverHash16,
            type_name: &str,
            kind: ResolveRequestKind,
        ) -> Option<ExternalTypeResolvedCacheEntry> {
            self.resolved_cache
                .borrow()
                .get(&(
                    dep_canonical.to_string(),
                    dep_source_hash,
                    type_name.to_string(),
                    kind,
                ))
                .cloned()
        }

        fn note_resolved_type_cache_hit(&self) {
            *self.cache_hits.borrow_mut() += 1;
        }

        fn note_resolved_type_cache_miss(&self) {
            *self.cache_misses.borrow_mut() += 1;
        }

        fn resolve_type_via_registry(
            &self,
            canonical: &str,
            type_name: &str,
            kind: ResolveRequestKind,
            _visited: &mut FxHashSet<(String, String)>,
        ) -> Option<RegistryRoute> {
            self.registry_routes
                .get(&(canonical.to_string(), type_name.to_string(), kind))
                .cloned()
        }

        fn read_source_for_type_resolution(
            &self,
            dep_canonical: &str,
            _profile_hash: Option<u64>,
        ) -> Option<String> {
            self.sources.get(dep_canonical).cloned()
        }

        fn store_resolved_type_cache(
            &self,
            dep_canonical: &str,
            dep_source_hash: ResolverHash16,
            type_name: &str,
            kind: ResolveRequestKind,
            resolved: Option<ResolvedElements>,
            tracked_deps: Vec<String>,
        ) {
            self.resolved_cache.borrow_mut().insert(
                (
                    dep_canonical.to_string(),
                    dep_source_hash,
                    type_name.to_string(),
                    kind,
                ),
                ExternalTypeResolvedCacheEntry {
                    resolved,
                    tracked_deps,
                },
            );
        }

        fn store_route_cache(
            &self,
            owner_canonical: &str,
            import_source: &str,
            type_name: &str,
            kind: ResolveRequestKind,
            entry: ExternalTypeRouteEntry,
        ) {
            self.stored_routes.borrow_mut().insert(
                (
                    owner_canonical.to_string(),
                    import_source.to_string(),
                    type_name.to_string(),
                    kind,
                ),
                entry,
            );
        }
    }

    fn empty_elements() -> ResolvedElements {
        ResolvedElements {
            props: Vec::new(),
            emits: Vec::new(),
            has_call_signature: false,
            root_runtime_types: vec![RuntimeType::Object],
        }
    }

    #[test]
    fn resolve_external_type_request_replays_route_cache_and_payload_deps() {
        let mut resolver = TestResolver::default();
        resolver.dependency_routes.insert(
            (
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            "/src/dep.ts".to_string(),
        );
        resolver.route_cache.insert(
            (
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            ExternalTypeRouteEntry {
                owner_hash: hash_16(b"/src/owner.ts"),
                target: Some(RegistryResolvedTarget {
                    final_canonical_id: "/src/final.ts".to_string(),
                    exported_name: "Props".to_string(),
                }),
                tracked_deps: vec!["/src/route.ts".to_string()],
                route_hashes: Vec::new(),
                negative_barrel_generation: None,
            },
        );
        resolver.sources.insert(
            "/src/final.ts".to_string(),
            "export interface Props {}".to_string(),
        );
        let final_hash = hash_16(b"export interface Props {}");
        resolver.resolved_cache.borrow_mut().insert(
            (
                "/src/final.ts".to_string(),
                final_hash,
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            ExternalTypeResolvedCacheEntry {
                resolved: Some(empty_elements()),
                tracked_deps: vec!["/src/payload.ts".to_string()],
            },
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_request(
            &resolver,
            "/src/owner.ts",
            "./dep",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("route cache replay should succeed");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/dep.ts"));
        assert!(tracked.contains("/src/route.ts"));
        assert!(tracked.contains("/src/payload.ts"));
        assert!(resolution.contains("/src/dep.ts"));
        assert!(resolution.contains("/src/route.ts"));
        assert!(resolution.contains("/src/payload.ts"));
        assert_eq!(*resolver.cache_hits.borrow(), 1);
    }

    #[test]
    fn resolve_external_type_request_uses_registry_route_when_payload_is_cached() {
        let mut resolver = TestResolver::default();
        resolver.dependency_routes.insert(
            (
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            "/src/dep.ts".to_string(),
        );
        resolver.registry_routes.insert(
            (
                "/src/dep.ts".to_string(),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            RegistryRoute {
                target: Some(RegistryResolvedTarget {
                    final_canonical_id: "/src/final.ts".to_string(),
                    exported_name: "Props".to_string(),
                }),
                tracked_deps: vec!["/src/registry.ts".to_string()],
                route_hashes: Vec::new(),
            },
        );
        resolver.sources.insert(
            "/src/final.ts".to_string(),
            "export interface Props {}".to_string(),
        );
        resolver.resolved_cache.borrow_mut().insert(
            (
                "/src/final.ts".to_string(),
                hash_16(b"export interface Props {}"),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            ExternalTypeResolvedCacheEntry {
                resolved: Some(empty_elements()),
                tracked_deps: vec!["/src/payload.ts".to_string()],
            },
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_request(
            &resolver,
            "/src/owner.ts",
            "./dep",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("registry route should succeed");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/dep.ts"));
        assert!(tracked.contains("/src/registry.ts"));
        assert!(tracked.contains("/src/payload.ts"));
        assert_eq!(*resolver.cache_hits.borrow(), 1);
    }

    #[test]
    fn resolve_external_type_request_uses_registry_target_source_when_payload_is_cold() {
        let mut resolver = TestResolver::default();
        resolver.dependency_routes.insert(
            (
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            "/src/dep.ts".to_string(),
        );
        resolver.registry_routes.insert(
            (
                "/src/dep.ts".to_string(),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            RegistryRoute {
                target: Some(RegistryResolvedTarget {
                    final_canonical_id: "/src/final.ts".to_string(),
                    exported_name: "InnerProps".to_string(),
                }),
                tracked_deps: vec!["/src/registry.ts".to_string()],
                route_hashes: vec![("/src/registry.ts".to_string(), hash_16(b"/src/registry.ts"))],
            },
        );
        resolver.sources.insert(
            "/src/final.ts".to_string(),
            "export interface InnerProps { label: string }".to_string(),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_request(
            &resolver,
            "/src/owner.ts",
            "./dep",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("cold registry route should resolve directly from the target source");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/dep.ts"));
        assert!(tracked.contains("/src/registry.ts"));
        let stored_routes = resolver.stored_routes.borrow();
        let route_entry = stored_routes
            .get(&(
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ))
            .expect("cold registry route should still persist the final route");
        let target = route_entry
            .target
            .as_ref()
            .expect("successful registry-target resolution should store a target");
        assert_eq!(target.final_canonical_id, "/src/final.ts");
        assert_eq!(target.exported_name, "InnerProps");
        assert!(
            resolver
                .resolved_cache
                .borrow()
                .keys()
                .any(|(dep, _, ty, _)| dep == "/src/final.ts" && ty == "InnerProps"),
            "cold registry route should cache the resolved payload under the final target",
        );
    }

    #[test]
    fn resolve_external_type_request_stores_root_caches_after_body_resolution() {
        let mut resolver = TestResolver::default();
        resolver.dependency_routes.insert(
            (
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                ResolveRequestKind::TypeImport,
            ),
            "/src/dep.ts".to_string(),
        );
        resolver.sources.insert(
            "/src/dep.ts".to_string(),
            "export { Props } from './inner'".to_string(),
        );
        resolver.recursive_results.insert(
            (
                "/src/dep.ts".to_string(),
                "./inner".to_string(),
                "Props".to_string(),
            ),
            Some(empty_elements()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_request(
            &resolver,
            "/src/owner.ts",
            "./dep",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("body-based resolution should succeed");

        assert!(actual.is_some());
        let dep_hash = hash_16(b"export { Props } from './inner'");
        assert!(resolver.resolved_cache.borrow().contains_key(&(
            "/src/dep.ts".to_string(),
            dep_hash,
            "Props".to_string(),
            ResolveRequestKind::TypeImport,
        )));
        let stored_routes = resolver.stored_routes.borrow();
        let route_entry = stored_routes
            .get(&(
                "/src/owner.ts".to_string(),
                "./dep".to_string(),
                "Props".to_string(),
                ResolveRequestKind::TypeImport,
            ))
            .expect("root route cache should be stored");
        let target = route_entry
            .target
            .as_ref()
            .expect("successful resolution should store a target");
        assert_eq!(target.final_canonical_id, "/src/dep.ts");
        assert_eq!(target.exported_name, "Props");
    }
}
