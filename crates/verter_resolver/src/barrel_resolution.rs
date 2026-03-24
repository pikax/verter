use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_core::utils::oxc::vue::resolve_type::{extract_export_surface, ResolvedElements};
use verter_vfs::ResolveRequestKind;

use crate::{ExternalTypeBodyCache, ResolverHash16};

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

    fn debug_enabled(&self) -> bool {
        false
    }

    fn debug_log(&self, _message: String) {}
}

#[allow(clippy::too_many_arguments)]
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
    let cached_barrel = resolver.cached_barrel_state(barrel_canonical);
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
            Some((spec.clone(), canonical))
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
            &mut state,
            &mut visited,
            kind,
            profile_hash,
        );

        if state.export_map.contains_key(type_name) {
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
            resolver.persist_barrel_state(barrel_canonical, &state, false);
            return Ok(Some(found));
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn scan_barrel_export_surface_recursive<R: BarrelResolutionResolver>(
    resolver: &R,
    root_specifier: &str,
    current_canonical: &str,
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

    let Some(current_source) = resolver.read_source_for_type_resolution(current_canonical, profile_hash)
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

    for nested_specifier in &surface.wildcard_reexport_sources {
        let Some(nested_canonical) =
            resolver.resolve_dependency_canonical(current_canonical, nested_specifier, kind)
        else {
            continue;
        };
        scan_barrel_export_surface_recursive(
            resolver,
            root_specifier,
            &nested_canonical,
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
    use crate::{ExternalTypeBodyCache, ResolverHash16};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_analysis::hash_16;
    use verter_core::utils::oxc::vue::resolve_type::{ResolvedElements, RuntimeType};
    use verter_vfs::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        cached: Option<BarrelResolutionState>,
        hashes: FxHashMap<String, ResolverHash16>,
        routes: FxHashMap<(String, String), String>,
        sources: FxHashMap<String, String>,
        recursive_results: FxHashMap<(String, String, String), Option<ResolvedElements>>,
        persisted: RefCell<Vec<(String, BarrelResolutionState, bool)>>,
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
}
