use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_core::utils::oxc::vue::resolve_type::{extract_export_surface, ResolvedElements};
use verter_workspace::ResolveRequestKind;

use crate::{ExportRegistryView, ExternalTypeBodyCache, RegistryExportEntry, ResolverHash16};

#[derive(Debug, Clone)]
pub struct BarrelResolutionState {
    pub export_map: FxHashMap<String, (String, String)>,
    pub source_hash: ResolverHash16,
    pub wildcard_sources: Vec<String>,
    pub scanned_sources: FxHashMap<String, ResolverHash16>,
    pub tracked_deps: FxHashSet<String>,
    pub fully_resolved: bool,
    pub generation: u64,
}

pub trait BarrelResolutionResolver {
    type Error;

    fn cached_barrel_state(&self, _barrel_canonical: &str) -> Option<BarrelResolutionState> {
        None
    }

    fn source_hash(&self, canonical: &str) -> ResolverHash16;

    fn resolve_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: ResolveRequestKind,
    ) -> Option<String>;

    fn read_source_for_type_resolution(
        &self,
        canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<Option<ResolvedElements>, Self::Error>;

    fn persist_barrel_state(
        &self,
        _barrel_canonical: &str,
        _state: &BarrelResolutionState,
        _replace_existing: bool,
    ) {
    }

    fn note_barrel_fact_reuse(&self) {}

    fn ensure_export_registry(&self, _canonical: &str) -> Option<ExportRegistryView> {
        None
    }

    fn debug_enabled(&self) -> bool {
        false
    }

    fn debug_log(&self, _message: String) {}
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_type_through_barrel<R: BarrelResolutionResolver>(
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
    let rebuilt_from_scratch = valid_barrel.is_none();

    if let Some(ref barrel_state) = valid_barrel {
        resolver.note_barrel_fact_reuse();
        cache.store_barrel_state(barrel_canonical, barrel_state.clone());
        tracked_deps.extend(barrel_state.tracked_deps.iter().cloned());
        resolution_deps.extend(barrel_state.tracked_deps.iter().cloned());

        if let Some((source_specifier, _)) = barrel_state.export_map.get(type_name) {
            if resolver.debug_enabled() {
                resolver.debug_log(format!(
                    "resolve_external_type barrel-cache-hit dep={} type={} -> {}",
                    barrel_canonical, type_name, source_specifier
                ));
            }
            return resolver.resolve_external_type_recursive(
                barrel_canonical,
                source_specifier,
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

        if barrel_state.fully_resolved {
            if resolver.debug_enabled() {
                resolver.debug_log(format!(
                    "resolve_external_type barrel-fully-resolved-absent dep={} type={}",
                    barrel_canonical, type_name
                ));
            }
            return Ok(None);
        }
    }

    let mut state = valid_barrel.unwrap_or_else(|| BarrelResolutionState {
        export_map: FxHashMap::default(),
        source_hash: barrel_source_hash,
        wildcard_sources: wildcard_sources.to_vec(),
        scanned_sources: FxHashMap::default(),
        tracked_deps: FxHashSet::default(),
        fully_resolved: false,
        generation: 0,
    });

    let specifier_to_canonical: Vec<(String, String)> = wildcard_sources
        .iter()
        .filter_map(|spec| {
            let canonical = resolver.resolve_dependency_canonical(barrel_canonical, spec, kind)?;
            Some((spec.to_string(), canonical))
        })
        .collect();

    let sources_to_scan: Vec<(String, String)> = specifier_to_canonical
        .iter()
        .filter(|(_, canonical)| !state.scanned_sources.contains_key(canonical))
        .cloned()
        .collect();

    for (child_specifier, child_canonical) in &sources_to_scan {
        let mut visited = FxHashSet::default();
        scan_barrel_export_surface_recursive(
            resolver,
            child_specifier,
            child_canonical,
            type_name,
            &mut state,
            &mut visited,
            kind,
            profile_hash,
        );

        if state.export_map.contains_key(type_name) {
            cache.store_barrel_state(barrel_canonical, state.clone());
            resolver.persist_barrel_state(barrel_canonical, &state, rebuilt_from_scratch);
            let resolved_from_source = resolver.resolve_external_type_recursive(
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
            )?;

            tracked_deps.extend(state.tracked_deps.iter().cloned());
            resolution_deps.extend(state.tracked_deps.iter().cloned());
            return Ok(resolved_from_source);
        }
    }

    let all_scanned = specifier_to_canonical
        .iter()
        .all(|(_, canonical)| state.scanned_sources.contains_key(canonical));
    if all_scanned {
        state.fully_resolved = true;
    }

    cache.store_barrel_state(barrel_canonical, state.clone());
    resolver.persist_barrel_state(barrel_canonical, &state, rebuilt_from_scratch);
    tracked_deps.extend(state.tracked_deps.iter().cloned());
    resolution_deps.extend(state.tracked_deps.iter().cloned());

    for (child_specifier, _) in &specifier_to_canonical {
        if let Some(found) = resolver.resolve_external_type_recursive(
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
            let child_canonical_for_map = resolver
                .resolve_dependency_canonical(barrel_canonical, child_specifier, kind)
                .unwrap_or_default();
            let map_entry = (child_specifier.clone(), child_canonical_for_map);
            state
                .export_map
                .entry(type_name.to_string())
                .or_insert_with(|| map_entry.clone());
            cache.store_barrel_state(barrel_canonical, state.clone());
            resolver.persist_barrel_state(barrel_canonical, &state, false);
            return Ok(Some(found));
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn scan_barrel_export_surface_recursive<R: BarrelResolutionResolver>(
    resolver: &R,
    root_specifier: &str,
    current_canonical: &str,
    type_name: &str,
    state: &mut BarrelResolutionState,
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

    let wildcard_sources = if let Some(registry) =
        resolver.ensure_export_registry(current_canonical)
    {
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

        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type barrel-scan-registry child={} exports={} nested_wildcards={}",
                current_canonical,
                registry.named.len(),
                registry.wildcard_edges.len(),
            ));
        }

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

        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type barrel-scan child={} exports={} nested_wildcards={}",
                current_canonical,
                surface.exported_names.len(),
                surface.wildcard_reexport_sources.len(),
            ));
        }

        surface.wildcard_reexport_sources
    };

    for nested_specifier in &wildcard_sources {
        let Some(nested_canonical) =
            resolver.resolve_dependency_canonical(current_canonical, nested_specifier, kind)
        else {
            continue;
        };
        scan_barrel_export_surface_recursive(
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
    use super::{resolve_type_through_barrel, BarrelResolutionResolver, BarrelResolutionState};
    use crate::{ExportRegistryView, ExternalTypeBodyCache, RegistryExportEntry, ResolverHash16};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_analysis::hash_16;
    use verter_core::utils::oxc::vue::resolve_type::{ResolvedElements, RuntimeType};
    use verter_workspace::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        cached: Option<BarrelResolutionState>,
        hashes: FxHashMap<String, ResolverHash16>,
        routes: FxHashMap<(String, String), String>,
        sources: FxHashMap<String, String>,
        registries: FxHashMap<String, ExportRegistryView>,
        recursive_results: FxHashMap<(String, String, String), Option<ResolvedElements>>,
        persisted: RefCell<Vec<(String, BarrelResolutionState, bool)>>,
        source_reads: RefCell<FxHashMap<String, usize>>,
    }

    impl BarrelResolutionResolver for TestResolver {
        type Error = String;

        fn cached_barrel_state(&self, _barrel_canonical: &str) -> Option<BarrelResolutionState> {
            self.cached.clone()
        }

        fn source_hash(&self, canonical: &str) -> ResolverHash16 {
            self.hashes
                .get(canonical)
                .copied()
                .unwrap_or_else(|| hash_16(canonical.as_bytes()))
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

        fn read_source_for_type_resolution(
            &self,
            canonical: &str,
            _profile_hash: Option<u64>,
        ) -> Option<String> {
            *self
                .source_reads
                .borrow_mut()
                .entry(canonical.to_string())
                .or_default() += 1;
            self.sources.get(canonical).cloned()
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

        fn persist_barrel_state(
            &self,
            barrel_canonical: &str,
            state: &BarrelResolutionState,
            replace_existing: bool,
        ) {
            self.persisted.borrow_mut().push((
                barrel_canonical.to_string(),
                state.clone(),
                replace_existing,
            ));
        }

        fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView> {
            self.registries.get(canonical).cloned()
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
    fn resolve_type_through_barrel_uses_cached_export_map_hit() {
        let resolver = TestResolver {
            cached: Some(BarrelResolutionState {
                export_map: FxHashMap::from_iter([(
                    "Props".to_string(),
                    ("./child".to_string(), "/src/child.ts".to_string()),
                )]),
                source_hash: hash_16(b"/src/barrel.ts"),
                wildcard_sources: vec!["./child".to_string()],
                scanned_sources: FxHashMap::from_iter([(
                    "/src/child.ts".to_string(),
                    hash_16(b"/src/child.ts"),
                )]),
                tracked_deps: FxHashSet::from_iter(["/src/child.ts".to_string()]),
                fully_resolved: false,
                generation: 1,
            }),
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/child.ts".to_string(), hash_16(b"/src/child.ts")),
            ]),
            recursive_results: FxHashMap::from_iter([(
                (
                    "/src/barrel.ts".to_string(),
                    "./child".to_string(),
                    "Props".to_string(),
                ),
                Some(empty_elements()),
            )]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "Props",
            &["./child".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("cached barrel hit should resolve");

        assert!(actual.is_some());
        assert!(tracked.contains("/src/child.ts"));
        assert!(resolution.contains("/src/child.ts"));
    }

    #[test]
    fn resolve_type_through_barrel_scans_and_persists_discovered_child() {
        let resolver = TestResolver {
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/child.ts".to_string(), hash_16(b"/src/child.ts")),
            ]),
            routes: FxHashMap::from_iter([(
                ("/src/barrel.ts".to_string(), "./child".to_string()),
                "/src/child.ts".to_string(),
            )]),
            sources: FxHashMap::from_iter([(
                "/src/child.ts".to_string(),
                "export interface Props {}".to_string(),
            )]),
            recursive_results: FxHashMap::from_iter([(
                (
                    "/src/barrel.ts".to_string(),
                    "./child".to_string(),
                    "Props".to_string(),
                ),
                Some(empty_elements()),
            )]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "Props",
            &["./child".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("progressive scan should resolve");

        assert!(actual.is_some());
        let persisted = resolver.persisted.borrow();
        assert!(!persisted.is_empty());
        let (_, state, _) = &persisted[0];
        assert!(state.export_map.contains_key("Props"));
        assert!(state.scanned_sources.contains_key("/src/child.ts"));
    }

    #[test]
    fn resolve_type_through_barrel_reuses_request_local_state_across_lookups() {
        let resolver = TestResolver {
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/a.ts".to_string(), hash_16(b"/src/a.ts")),
                ("/src/b.ts".to_string(), hash_16(b"/src/b.ts")),
            ]),
            routes: FxHashMap::from_iter([
                (
                    ("/src/barrel.ts".to_string(), "./a".to_string()),
                    "/src/a.ts".to_string(),
                ),
                (
                    ("/src/barrel.ts".to_string(), "./b".to_string()),
                    "/src/b.ts".to_string(),
                ),
            ]),
            sources: FxHashMap::from_iter([
                (
                    "/src/a.ts".to_string(),
                    "export interface Foo {}".to_string(),
                ),
                (
                    "/src/b.ts".to_string(),
                    "export interface Bar {}".to_string(),
                ),
            ]),
            recursive_results: FxHashMap::from_iter([
                (
                    (
                        "/src/barrel.ts".to_string(),
                        "./a".to_string(),
                        "Foo".to_string(),
                    ),
                    Some(empty_elements()),
                ),
                (
                    (
                        "/src/barrel.ts".to_string(),
                        "./b".to_string(),
                        "Bar".to_string(),
                    ),
                    Some(empty_elements()),
                ),
            ]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let first = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "Foo",
            &["./a".to_string(), "./b".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first lookup should resolve");
        assert!(first.is_some());

        let second = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "Bar",
            &["./a".to_string(), "./b".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second lookup should resolve from the extended local barrel state");
        assert!(second.is_some());

        let reads = resolver.source_reads.borrow();
        assert_eq!(
            reads.get("/src/a.ts").copied().unwrap_or_default(),
            1,
            "request-local barrel state should avoid rescanning the already-checked sibling",
        );
        assert_eq!(
            reads.get("/src/b.ts").copied().unwrap_or_default(),
            1,
            "the later lookup should only scan the newly-needed sibling once",
        );
    }

    #[test]
    fn resolve_type_through_barrel_preserves_wildcard_source_order_for_duplicate_exports() {
        let resolver = TestResolver {
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/legacy.ts".to_string(), hash_16(b"/src/legacy.ts")),
                ("/src/Button.ts".to_string(), hash_16(b"/src/Button.ts")),
            ]),
            routes: FxHashMap::from_iter([
                (
                    ("/src/barrel.ts".to_string(), "./legacy".to_string()),
                    "/src/legacy.ts".to_string(),
                ),
                (
                    ("/src/barrel.ts".to_string(), "./Button".to_string()),
                    "/src/Button.ts".to_string(),
                ),
            ]),
            sources: FxHashMap::from_iter([
                (
                    "/src/legacy.ts".to_string(),
                    "export interface ButtonProps { source: 'legacy' }".to_string(),
                ),
                (
                    "/src/Button.ts".to_string(),
                    "export interface ButtonProps { source: 'button' }".to_string(),
                ),
            ]),
            recursive_results: FxHashMap::from_iter([
                (
                    (
                        "/src/barrel.ts".to_string(),
                        "./legacy".to_string(),
                        "ButtonProps".to_string(),
                    ),
                    Some(empty_elements()),
                ),
                (
                    (
                        "/src/barrel.ts".to_string(),
                        "./Button".to_string(),
                        "ButtonProps".to_string(),
                    ),
                    Some(empty_elements()),
                ),
            ]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "ButtonProps",
            &["./legacy".to_string(), "./Button".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("barrel lookup should resolve");

        assert!(actual.is_some());
        let persisted = resolver.persisted.borrow();
        let (_, state, _) = persisted.last().expect("barrel state should be persisted");
        assert_eq!(
            state.export_map.get("ButtonProps"),
            Some(&("./legacy".to_string(), "/src/legacy.ts".to_string())),
            "barrel resolution must preserve declared wildcard source order when multiple children export the same type",
        );
    }

    #[test]
    fn resolve_type_through_barrel_uses_export_registry_before_source_parse() {
        let resolver = TestResolver {
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/child.ts".to_string(), hash_16(b"/src/child.ts")),
            ]),
            routes: FxHashMap::from_iter([(
                ("/src/barrel.ts".to_string(), "./child".to_string()),
                "/src/child.ts".to_string(),
            )]),
            registries: FxHashMap::from_iter([(
                "/src/child.ts".to_string(),
                ExportRegistryView {
                    source_hash: hash_16(b"/src/child.ts"),
                    named: FxHashMap::from_iter([(
                        "Props".to_string(),
                        RegistryExportEntry::Defined,
                    )]),
                    wildcard_edges: Vec::new(),
                },
            )]),
            sources: FxHashMap::from_iter([(
                "/src/child.ts".to_string(),
                "export interface Props {}".to_string(),
            )]),
            recursive_results: FxHashMap::from_iter([(
                (
                    "/src/barrel.ts".to_string(),
                    "./child".to_string(),
                    "Props".to_string(),
                ),
                Some(empty_elements()),
            )]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "Props",
            &["./child".to_string()],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("registry-backed scan should resolve");

        assert!(actual.is_some());
        assert_eq!(
            resolver
                .source_reads
                .borrow()
                .get("/src/child.ts")
                .copied()
                .unwrap_or_default(),
            0,
            "barrel export discovery should use the cached export registry instead of reparsing the child source",
        );
    }

    #[test]
    fn resolve_type_through_barrel_scans_declared_wildcard_order_until_match() {
        let resolver = TestResolver {
            hashes: FxHashMap::from_iter([
                ("/src/barrel.ts".to_string(), hash_16(b"/src/barrel.ts")),
                ("/src/a.ts".to_string(), hash_16(b"/src/a.ts")),
                ("/src/Button.vue".to_string(), hash_16(b"/src/Button.vue")),
                ("/src/z.ts".to_string(), hash_16(b"/src/z.ts")),
            ]),
            routes: FxHashMap::from_iter([
                (
                    ("/src/barrel.ts".to_string(), "./a".to_string()),
                    "/src/a.ts".to_string(),
                ),
                (
                    ("/src/barrel.ts".to_string(), "./Button.vue".to_string()),
                    "/src/Button.vue".to_string(),
                ),
                (
                    ("/src/barrel.ts".to_string(), "./z".to_string()),
                    "/src/z.ts".to_string(),
                ),
            ]),
            sources: FxHashMap::from_iter([
                (
                    "/src/a.ts".to_string(),
                    "export interface Foo { value: string }".to_string(),
                ),
                (
                    "/src/Button.vue".to_string(),
                    "export interface ButtonProps { label: string }".to_string(),
                ),
                (
                    "/src/z.ts".to_string(),
                    "export interface Bar { count: number }".to_string(),
                ),
            ]),
            recursive_results: FxHashMap::from_iter([(
                (
                    "/src/barrel.ts".to_string(),
                    "./Button.vue".to_string(),
                    "ButtonProps".to_string(),
                ),
                Some(empty_elements()),
            )]),
            ..Default::default()
        };

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_type_through_barrel(
            &resolver,
            "/src/barrel.ts",
            "ButtonProps",
            &[
                "./a".to_string(),
                "./Button.vue".to_string(),
                "./z".to_string(),
            ],
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("declared-order scan should resolve once it reaches the matching child");

        assert!(actual.is_some());
        let reads = resolver.source_reads.borrow();
        assert_eq!(
            reads.get("/src/a.ts").copied().unwrap_or_default(),
            1,
            "barrel scans should preserve declared wildcard order, including earlier siblings",
        );
        assert_eq!(
            reads.get("/src/Button.vue").copied().unwrap_or_default(),
            1,
            "the matching child should only be read once for export discovery when the recursive body lookup is mocked",
        );
        assert_eq!(
            reads.get("/src/z.ts").copied().unwrap_or_default(),
            0,
            "barrel scans should stop after the first declared matching child resolves the requested type",
        );
    }
}
