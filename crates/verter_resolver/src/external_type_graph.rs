use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_core::utils::oxc::vue::resolve_type::{extract_export_surface, ResolvedElements};
use verter_vfs::ResolveRequestKind;

use crate::{
    resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
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

    fn read_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String>;

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
    let specifier_to_canonical: Vec<(String, String)> = wildcard_sources
        .iter()
        .filter_map(|spec| {
            resolver
                .resolve_dependency_canonical(barrel_canonical, spec, kind)
                .map(|canonical| (spec.clone(), canonical))
        })
        .collect();

    let mut export_map = FxHashMap::default();
    let mut scanned = FxHashSet::default();
    let mut transitive_deps = BTreeSet::new();

    for (child_specifier, child_canonical) in &specifier_to_canonical {
        let mut visited = FxHashSet::default();
        scan_barrel_export_surface_recursive_from_graph(
            resolver,
            child_specifier,
            child_canonical,
            &mut export_map,
            &mut scanned,
            &mut transitive_deps,
            &mut visited,
            kind,
            profile_hash,
        );

        if export_map.contains_key(type_name) {
            tracked_deps.extend(transitive_deps.iter().cloned());
            resolution_deps.extend(transitive_deps.iter().cloned());
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

    tracked_deps.extend(transitive_deps.iter().cloned());
    resolution_deps.extend(transitive_deps.iter().cloned());

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
    export_map: &mut FxHashMap<String, (String, String)>,
    scanned: &mut FxHashSet<String>,
    tracked_deps: &mut BTreeSet<String>,
    visited: &mut FxHashSet<String>,
    kind: ResolveRequestKind,
    profile_hash: Option<u64>,
) {
    if !visited.insert(current_canonical.to_string()) {
        return;
    }
    if !scanned.insert(current_canonical.to_string()) {
        return;
    }

    tracked_deps.insert(current_canonical.to_string());
    let Some(current_source) =
        resolver.read_source_for_type_resolution(current_canonical, profile_hash)
    else {
        return;
    };

    let alloc = oxc_allocator::Allocator::new();
    let surface = extract_export_surface(&current_source, &alloc);

    for name in &surface.exported_names {
        export_map
            .entry(name.clone())
            .or_insert_with(|| (root_specifier.to_string(), current_canonical.to_string()));
    }

    for nested_specifier in &surface.wildcard_reexport_sources {
        let Some(nested_canonical) =
            resolver.resolve_dependency_canonical(current_canonical, nested_specifier, kind)
        else {
            continue;
        };
        scan_barrel_export_surface_recursive_from_graph(
            resolver,
            root_specifier,
            &nested_canonical,
            export_map,
            scanned,
            tracked_deps,
            visited,
            kind,
            profile_hash,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_external_type_from_graph, ExternalTypeGraphResolver};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_vfs::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        routes: FxHashMap<(String, String), String>,
        sources: FxHashMap<String, String>,
        named_export_targets: FxHashMap<(String, String, ResolveRequestKind), (String, String)>,
        logs: RefCell<Vec<String>>,
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

        fn read_source_for_type_resolution(
            &self,
            dep_canonical: &str,
            _profile_hash: Option<u64>,
        ) -> Option<String> {
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
        let mut cache = FxHashMap::default();
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
        let mut cache = FxHashMap::default();
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
        let mut cache = FxHashMap::default();
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
}
